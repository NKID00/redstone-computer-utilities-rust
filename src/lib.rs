mod datatype;

use std::sync::Arc;

use futures::{FutureExt, SinkExt, TryStreamExt, future::BoxFuture};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::de::DeserializeOwned;
pub use serde_json;
use serde_json::Value;
use tokio::{net::TcpStream, select, signal::ctrl_c, sync::Mutex};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{self, Message},
};
use tracing::{debug, error, info, trace};
use trait_set::trait_set;

pub use crate::datatype::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unexpected websocket error")]
    WebsocketError(#[from] tungstenite::Error),
    #[error("unexpected api result from server while no api request was pending")]
    UnexpectedApiResult,
    #[error("invalid message from server")]
    InvalidServerMessage,
    #[error("unexpected disconnect from server")]
    UnexpectedDisconnect,
    #[error("scriptInitialize callback returned Err")]
    InitializeFailed,
    #[error("failed to serialize message")]
    SerializeFailed(#[from] serde_json::Error),
    #[error("error code from server")]
    ErrorCode(#[from] ErrorCode),
}

pub type Result<T> = std::result::Result<T, Error>;

trait_set! {
    trait OnInitCallback = FnOnce(Context) -> BoxFuture<'static, std::result::Result<(), ErrorCode>>;
    trait OnExecuteCallback = Fn(Context, Vec<serde_json::Value>) -> BoxFuture<'static, std::result::Result<i32, ErrorCode>>;
}

pub struct Script {
    name: String,
    description: String,
    server: String,
    on_init: Option<Box<dyn OnInitCallback + Send>>,
    on_execute: Option<Arc<dyn OnExecuteCallback + Send + Sync>>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            name: "example".to_owned(),
            description: "".to_owned(),
            server: "ws://localhost:37265/".to_owned(),
            on_init: None,
            on_execute: None,
        }
    }
}

impl Script {
    pub fn new(name: impl AsRef<str>) -> Self {
        Self::default().name(name)
    }

