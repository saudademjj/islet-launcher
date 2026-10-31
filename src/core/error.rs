use serde::ser::{Serialize, SerializeStruct, Serializer};

pub type AppResult<T> = Result<T, AppError>;

/// Application error type. It is serialized to the frontend as
/// `{ code, message, detail }` so the UI can show a readable Chinese message
/// while keeping technical details for debugging/logs.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("network request failed")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{message}")]
    App {
        code: &'static str,
        message: String,
        detail: Option<String>,
    },
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::App {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn detail(
        code: &'static str,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::App {
            code,
            message: message.into(),
            detail: Some(detail.into()),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Http(_) => "network",
            Self::Json(_) => "invalid_data",
            Self::App { code, .. } => code,
        }
    }

    pub fn user_message(&self) -> String {
        match self {
            Self::Io(_) => crate::core::i18n::pick("文件操作失败，请检查磁盘空间和权限。", "A file operation failed; check disk space and permissions.").into(),
            Self::Http(_) => crate::core::i18n::pick("网络请求失败，请检查网络连接后重试。", "A network request failed; check your connection and try again.").into(),
            Self::Json(_) => crate::core::i18n::pick("数据格式错误，可能是缓存损坏。", "Malformed data; the cache may be damaged.").into(),
            Self::App { message, .. } => message.clone(),
        }
    }

    pub fn detail_text(&self) -> Option<String> {
        match self {
            Self::Io(e) => Some(e.to_string()),
            // Do not expose raw HTTP errors: request URLs can contain OAuth codes.
            Self::Http(_) => None,
            Self::Json(e) => Some(e.to_string()),
            Self::App { detail, .. } => detail.clone(),
        }
    }

    pub fn to_payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code().to_string(),
            message: self.user_message(),
            detail: self.detail_text(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let payload = self.to_payload();
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("code", &payload.code)?;
        state.serialize_field("message", &payload.message)?;
        state.serialize_field("detail", &payload.detail)?;
        state.end()
    }
}

impl From<AppError> for ErrorPayload {
    fn from(value: AppError) -> Self {
        value.to_payload()
    }
}
