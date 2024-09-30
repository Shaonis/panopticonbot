use teloxide::adaptors::DefaultParseMode;
use teloxide::types::{BotCommandScope, ParseMode, Recipient};
use teloxide::update_listeners::UpdateListener;
use teloxide::{
    prelude::*,
    update_listeners::webhooks,
};
use std::convert::Infallible;
use std::net::SocketAddr;
use secrecy::ExposeSecret;
use handlers::{handler_schema, PublicCommand, AdminCommand};
use db::{Database, RedisAPI};
use url::Url;
pub use config::Settings;
pub use scheduler::Scheduler;
use teloxide::utils::command::BotCommands;

mod errors;
mod config;
mod handlers;
mod scheduler;
mod db;

type Bot = DefaultParseMode<teloxide::Bot>;

pub async fn run_bot(settings: Settings, scheduler: Scheduler) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting the bot...");
    // Configure Database
    let redis_cache = RedisAPI::new(&settings.redis_url, 1800).await?;
    let db = Database::new(&settings.sqlite_path, redis_cache).await?;
    // Configure bot
    let bot = teloxide::Bot::new(settings.bot_token.expose_secret())
        .parse_mode(ParseMode::Html);
    let _ = set_bot_commands(&bot, settings.forum_id).await;
    
    // Handler tree
    let dependencies = dptree::deps![db, settings.forum_id, scheduler];
    let mut dp = Dispatcher::builder(bot.clone(), handler_schema())
        .dependencies(dependencies)
        .build();
    
    // Webhook or long-polling
    if let Some(webhook_url) = settings.webhook_url {
        let webhook_listener = settings.webhook_listener.expect("settings validated");

        tracing::info!("Using webhook: {webhook_url}");
        tracing::info!("Listening on: {webhook_listener}");
        dp.dispatch_with_listener(
            setup_listener(bot, webhook_url, webhook_listener).await,
            LoggingErrorHandler::with_custom_text("An error from the update listener, try again or use polling"),
        ).await;
    } else {
        tracing::info!("Using long-polling");
        dp.dispatch().await;
    }
    
    Ok(())
}

async fn setup_listener(bot: Bot, webhook_url: Url, webhook_listener: SocketAddr) -> impl UpdateListener<Err = Infallible> {
    let options = webhooks::Options::new(webhook_listener, webhook_url);
    webhooks::axum(bot, options)
        .await
        .expect("Couldn't setup webhook")
}

async fn set_bot_commands(bot: &Bot, forum_id: ChatId) -> Result<(), Box<dyn std::error::Error>> {
    bot.set_my_commands(PublicCommand::bot_commands())
        .scope(BotCommandScope::AllPrivateChats)
        .await?;
    bot.set_my_commands(AdminCommand::bot_commands())
        .scope(BotCommandScope::Chat { chat_id: Recipient::Id(forum_id) })
        .await?;

    Ok(())
}
