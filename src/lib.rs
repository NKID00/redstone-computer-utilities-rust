mod datatype;

use std::{
    net::TcpStream,
    sync::{Arc, Mutex},
};

use log::*;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde_json::json;
use tungstenite::{Message, WebSocket, connect, stream::MaybeTlsStream};

pub use crate::datatype::ResponseOrErrorCode;
use crate::datatype::{ApiRequest, ApiResult, Event, EventParam, EventResponse};

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
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Script {
    name: String,
    description: String,
    server: String,
    on_init: Option<Box<dyn FnOnce(&mut Context) -> ResponseOrErrorCode>>,
    on_execute: Option<Arc<dyn Fn(&mut Context) -> ResponseOrErrorCode>>,
}

impl Script {
    pub fn new() -> Self {
        Self {
            name: "".to_owned(),
            description: "".to_owned(),
            server: "ws://localhost:32765/".to_owned(),
            on_init: None,
            on_execute: None,
        }
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

    pub fn on_init<F: FnOnce(&mut Context) -> ResponseOrErrorCode + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_init = Some(Box::new(callback));
        self
    }

    pub fn on_execute<F: Fn(&mut Context) -> ResponseOrErrorCode + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_execute = Some(Arc::new(callback));
        self
    }

    pub fn run(self) -> Result<()> {
        info!("Connecting to server");
        let url = format!(
            "{}?name={}&description={}",
            self.server,
            url_encode_query(&self.name),
            url_encode_query(&self.description)
        );
        let (ws, _) = connect(url)?;
        Context {
            script: self,
            ws: Arc::new(Mutex::new(ws)),
        }
        .main_loop()
    }
}

pub struct Context {
    script: Script,
    ws: Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
}

impl Context {
    fn main_loop(&mut self) -> Result<()> {
        loop {
            let Some(evt) = self.try_next_event()? else {
                return Ok(());
            };
            self.handle_event(evt)?;
        }
    }

    fn try_next_event(&mut self) -> Result<Option<Event>> {
        match self.try_next_event_or_api_result()? {
            Some(EventOrApiResult::Event(evt)) => Ok(Some(evt)),
            Some(EventOrApiResult::ApiResult(res)) => {
                error!("unexpected api response {res:?}");
                Err(Error::UnexpectedApiResult)
            }
            None => Ok(None),
        }
    }

    fn try_next_event_or_api_result(&mut self) -> Result<Option<EventOrApiResult>> {
        let Some(s) = self.try_next_message()? else {
            return Ok(None);
        };
        if let Ok(evt) = serde_json::from_str::<Event>(&s) {
            trace!("event request {evt:?}");
            return Ok(Some(EventOrApiResult::Event(evt)));
        }
        if let Ok(res) = serde_json::from_str::<ApiResult>(&s) {
            trace!("api response {res:?}");
            return Ok(Some(EventOrApiResult::ApiResult(res)));
        }
        error!("unrecognized server message {s:?}");
        return Err(Error::InvalidServerMessage);
    }

    fn try_next_message(&mut self) -> Result<Option<String>> {
        loop {
            match {
                let mut lock = self.ws.lock().unwrap();
                lock.read()?
            } {
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

    fn handle_event(&mut self, evt: Event) -> Result<()> {
        match evt {
            Event::ScriptInitialize {} => {
                if self.script.on_execute.is_some() {
                    self.subscribe_run();
                }
                if let Some(callback) = self.script.on_init.take() {
                    let resp = callback(self);
                    self.send_event_response(EventResponse { finish: resp })?;
                } else {
                    self.send_event_response(EventResponse::ok(json!({})))?;
                }
            }
            Event::ScriptRun {} => {
                if let Some(callback) = &self.script.on_execute {
                    let callback = callback.clone();
                    let resp = callback(self);
                    self.send_event_response(EventResponse { finish: resp })?;
                } else {
                    self.send_event_response(EventResponse::ok(json!({})))?;
                }
            }
        }
        Ok(())
    }

    fn send_event_response(&mut self, resp: EventResponse) -> Result<()> {
        let mut lock = self.ws.lock().unwrap();
        lock.send(Message::Text(serde_json::to_string(&resp).unwrap().into()))?;
        Ok(())
    }

    fn send_api_request(&mut self, req: ApiRequest) -> Result<ApiResult> {
        {
            let mut lock = self.ws.lock().unwrap();
            lock.send(Message::Text(serde_json::to_string(&req).unwrap().into()))?;
        }
        loop {
            match self.try_next_event_or_api_result()? {
                Some(EventOrApiResult::ApiResult(res)) => return Ok(res),
                Some(EventOrApiResult::Event(evt)) => self.handle_event(evt)?,
                None => return Err(Error::UnexpectedDisconnect),
            }
        }
    }

    pub fn subscribe_run(&mut self) {
        self.send_api_request(ApiRequest::Subscribe(EventParam::ScriptRun {}))
            .unwrap();
    }

    pub fn info(&mut self, message: impl AsRef<str>) {
        self.send_api_request(ApiRequest::Log {
            message: message.as_ref().to_owned(),
            level: datatype::LogLevel::Info,
        })
        .unwrap();
    }
}

fn url_encode_query(s: &str) -> String {
    const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');
    utf8_percent_encode(s, QUERY).to_string()
}

#[derive(Debug, Clone)]
pub(crate) enum EventOrApiResult {
    Event(Event),
    ApiResult(ApiResult),
}
