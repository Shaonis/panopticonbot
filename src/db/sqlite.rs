use crate::db::models::MappingChat;
use crate::db::redis::RedisAPI;
use crate::errors;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Executor, Row, Sqlite, SqlitePool};
use crate::scheduler::Scheduler;

async fn create_sqlite_pool(path: &str) -> errors::Result<SqlitePool> {
    if !Sqlite::database_exists(path).await.unwrap_or(false) {
        println!("SQLite database not found, creating a new one: {path}");
        Sqlite::create_database(path).await?;
    }
    let pool = SqlitePool::connect(path).await?;
    pool.execute(
        r#"
           CREATE TABLE IF NOT EXISTS mapping (
               private_chat INTEGER NOT NULL PRIMARY KEY,
               topic_chat INTEGER NOT NULL,
               last_private INTEGER NOT NULL,
               last_topic INTEGER NOT NULL
           );
           "#
    ).await?;
    pool.execute(
        r#"
           CREATE TABLE IF NOT EXISTS banned (
               chat_id INTEGER NOT NULL PRIMARY KEY
           );
           "#
    ).await?;
    Ok(pool)
}

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
    redis_cache: RedisAPI,
}

impl Database {
    pub async fn new(sqlite_path: &str, redis_cache: RedisAPI) -> errors::Result<Self> {
        let pool = create_sqlite_pool(sqlite_path).await?;
        Ok(Self { pool, redis_cache })
    }

