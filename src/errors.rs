use derive_more::{Display, Error};
use tracing::error;

#[derive(Debug, Display, Error)]
pub enum ServerError {
    RunTime(Box<dyn std::error::Error + Send + Sync>),
    Config(Box<dyn std::error::Error + Send + Sync>),
}

impl From<Box<pingora::Error>> for ServerError {
    fn from(value: Box<pingora::Error>) -> Self {
        error!("Pingora error: {}", value);
        ServerError::RunTime(Box::new(value))
    }
}

impl From<tracing::subscriber::SetGlobalDefaultError> for ServerError {
    fn from(value: tracing::subscriber::SetGlobalDefaultError) -> Self {
        error!("Tracing error: {}", value);
        ServerError::RunTime(Box::new(value))
    }
}

impl From<openssl::error::ErrorStack> for ServerError {
    fn from(value: openssl::error::ErrorStack) -> Self {
        error!("OpenSSL error: {}", value);
        ServerError::RunTime(Box::new(value))
    }
}

impl From<std::io::Error> for ServerError {
    fn from(value: std::io::Error) -> Self {
        error!("IO error: {}", value);
        ServerError::RunTime(Box::new(value))
    }
}

impl From<toml::de::Error> for ServerError {
    fn from(value: toml::de::Error) -> Self {
        error!("TOML deserialization error: {}", value);
        ServerError::Config(Box::new(value))
    }
}
