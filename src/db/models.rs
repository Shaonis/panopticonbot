#[derive(Clone, Copy, Debug)]
pub struct MappingChat {
    pub sender_chat: i64,
    pub recipient_chat: i64,
    pub last_private: i32,
    pub last_topic: i32,
}

impl MappingChat {
    pub fn new(private_chat: i64, forum_topic: i64, last_private: i32, last_topic: i32) -> Self {
        Self {
            sender_chat: private_chat,
            recipient_chat: forum_topic,
            last_private,
            last_topic,
        }
    }

    pub fn sync(&mut self, last_private: i32, last_topic: i32) {
        self.last_private = last_private;
        self.last_topic = last_topic;
    }

    pub fn unique_id(&self) -> i64 {
        self.sender_chat.min(self.recipient_chat)
    }
}
