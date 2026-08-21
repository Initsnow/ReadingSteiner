use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serde json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("control error: {0}")]
    Control(String),
    #[error("other error: {0}")]
    Other(String),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl Error {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