    pub fn name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_owned();
        self
    }

    pub fn description(mut self, description: impl AsRef<str>) -> Self {
        self.description = description.as_ref().to_owned();
        self
    }

    pub fn server(mut self, server: impl AsRef<str>) -> Self {
        self.server = server.as_ref().to_owned();
        self
    }

    pub fn on_init<F, Fut>(mut self, callback: F) -> Self
    where
        F: FnOnce(Context) -> Fut + Send + 'static,
        Fut: Future<Output = std::result::Result<(), ErrorCode>> + Send + 'static,
    {
        self.on_init = Some(Box::new(move |ctx| callback(ctx).boxed()));
        self
    }

    pub fn on_execute<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(Context, Vec<serde_json::Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<i32, ErrorCode>> + Send + 'static,
    {
        self.on_execute = Some(Arc::new(move |ctx, args| callback(ctx, args).boxed()));
        self
    }

    pub async fn run(self) -> Result<()> {
        info!("Connecting to server");
        let url = format!(
            "{}?name={}&description={}",
            self.server,
            url_encode_query(&self.name),
            url_encode_query(&self.description)
        );
        let (ws, _) = connect_async(url).await?;
        let ws = Arc::new(Mutex::new(ws));
        Context {
            script: Arc::new(Mutex::new(self)),
            ws: ws.clone(),
        }
        .main_loop()
        .await?;
        let mut lock = ws.lock().await;
        lock.close(None).await?;
        lock.flush().await?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Context {
    script: Arc<Mutex<Script>>,
    ws: Arc<Mutex<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
}

impl Context {
    async fn main_loop(&mut self) -> Result<()> {
        info!("Connected");
        loop {
            let Some(evt) = self.try_next_event().await? else {
                return Ok(());
            };
            Box::pin(self.handle_event(evt)).await?;
        }
    }

    async fn try_next_event(&mut self) -> Result<Option<Event>> {
        match self.try_next_event_or_api_result().await? {
            Some(EventOrApiResult::Event(evt)) => Ok(Some(evt)),
            Some(EventOrApiResult::ApiResult(res)) => {
                error!("unexpected api result {res:?}");
                Err(Error::UnexpectedApiResult)
            }
            None => Ok(None),
        }
    }

    async fn try_next_event_or_api_result(&mut self) -> Result<Option<EventOrApiResult>> {
        let Some(s) = self.try_next_message().await? else {
            return Ok(None);
        };
        if let Ok(evt) = serde_json::from_str::<Event>(&s) {
            trace!("event request {evt:?}");
            return Ok(Some(EventOrApiResult::Event(evt)));
        }
        if let Ok(res) = serde_json::from_str::<ApiResultWrapper>(&s) {
            trace!("api result {res:?}");
            return Ok(Some(EventOrApiResult::ApiResult(res)));
        }
        error!("unrecognized server message {s:?}");
        Err(Error::InvalidServerMessage)
    }

    async fn try_next_message(&mut self) -> Result<Option<String>> {
        loop {
            let Some(message) = ({
                let mut lock = self.ws.lock().await;
                select! {
                    message = lock.try_next() => {
                        message?
                    }
                    _ = ctrl_c() => {
                        info!("Shutdown");
                        return Ok(None);
                    }
                }
            }) else {
                return Ok(None);
            };
            match message {
                Message::Text(bytes) => return Ok(Some(bytes.to_string())),
                Message::Binary(b) => {
                    error!("unrecognized server message {b:?}");
                    return Err(Error::InvalidServerMessage);
                }
                Message::Close(_) => return Ok(None),
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn handle_event(&mut self, evt: Event) -> Result<()> {
        match evt {
            Event::ScriptInitialize {} => {
                info!("Initializing");
                if self.script.lock().await.on_execute.is_some() {
                    self.subscribe_run().await?;
                }
                if let Some(callback) = { self.script.lock().await.on_init.take() } {
                    debug!("Call on_init");
                    let result = callback(self.clone()).await;
                    if let Err(e) = result {
                        self.send_event_response(EventResponse::Err(e)).await?;
                        return Err(Error::InitializeFailed);
                    }
                }
                self.send_event_response(EventResponse::empty()).await?;
                info!("Running");
            }
            Event::ScriptRun { content } => {
                let result = if let Some(callback) = { self.script.lock().await.on_execute.clone() }
                {
                    debug!("Call on_execute");
                    let result = callback(self.clone(), content.argument).await;
                    match result {
                        Ok(result) => result,
                        Err(e) => {
                            self.send_event_response(EventResponse::Err(e)).await?;
                            return Ok(());
                        }
                    }
                } else {
                    0
                };
                self.send_event_response(EventResponse::ScriptRun { result })
                    .await?;
            }
            // TODO: impl
            Event::InterfaceChange { .. } => {}
            Event::BlockUpdate { .. } => {}
            Event::Alarm { .. } => {}
        }
        Ok(())
    }

    async fn send_event_response(&mut self, resp: EventResponse) -> Result<()> {
        trace!("event response {resp:?}");
        self.send(serde_json::to_string(&EventResponseWrapper {
            finish: resp,
        })?)
        .await?;
        Ok(())
    }

    async fn send_api_request<T: DeserializeOwned>(&mut self, req: ApiRequest) -> Result<T> {
        trace!("api request {req:?}");
        self.send(serde_json::to_string(&req)?).await?;
        loop {
            match self.try_next_event_or_api_result().await? {
                Some(EventOrApiResult::ApiResult(res)) => {
                    let value = res.result.into_result()?;
                    return Ok(serde_json::from_value(value)?);
                }
                Some(EventOrApiResult::Event(evt)) => Box::pin(self.handle_event(evt)).await?,
                None => return Err(Error::UnexpectedDisconnect),
            }
        }
    }

    async fn send_api_request_discard_result(&mut self, req: ApiRequest) -> Result<()> {
        self.send_api_request::<Value>(req).await?;
        Ok(())
    }

    async fn send(&mut self, message: String) -> Result<()> {
        let mut lock = self.ws.lock().await;
        lock.send(Message::Text(message.into())).await?;
        Ok(())
    }

    pub async fn subscribe_run(&mut self) -> Result<()> {
        self.send_api_request_discard_result(ApiRequest::Subscribe(SubscribeParam::ScriptRun {}))
            .await
    }

    pub async fn read_interface(&mut self, name: impl AsRef<str>) -> Result<String> {
        let result: ReadInterfaceResult = self
            .send_api_request(ApiRequest::ReadInterface {
                name: name.as_ref().to_owned(),
            })
            .await?;
        Ok(result.value)
    }

    pub async fn write_interface(
        &mut self,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) -> Result<()> {
        self.send_api_request_discard_result(ApiRequest::WriteInterface {
            name: name.as_ref().to_owned(),
            value: value.as_ref().to_owned(),
        })
        .await
    }

    pub async fn query_gametime(&mut self) -> Result<i64> {
        let result: QueryGametimeResult =
            self.send_api_request(ApiRequest::QueryGametime {}).await?;
        Ok(result.gametime)
    }

    pub async fn execute_command(
        &mut self,
        command: impl AsRef<str>,
    ) -> Result<ExecuteCommandResult> {
        self.send_api_request(ApiRequest::ExecuteCommand {
            command: command.as_ref().to_owned(),
        })
        .await
    }

    pub async fn log(&mut self, message: impl AsRef<str>, level: LogLevel) -> Result<()> {
        self.send_api_request_discard_result(ApiRequest::Log {
            message: message.as_ref().to_owned(),
            level,
        })
        .await
    }

    pub async fn debug(&mut self, message: impl AsRef<str>) -> Result<()> {
        self.log(message, LogLevel::Debug).await
    }

    pub async fn info(&mut self, message: impl AsRef<str>) -> Result<()> {
        self.log(message, LogLevel::Info).await
    }

    pub async fn warn(&mut self, message: impl AsRef<str>) -> Result<()> {
        self.log(message, LogLevel::Warn).await
    }

    pub async fn error(&mut self, message: impl AsRef<str>) -> Result<()> {
        self.log(message, LogLevel::Error).await
    }

    pub async fn fatal(&mut self, message: impl AsRef<str>) -> Result<()> {
        self.log(message, LogLevel::Fatal).await
    }
}

fn url_encode_query(s: &str) -> String {
    const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');
    utf8_percent_encode(s, QUERY).to_string()
}

#[derive(Debug, Clone)]
pub(crate) enum EventOrApiResult {
    Event(Event),
    ApiResult(ApiResultWrapper),
}
