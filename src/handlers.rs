use crate::db::{Database, MappingChat};
use crate::Bot;
use rand::{prelude::SliceRandom, thread_rng};
use teloxide::{
    dispatching::UpdateHandler,
    macros::BotCommands,
    types::{
        ChatId, 
        InlineKeyboardButton, 
        InlineKeyboardMarkup, 
        LinkPreviewOptions, 
        MessageId, 
        ReplyParameters, 
        ThreadId,
    },
    prelude::*,
};
use tracing::instrument;
use crate::scheduler::Scheduler;
use std::env;
use teloxide::types::{MessageKind, User};
use std::sync::LazyLock;

type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const TOPIC_ICON_COLOR: [u32; 6] = [  // https://core.telegram.org/bots/api#createforumtopic
    7322096, 16766590, 13338331, 9367192, 16749490, 16478047,
];
const LINK_PREVIEW_OPTIONS: LinkPreviewOptions = LinkPreviewOptions {
    is_disabled: true,
    url: None,
    prefer_small_media: false,
    prefer_large_media: false,
    show_above_text: false,
};
static START_COMMAND: LazyLock<String> = LazyLock::new(|| {
    env::var("START_COMMAND").expect("env var START_COMMAND must be set")
});
static HELP_COMMAND: LazyLock<String> = LazyLock::new(|| {
    env::var("HELP_COMMAND").expect("env var HELP_COMMAND must be set")
});

#[derive(BotCommands, Clone)]
#[command(rename_rule = "snake_case")]
pub enum PublicCommand {
    /// Start
    #[command(description = "start")]
    Start,
    /// Help
    #[command(description = "help")]
    Help,
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "snake_case")]
pub enum AdminCommand {
    /// Drop topic
    #[command(description = "Drop the current topic")]
    DropTopic(String),
}

pub fn handler_schema() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync + 'static>> {
    dptree::entry()
        .branch(Update::filter_message()
            .branch(dptree::entry()
                .filter_command::<PublicCommand>()
                .endpoint(public_command_handler)
            )
            .branch(
                dptree::filter(|msg: Message| msg.chat.is_private())
                    .filter_map(|msg: Message| msg.from)
                    .endpoint(private_handler)
            )
            .branch(dptree::entry()
                .filter_command::<AdminCommand>()
                .filter(|msg: Message, forum_id: ChatId| msg.chat.id == forum_id)
                .filter_map(|msg: Message| msg.thread_id)
                .endpoint(admin_command_handler)
            )
            .branch(dptree::filter(|msg: Message, forum_id: ChatId| {
                msg.chat.id == forum_id && matches!(msg.kind,
                    MessageKind::Common(_) | MessageKind::Dice(_)
                )
            })
                .filter_map(|msg: Message| msg.thread_id)
                .endpoint(topic_handler))
        )
        .branch(Update::filter_callback_query()
            .branch(dptree::filter(|call: CallbackQuery|
                call.data.map_or(false, |data| data == "ban")
            )
                .filter_map(|call: CallbackQuery|
                    call.message.and_then(|maybe_msg| maybe_msg.regular_message().cloned())
                )
                .filter_map(|msg: Message| msg.thread_id)
                .endpoint(ban_handler))
        )
}

#[instrument(
    name = "Public command handler",
    skip(bot, msg, cmd),
)]
async fn public_command_handler(bot: Bot, msg: Message, cmd: PublicCommand) -> HandlerResult {
    let response = match cmd {
        PublicCommand::Start => START_COMMAND.as_str(),
        PublicCommand::Help => HELP_COMMAND.as_str(),
    };
    bot.send_message(msg.chat.id, response).await?;
    Ok(())
}

#[instrument(
    name = "Private chat handler",
    skip(bot, msg, user, db, forum_id, scheduler),
)]
async fn private_handler(
    bot: Bot,
    msg: Message,
    user: User,
    mut db: Database,
    forum_id: ChatId,
    scheduler: Scheduler,
) -> HandlerResult {
    if db.check_ban(msg.chat.id.0).await? {
        return Ok(());
    }
    if let Some(mut mapping) = db.get_mapping(msg.chat.id.0).await.ok().flatten() {
        let thread_id = ThreadId(MessageId(mapping.recipient_chat.0 as i32));
        let last_topic = if let Some(reply_msg) = msg.reply_to_message() {
            let shift = msg.id.0 - reply_msg.id.0 - 1;
            let reply_msg_id = MessageId(mapping.last_topic.0 - shift);
            bot.copy_message(forum_id, msg.chat.id, msg.id)
                .message_thread_id(thread_id)
                .reply_parameters(ReplyParameters::new(reply_msg_id))
                .await?
        } else {
            bot.copy_message(forum_id, msg.chat.id, msg.id)
                .message_thread_id(thread_id)
                .await?
        };
        mapping.sync(msg.id, last_topic);
        db.sync_mapping(mapping, scheduler).await?;
    } else {
        create_new_topic(bot, msg, user, db, forum_id).await?;
    }
    
    Ok(())
}