    pub async fn save_mapping(&mut self, mapping: MappingChat) -> errors::Result<()> {
        self.redis_cache.save_mapping(mapping).await?;
        sqlx::query(
            r#"
               INSERT INTO mapping (private_chat, topic_chat, last_private, last_topic)
               VALUES (?, ?, ?, ?)
               ON CONFLICT (private_chat) DO UPDATE SET
                   last_private = excluded.last_private,
                   last_topic = excluded.last_topic;
               "#
        )
            .bind(mapping.sender_chat)
            .bind(mapping.recipient_chat)
            .bind(mapping.last_private)
            .bind(mapping.last_topic)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn sync_mapping(&mut self, mapping: MappingChat, scheduler: Scheduler) -> errors::Result<()> {
        self.redis_cache.save_mapping(mapping).await?;
        // Schedule the task to run in the background,
        // there will be no database query spam!
        let pool = self.pool.clone();
        let task_id = mapping.unique_id() as u64;
        scheduler.add_task(task_id, move || async move {
            let _ = sqlx::query(
                r#"
                   UPDATE mapping
                   SET last_private = ?, last_topic = ?
                   WHERE private_chat = ? OR topic_chat = ?;
                   "#
            )
                .bind(mapping.last_private)
                .bind(mapping.last_topic)
                .bind(mapping.sender_chat)
                .bind(mapping.sender_chat)
                .execute(&pool)
                .await;
            tracing::info!("Successfully synchronized mapping: {task_id}");
        });

        Ok(())
    }

    pub async fn get_mapping(&mut self, chat_id: i64) -> errors::Result<Option<MappingChat>> {
        if let Ok(Some(mapping)) = self.redis_cache.get_mapping(chat_id).await {
            return Ok(Some(mapping));
        }
        let mapping = sqlx::query(
            r#"
               SELECT private_chat, topic_chat, last_private, last_topic
               FROM mapping
               WHERE private_chat = ? OR topic_chat = ?;
               "#
        )
            .bind(chat_id)
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| MappingChat {
                    sender_chat: row.get(0),
                    recipient_chat: row.get(1),
                    last_private: row.get(2),
                    last_topic: row.get(3),
                })
            })?;

        if let Some(mapping) = mapping {
            self.redis_cache.save_mapping(mapping).await?;
        }
        Ok(mapping)
    }

    pub async fn drop_mapping(&mut self, topic_chat: i64) -> errors::Result<()> {
        sqlx::query(
            r#"
               DELETE FROM mapping
               WHERE topic_chat = ?;
               "#
        )
            .bind(topic_chat)
            .execute(&self.pool)
            .await?;
        self.redis_cache.delete_mapping(topic_chat).await?;
        
        Ok(())
    }

    pub async fn ban_user(&mut self, private_chat: i64) -> errors::Result<()> {
        sqlx::query(
            r#"
               INSERT INTO banned (chat_id)
               VALUES (?);
               "#
        )
            .bind(private_chat)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            r#"
               DELETE FROM mapping
               WHERE private_chat = ?;
               "#
        )
            .bind(private_chat)
            .execute(&self.pool)
            .await?;

        self.redis_cache.ban_user(private_chat).await?;
        self.redis_cache.delete_mapping(private_chat).await?;

        Ok(())
    }

    pub async fn check_ban(&mut self, private_chat: i64) -> errors::Result<bool> {
        if let Some(banned) = self.redis_cache.check_ban(private_chat).await.ok().flatten() {
            if banned {
                self.redis_cache.ban_user(private_chat).await?;
            }
            return Ok(banned);
        }
        // The request will only be sent if the mapping is not in the cache,
        // there will be no database query spam!
        let banned = sqlx::query(
            r#"
               SELECT chat_id
               FROM banned
               WHERE chat_id = ?;
               "#
        )
            .bind(private_chat)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.is_some())?;

        if banned {
            self.redis_cache.ban_user(private_chat).await?;
        }
        Ok(banned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::redis::tests::get_test_redis;
    
    async fn setup_sqlite() -> Database {
        let redis_cache = get_test_redis().await;
        let pool = create_sqlite_pool(":memory:")
            .await
            .expect("Failed to create SQLite pool");
        Database { pool, redis_cache }
    }

    #[tokio::test]
    async fn test_save_mapping() {
        let mut db = setup_sqlite().await;

        let mapping = MappingChat {
            sender_chat: 1,
            recipient_chat: 2,
            last_private: 3,
            last_topic: 4,
        };
        db.save_mapping(mapping).await.expect("Failed to save mapping");
        let first_mapping = db.get_mapping(1)
            .await
            .expect("Failed to get mapping")
            .expect("Mapping not found");
        let second_mapping = db.get_mapping(2)
            .await
            .expect("Failed to get mapping")
            .expect("Mapping not found");
        assert_eq!(first_mapping.sender_chat, second_mapping.recipient_chat);
        assert_eq!(first_mapping.recipient_chat, second_mapping.sender_chat);
        assert_eq!(first_mapping.last_private, second_mapping.last_private);
        assert_eq!(first_mapping.last_topic, second_mapping.last_topic);
    }

    #[tokio::test]
    async fn test_drop_mapping() {
        let mut db = setup_sqlite().await;

        let mapping = MappingChat {
            sender_chat: 5,
            recipient_chat: 6,
            last_private: 7,
            last_topic: 8,
        };
        let fetched_mapping = db.get_mapping(6).await;
        assert!(fetched_mapping.is_ok_and(|m| m.is_none()));
        db.save_mapping(mapping).await.expect("Failed to save mapping");
        db.drop_mapping(6).await.expect("Failed to delete mapping");
        let fetched_mapping = db.get_mapping(6).await;
        assert!(fetched_mapping.is_ok_and(|m| m.is_none()));
    }
    
    #[tokio::test]
    async fn test_ban_user() {
        let mut db = setup_sqlite().await;
        
        let mapping = MappingChat {
            sender_chat: 9,
            recipient_chat: 10,
            last_private: 11,
            last_topic: 12,
        };
        let _ = db.save_mapping(mapping).await;
        let banned = db.check_ban(9).await.expect("Failed to check ban");
        assert!(!banned);
        db.ban_user(9).await.expect("Failed to ban user");
        let banned = db.check_ban(9).await.expect("Failed to check ban");
        assert!(banned);
    }
}
