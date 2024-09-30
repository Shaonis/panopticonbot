use teloxide::types::{ChatId, MessageId};

/// Represents a link between chat rooms: a user in private messages and a forum topic.
/// The values of the `sender_chat` i `recipient_chat` fields can be swapped.
/// When receiving the structure, `sender_chat` is the initiator.
/// The `last_private`, `last_topic` fields store the id of the last message in a private and a topic respectively.
#[derive(Clone, Copy, Debug)]
pub struct MappingChat {
    pub sender_chat: ChatId,
    pub recipient_chat: ChatId,
    pub last_private: MessageId,
    pub last_topic: MessageId,
}

impl MappingChat {
    pub fn new(
        private_chat: ChatId, 
        forum_topic: ChatId, 
        last_private: MessageId, 
        last_topic: MessageId,
    ) -> Self {
        Self {
            sender_chat: private_chat,
            recipient_chat: forum_topic,
            last_private,
            last_topic,
        }
    }

    pub fn sync(&mut self, last_private: MessageId, last_topic: MessageId) {
        self.last_private = last_private;
        self.last_topic = last_topic;
    }

    pub fn unique_id(&self) -> i64 {
        self.sender_chat.0.min(self.recipient_chat.0)
    }
}

impl From<(i64, i64, i32, i32)> for MappingChat {
    fn from(tuple: (i64, i64, i32, i32)) -> Self {
        Self {
            sender_chat: ChatId(tuple.0),
            recipient_chat: ChatId(tuple.1),
            last_private: MessageId(tuple.2),
            last_topic: MessageId(tuple.3),
        }
    }
}

impl From<MappingChat> for (i64, i64, i32, i32) {
    fn from(mapping: MappingChat) -> Self {
        (
            mapping.sender_chat.0, 
            mapping.recipient_chat.0, 
            mapping.last_private.0, 
            mapping.last_topic.0
        )
    }
}
