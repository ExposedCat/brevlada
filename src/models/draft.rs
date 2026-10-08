use super::{Draft, Message};

#[derive(Clone, Debug)]
pub struct Target {
    pub folder: String,
    pub uid: u32,
    pub validity: Option<u32>,
    pub message_id: String,
}

impl Target {
    pub fn new(folder: &str, message: &Message) -> Self {
        Self {
            folder: folder.into(),
            uid: message.uid,
            validity: message.uid_validity,
            message_id: message.message_id.clone(),
        }
    }
}

pub struct Saved {
    pub item: super::SentMessage,
    pub original_parts: Vec<String>,
    pub cleanup_error: Option<String>,
}

pub struct Outcome {
    pub message: Message,
    pub cleanup_error: Option<String>,
}

impl From<&Message> for Draft {
    fn from(message: &Message) -> Self {
        Self {
            attachments_loaded: false,
            to: message.recipients.clone(),
            cc: message.cc.clone(),
            subject: message.subject.clone(),
            text: message.body_text.clone(),
            html: (!message.body_html.is_empty()).then(|| message.body_html.clone()),
            attachments: Vec::new(),
            attachment_names: message.attachments.clone(),
            removed_attachments: Vec::new(),
            attachment_source: None,
            in_reply_to: message.in_reply_to.clone(),
            references: message.references.clone(),
        }
    }
}
