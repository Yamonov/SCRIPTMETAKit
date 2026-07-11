use std::{error::Error, fmt, path::PathBuf};

pub type ScriptMetaKitResult<T> = Result<T, ScriptMetaKitError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScriptMetaKitError {
    InvalidConfig(String),
    Parse(String),
    Url(String),
    Io { path: PathBuf, message: String },
    Timeout(String),
    Cache(String),
    Conflict(String),
    NotImplemented(String),
}

impl fmt::Display for ScriptMetaKitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid configuration: {message}"),
            Self::Parse(message) => write!(formatter, "parse error: {message}"),
            Self::Url(message) => write!(formatter, "url error: {message}"),
            Self::Io { path, message } => write!(formatter, "{}: {message}", path.display()),
            Self::Timeout(message) => write!(formatter, "timeout: {message}"),
            Self::Cache(message) => write!(formatter, "cache error: {message}"),
            Self::Conflict(message) => write!(formatter, "conflict: {message}"),
            Self::NotImplemented(message) => write!(formatter, "not implemented: {message}"),
        }
    }
}

impl ScriptMetaKitError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig(_) => "invalid_config",
            Self::Parse(_) => "parse_error",
            Self::Url(_) => "url_error",
            Self::Io { .. } => "io_error",
            Self::Timeout(_) => "timeout",
            Self::Cache(_) => "cache_error",
            Self::Conflict(_) => "conflict",
            Self::NotImplemented(_) => "not_implemented",
        }
    }
}

impl Error for ScriptMetaKitError {}

impl From<url::ParseError> for ScriptMetaKitError {
    fn from(error: url::ParseError) -> Self {
        Self::Url(error.to_string())
    }
}
