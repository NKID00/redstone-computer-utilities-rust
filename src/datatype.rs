use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use strum::Display;

use crate::Error;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "api", content = "param", rename_all = "camelCase")]
pub(crate) enum ApiRequest {
    Subscribe(SubscribeParam),
    ReadInterface { name: String },
    WriteInterface { name: String, value: String },
    QueryGametime {},
    ExecuteCommand { command: String },
    Log { message: String, level: LogLevel },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "name", content = "param", rename_all = "camelCase")]
pub enum SubscribeParam {
    ScriptRun {},
    InterfaceChange(InterfaceChangeParam),
    BlockUpdate(BlockUpdateParam),
    Alarm(AlarmParam),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InterfaceChangeParam {
    name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockUpdateParam {
    pos: BlockPos,
    #[serde(rename = "type")]
    type_: BlockUpdateType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AlarmParam {
    gametime: i64,
    at: AlarmAt,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockPos(i32, i32, i32, String);

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum BlockUpdateType {
    NeighborUpdate,
    PostPlacement,
    Any,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum AlarmAt {
    Start,
    End,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ApiResultWrapper {
    pub(crate) result: ApiResult,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub(crate) enum ApiResult {
    Ok(serde_json::Value),
    Err(ErrorCode),
}

impl From<ApiResult> for Result<serde_json::Value, ErrorCode> {
    fn from(value: ApiResult) -> Self {
        match value {
            ApiResult::Ok(value) => Ok(value),
            ApiResult::Err(code) => Err(code),
        }
    }
}

impl ApiResult {
    pub(crate) fn into_result(self) -> Result<serde_json::Value, ErrorCode> {
        self.into()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReadInterfaceResult {
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueryGametimeResult {
    pub gametime: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecuteCommandResult {
    pub feedback: String,
    pub error: String,
    pub result: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event", rename_all = "camelCase")]
pub(crate) enum Event {
    ScriptInitialize {},
    ScriptRun {
        content: ScriptRunContent,
    },
    InterfaceChange {
        param: InterfaceChangeParam,
        content: InterfaceChangeContent,
    },
    BlockUpdate {
        param: BlockUpdateParam,
    },
    Alarm {
        param: AlarmParam,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScriptRunContent {
    pub(crate) argument: Vec<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InterfaceChangeContent {
    pub(crate) previous: String,
    pub(crate) current: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct EventResponseWrapper {
    pub(crate) finish: EventResponse,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub(crate) enum EventResponse {
    Ok(serde_json::Value),
    ScriptRun { result: i32 },
    Err(ErrorCode),
}

impl EventResponse {
    pub(crate) fn empty() -> Self {
        Self::Ok(json!({}))
    }
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug, Clone, Copy, Display)]
#[repr(i32)]
pub enum ErrorCode {
    GeneralError = -1,
    ArgumentInvalid = -2,
    NameIllegal = -3,
    NameExists = -4,
    NameNotFound = -5,
    InternalError = -6,
    ChunkUnloaded = -7,
}

impl From<Error> for ErrorCode {
    fn from(_value: Error) -> Self {
        Self::InternalError
    }
}

impl std::error::Error for ErrorCode {}
