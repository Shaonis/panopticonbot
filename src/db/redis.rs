use crate::db::models::MappingChat;
use crate::errors;
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;

#[derive(Clone)]
pub struct RedisAPI {
    conn: MultiplexedConnection,
    key_ttl: i64,
}

impl RedisAPI {
    pub async fn new(redis_url: &str, key_ttl: u64) -> errors::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn: MultiplexedConnection = client.get_multiplexed_async_connection().await?;
        Ok(Self {
            conn,
            key_ttl: key_ttl as i64,
        })
    }

    fn mapping_key(&self, chat_id: i64) -> String {
        format!("mapping:{}", chat_id)
    }

    fn mapping_value(&self, relevant_chat: i64, last_private: i32, last_topic: i32) -> String {
        format!("{}:{}:{}", relevant_chat, last_private, last_topic)
    }
    
    fn banned_key(&self, private_chat: i64) -> String {
        format!("banned:{}", private_chat)
    }
    
    pub async fn save_mapping(&mut self, mapping: MappingChat) -> errors::Result<()> {
        let first_key = self.mapping_key(mapping.sender_chat);
        let first_value = self.mapping_value(
            mapping.recipient_chat, 
            mapping.last_private, 
            mapping.last_topic,
        );
        let second_key = self.mapping_key(mapping.recipient_chat);
        let second_value = self.mapping_value(
            mapping.sender_chat, 
            mapping.last_private, 
            mapping.last_topic,
        );
        redis::pipe()
            .atomic()
            .set(&first_key, first_value)
            .set(&second_key, second_value)
            .expire(first_key, self.key_ttl)
            .expire(second_key, self.key_ttl)
            .query_async(&mut self.conn)
            .await?;

        Ok(())
    }

    pub async fn get_mapping(&mut self, chat_id: i64) -> errors::Result<Option<MappingChat>> {
        let key = self.mapping_key(chat_id);
        let mapping_data: Option<String> = self.conn.get(&key).await?;

        if let Some(mapping_data) = mapping_data {
            let mut parts = mapping_data.split(':');
            let relevant_chat = parts.next().expect("infallible").parse::<i64>()?;
            let last_private = parts.next().expect("infallible").parse::<i32>()?;
            let last_topic = parts.next().expect("infallible").parse::<i32>()?;
            Ok(Some(MappingChat {
                sender_chat: chat_id,
                recipient_chat: relevant_chat,
                last_private,
                last_topic,
            }))
        } else { Ok(None) }
    }

    pub async fn delete_mapping(&mut self, chat_id: i64) -> errors::Result<()> {
        let first_key = self.mapping_key(chat_id);

        let mapping_data: String = self.conn.get(&first_key).await?;
        let mut parts = mapping_data.split(':');
        let mapping_chat = parts.next().expect("infallible").parse::<i64>()?;

        let second_key = self.mapping_key(mapping_chat);

        redis::pipe()
            .atomic()
            .del(first_key)
            .del(second_key)
            .query_async(&mut self.conn)
            .await?;

        Ok(())
    }

    pub async fn ban_user(&mut self, private_chat: i64) -> errors::Result<()> {
        let key = self.banned_key(private_chat);
        self.conn.set(&key, "").await?;
        self.conn.expire(&key, self.key_ttl).await?;
        Ok(())
    }

    pub async fn check_ban(&mut self, private_chat: i64) -> errors::Result<Option<bool>> {
        let banned_key = self.banned_key(private_chat);
        let banned: bool = self.conn.exists(banned_key).await?;
        let mapping_key = self.mapping_key(private_chat);
        let mapping_exists: bool = self.conn.exists(mapping_key).await?;

        if !banned && !mapping_exists {
            return Ok(None);
        }
        Ok(Some(banned))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    
    // Also for SQLite tests
    pub async fn get_test_redis() -> RedisAPI {
        let mut redis_api = RedisAPI::new(
            "redis://127.0.0.1/",
            60
        ).await.expect("Failed to connect to Redis");
        let _: () = redis::cmd("FLUSHALL")
            .query_async(&mut redis_api.conn)
            .await
            .expect("Failed to flush Redis");
        redis_api
    }

    #[tokio::test]
    async fn test_save_mapping() {
        let mut redis_api = get_test_redis().await;

        let mapping = MappingChat {
            sender_chat: 1,
            recipient_chat: 2,
            last_private: 3,
            last_topic: 4,
        };
        redis_api.save_mapping(mapping).await.expect("Failed to save mapping");
        let first_mapping = redis_api.get_mapping(1)
            .await
            .expect("Failed to get mapping")
            .expect("Mapping not found");
        let second_mapping = redis_api.get_mapping(2)
            .await
            .expect("Failed to get mapping")
            .expect("Mapping not found");
        assert_eq!(first_mapping.sender_chat, second_mapping.recipient_chat);
        assert_eq!(first_mapping.recipient_chat, second_mapping.sender_chat);
        assert_eq!(first_mapping.last_private, second_mapping.last_private);
        assert_eq!(first_mapping.last_topic, second_mapping.last_topic);
    }

    #[tokio::test]
    async fn test_delete_mapping() {
        let mut redis_api = get_test_redis().await;

        let mapping = MappingChat {
            sender_chat: 5,
            recipient_chat: 6,
            last_private: 7,
            last_topic: 8,
        };
        let fetched_mapping = redis_api.get_mapping(5).await;
        assert!(fetched_mapping.is_ok_and(|m| m.is_none()));
        redis_api.save_mapping(mapping).await.expect("Failed to save mapping");
        redis_api.delete_mapping(5).await.expect("Failed to delete mapping");
        let fetched_mapping = redis_api.get_mapping(5).await;
        assert!(fetched_mapping.is_ok_and(|m| m.is_none()));
    }

    #[tokio::test]
    async fn test_ban_user() {
        let mut redis_api = get_test_redis().await;

        let banned = redis_api.check_ban(13).await.expect("Failed to check ban");
        assert!(banned.is_none());
        redis_api.ban_user(13).await.expect("Failed to ban user");
        let banned = redis_api.check_ban(13).await.expect("Failed to check ban");
        assert!(banned.is_some());
    }
}
