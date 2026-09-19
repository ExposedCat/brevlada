use super::{Message, senders};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActionTarget {
    Sender(String),
    Messages(Vec<(u32, String)>),
}

impl ActionTarget {
    pub fn matches(&self, message: &Message) -> bool {
        match self {
            Self::Sender(sender) => senders::key(message) == *sender,
            Self::Messages(messages) => messages
                .iter()
                .any(|(uid, id)| *uid == message.uid && *id == message.message_id),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Sender(sender) => sender,
            Self::Messages(_) => "selected messages",
        }
    }
}

impl From<&str> for ActionTarget {
    fn from(sender: &str) -> Self {
        Self::Sender(sender.into())
    }
}

impl From<String> for ActionTarget {
    fn from(sender: String) -> Self {
        Self::Sender(sender)
    }
}

impl From<&ActionTarget> for ActionTarget {
    fn from(target: &ActionTarget) -> Self {
        target.clone()
    }
}