#[instrument(
    name = "Topic handler",
    skip(bot, msg, thread_id, db, scheduler),
)]
async fn topic_handler(
    bot: Bot,
    msg: Message,
    thread_id: ThreadId,
    mut db: Database,
    scheduler: Scheduler,
) -> HandlerResult {
    let thread_id = thread_id.0.0;
    let mut mapping = db.get_mapping(thread_id as i64).await?.ok_or_else(|| {
        tracing::warn!("Mapping not configured: {thread_id}");
        "Mapping not configured"
    })?;
    let with_reply = msg.reply_to_message()
        .map_or(false, |reply| reply.id.0 != thread_id);
    
    let last_private = if with_reply {
        let reply_to_message_id = msg.reply_to_message().expect("with reply").id.0;
        let shift = msg.id.0 - reply_to_message_id - 1;
        let reply_msg_id = MessageId(mapping.last_private.0 - shift);
        bot.copy_message(mapping.recipient_chat, msg.chat.id, msg.id)
            .reply_parameters(ReplyParameters::new(reply_msg_id))
            .await?
    } else {
        bot.copy_message(mapping.recipient_chat, msg.chat.id, msg.id).await?
    };
    mapping.sync(last_private, msg.id);
    db.sync_mapping(mapping, scheduler).await?;

    Ok(())
}

#[instrument(
    name = "Admin command handler",
    skip(bot, msg, thread_id, cmd, forum_id, db, scheduler),
)]
async fn admin_command_handler(
    bot: Bot,
    msg: Message,
    thread_id: ThreadId,
    cmd: AdminCommand,  // while 1 command !!!
    forum_id: ChatId, 
    mut db: Database,
    scheduler: Scheduler,
) -> HandlerResult {
    if let AdminCommand::DropTopic(forum_name) = cmd {
        if forum_name.is_empty() {
            bot.send_message(
                msg.chat.id, 
                "⚠️ Please, specify a new topic name,\nf.e. /drop_topic {topic_name}"
            )
                .message_thread_id(thread_id).await?;
            return Ok(());
        }
        let thread_id_num = thread_id.0.0 as i64;
        if let Some(mapping) = db.get_mapping(thread_id_num).await? {
            // Delete mapping
            let _ = db.drop_mapping(thread_id_num).await;
            scheduler.cancel_task(mapping.unique_id() as u64); // Cancel scheduled synchronization
            // Drop topic
            let forum_name = format!("🗄 {forum_name}");
            close_topic(&bot, forum_id, thread_id, &forum_name).await?;
            bot.send_message(msg.chat.id, "🗑 Topic dropped")
                .message_thread_id(thread_id).await?;
            tracing::info!("Topic dropped: {}", thread_id.0.0);
        }
    }
    
    Ok(())
}

#[instrument(
    name = "Ban handler",
    skip(bot, call, msg, thread_id, db, forum_id, scheduler),
)]
async fn ban_handler(
    bot: Bot, 
    call: CallbackQuery,
    msg: Message,
    thread_id: ThreadId,
    mut db: Database, 
    forum_id: ChatId,
    scheduler: Scheduler,
) -> HandlerResult {
    if let Some(mapping) = db.get_mapping(thread_id.0.0 as i64).await? {
        // Ban user
        db.ban_user(mapping.recipient_chat.0).await?;
        scheduler.cancel_task(mapping.unique_id() as u64); // Cancel scheduled synchronization
        // Drop topic
        let topic_name = format!("🚫 {}", mapping.recipient_chat);
        close_topic(&bot, forum_id, thread_id, &topic_name).await?;
        bot.send_message(msg.chat.id, "🚫 The user was blocked")
            .message_thread_id(thread_id)
            .await?;
    
        bot.answer_callback_query(call.id)
            .text("♨️ Successfully banned!")
            .show_alert(true)
            .await?;
        tracing::info!("User banned: {}", mapping.recipient_chat.0);
    }
    bot.edit_message_reply_markup(forum_id, msg.id)
        .reply_markup(InlineKeyboardMarkup::default())
        .await?;
    
    Ok(())
}

async fn create_new_topic(
    bot: Bot,
    msg: Message,
    user: User,
    mut db: Database,
    forum_id: ChatId,
) -> HandlerResult {
    let topic_icon = *TOPIC_ICON_COLOR.choose(&mut thread_rng()).expect("infallible");
    let topic = bot.create_forum_topic(
        forum_id,
        &user.first_name,
        topic_icon,
        "",
    ).await?;

    let user_info = format!(
        "<a href=\"{}\"><b>{}</b></a> \
        \n🆔 <code>{}</code> \
        \n🎗 Username - {} \
        \n\n🌐 Language code: {}",
        user.preferably_tme_url(),
        user.full_name(),
        user.id,
        user.username.as_deref().unwrap_or("None"),
        user.language_code.as_deref().unwrap_or("None"),
    );
    let ban_button = InlineKeyboardMarkup::new(
        vec![vec![InlineKeyboardButton::callback("🚫 Ban", "ban")]]
    );
    let init_msg = bot.send_message(forum_id, user_info)
        .message_thread_id(topic.thread_id)
        .reply_markup(ban_button)
        .link_preview_options(LINK_PREVIEW_OPTIONS)
        .await?;
    bot.pin_chat_message(forum_id, init_msg.id).await?;

    let last_topic = bot.copy_message(forum_id, msg.chat.id, msg.id)
        .message_thread_id(topic.thread_id)
        .await?;
    
    let topic_chat = ChatId(topic.thread_id.0.0 as i64);
    let mapping = MappingChat::new(
        msg.chat.id,
        topic_chat,
        msg.id,
        last_topic,
    );
    db.save_mapping(mapping).await?;
    tracing::info!("New topic created: {}", topic_chat.0);

    Ok(())
}

async fn close_topic(
    bot: &Bot,
    forum_id: ChatId,
    thread_id: ThreadId,
    forum_name: &str,
) -> HandlerResult {
    bot.close_forum_topic(forum_id, thread_id).await?;
    bot.edit_forum_topic(forum_id, thread_id)
        .name(forum_name)
        .await?;

    Ok(())
}
