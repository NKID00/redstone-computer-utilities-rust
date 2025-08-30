use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "api", rename_all = "camelCase")]
pub(crate) enum ApiRequest {
    Subscribe(EventParam),
    Log { message: String, level: LogLevel },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "name", content = "param", rename_all = "camelCase")]
pub enum EventParam {
    ScriptRun {},
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
pub(crate) struct ApiResult {
    result: ResponseOrErrorCode,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event", rename_all = "camelCase")]
pub(crate) enum Event {
    ScriptInitialize {},
    ScriptRun {},
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct EventResponse {
    pub(crate) finish: ResponseOrErrorCode,
}

impl EventResponse {
    pub(crate) fn ok(resp: serde_json::Value) -> Self {
        Self {
            finish: ResponseOrErrorCode::Ok(resp),
        }
    }

    pub(crate) fn err(code: ErrorCode) -> Self {
        Self {
            finish: ResponseOrErrorCode::Err(code),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ResponseOrErrorCode {
    Ok(serde_json::Value),
    Err(ErrorCode),
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug, Clone, Copy)]
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
