use std::net::SocketAddr;
use config::{Config, Environment};
use serde::Deserialize;
use url::Url;
use teloxide::types::ChatId;
use secrecy::SecretBox;
use crate::errors::ConfigError;

#[derive(Deserialize)]
pub struct Settings {
    pub bot_token: SecretBox<String>,
    pub forum_id: ChatId,
    pub sqlite_path: String,
    pub redis_url: Url,
    pub webhook_url: Option<Url>,
    pub webhook_listener: Option<SocketAddr>,
}

impl Settings {
    pub fn from_env(env_path: &str) -> Result<Self, ConfigError> {
        dotenvy::from_filename(env_path)?;
        let config = Config::builder()
            .add_source(Environment::default())
            .build()?;

        let settings: Settings = config.try_deserialize()?;
        if settings.webhook_url.is_some() && settings.webhook_listener.is_none() {
            return Err(ConfigError::Invalid(
                "WEBHOOK_URL is set, but the address that the bot will listen to (WEBHOOK_LISTENER) is not"
            ));
        }
        Ok(settings)
    }
}
