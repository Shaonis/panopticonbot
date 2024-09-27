# Panopticon Feedback Bot

**Panopticon Feedback Bot** - Telegram bot that creates a thread in a given forum for each user that writes in it.

That's why it has Panopticon in the name - this concept makes it easier for administrators to respond to individual users. This is especially useful in scenarios where it is necessary to have clear dialogs between users and administrators

## Key Features
- **Admin-user topics**: Each user gets a separate topic for discussion on the forum, which provides structured and clear communication between users and administrators
- **Simple and fast**: The bot is written in Rust, which ensures high performance and reliability
- **Long-polling or Webhook**: You can run the bot in long-polling or webhook mode, which provides flexibility depending on your server settings
- **Banning feature**: Admins can ban users from interacting with the bot, which will be useful if you start getting spammed
- **Topic archiving** - you can archive a topic at any time to save important information
- **Docker Support**: Easily deploy the bot using Docker, which takes care of all dependencies and services

## Why use Panopticon Feedback Bot?
Managing multiple user interactions with a single bot can be an overwhelming task. Panopticon solves this problem by creating separate forum threads for each user, ensuring that no messages are lost and every conversation is easy to track. 

This functionality is ideal for:
- **Community Admins**: When giving feedback, admins and just popular personalities are better off remaining anonymous to avoid becoming spam victims
- **Customer Support**: Can be used to structure user feedback, complaints and suggestions into convenient discussion threads
- **Personal Use**: Having your own hotline that every Telegram user can write to can be extremely useful. Especially in cases when users who have been restricted by Telegram from writing to new people want to write to you

## Requirements
Before starting, make sure you have the following:
- **Rust 1.81** or higher (locally or via Docker)
- **Redis** instance running (locally or via Docker)
- Telegram **Bot Token** from [BotFather](https://core.telegram.org/bots#botfather)
- **Forum ID** where topics will be created (must have `-100` prefix)

**Don't forget to add a bot to the forum and give permissions to change topics**

## How to Run the Bot

Before running the bot, create a `.env` file in root directory with the necessary environment variables:
```bash
### Required ###
BOT_TOKEN={YOUR TELEGRAM BOT TOKEN}
FORUM_ID={FORUM ID with -100 prefix}
SQLITE_PATH={PATH TO YOUR SQLITE DB FILE}
REDIS_URL={YOUR REDIS URL}
START_COMMAND="{TEXT FOR START COMMAND}"
HELP_COMMAND="{TEXT FOR HELP COMMAND}"

### Optional (for Webhook) ###
WEBHOOK_URL={YOUR WEBHOOK URL}
WEBHOOK_PORT={PORT FOR WEBHOOK}  # is also required if webhook is used

### Example ###
BOT_TOKEN=123456789:AAEQIi5ZhwXuQnwHg0Po6povuMMcC99Vcpc
FORUM_ID=-100123456789
SQLITE_PATH=sqlite/database.db
REDIS_URL=redis://localhost:6379/0
START_COMMAND=Hello, ask a question and we will try to answer it as soon as possible!
HELP_COMMAND=All your messages are sent to us. If you need anything, write to us and we will respond.

WEBHOOK_URL=https://your-webhook-url.com
WEBHOOK_PORT=8443
```

### 1. Running in Long-Polling Mode

In long-polling mode, the bot periodically requests updates from Telegram. This is the easiest setup and requires no external URL configuration.

```bash
git clone https://github.com/your-username/panopticonbot.git
cd panopticonbot
cargo run --release
```

### 2. Running in Webhook Mode

In webhook mode, Telegram pushes updates directly to your server, which makes this method more efficient for high-traffic bots. You will need to specify a valid HTTPS endpoint for Telegram to send these updates to your bot.

First, add the following environment variables to your .env file:
```bash
WEBHOOK_URL="https://your-webhook-url"
WEBHOOK_PORT=8443 # or any other available port
```

And run the bot:
```bash
cargo run --release
```

### 3. Running with Docker

The bot can also be launched using Docker for easy deployment. You can use Docker Compose to run the bot in either long-polling or webhook mode.

- You may need to modify docker-compose.yaml to specify your volumes or other settings
- Don't forget to create an .env file with environment variables in the project root directory as shown above
- If you want to use Webhook, in addition to adding data about it to the .env, also uncomment the ports section in docker-compose.yaml
- Before running Docker Compose, create a Docker image with an appropriate name:

```bash
docker build -t panopticonbot .
```
Now run the bot with a single command:
```bash
docker compose up
```

Docker Compose will automatically build the bot, configure the necessary services and start everything with a single command. You can check the logs to make sure the bot is running properly.
