use serde::Serialize;
use ts_rs::TS;

/// What went wrong, for code to branch on. The user sees `ApiError::message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ErrorKind {
    NotFound,
    InvalidInput,
    Cancelled,
    UnknownMethod,
    Internal,
}

/// Error returned by every API method.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ApiError {
    pub kind: ErrorKind,
    /// A readable English sentence: what went wrong and what to do.
    pub message: String,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }

    pub fn unknown_method(name: &str) -> Self {
        Self::new(
            ErrorKind::UnknownMethod,
            format!(
                "The app asked for \"{name}\", which this version does not have. Update the app and try again."
            ),
        )
    }

    /// Generic message only: details go to the log, never to the user.
    pub fn internal() -> Self {
        Self::new(
            ErrorKind::Internal,
            "Something went wrong inside the app. Restart it and try again.",
        )
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ApiError {}
