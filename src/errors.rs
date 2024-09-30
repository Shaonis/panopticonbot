use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to load the configuration: {0}")]
    Load(#[from] config::ConfigError),
    #[error("Failed to load data from .env file: {0}")]
    EnvFile(#[from] dotenvy::Error),
    #[error("Incorrect data: {0}")]
    Invalid(&'static str),
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Redis(#[from] redis::RedisError),
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
}
