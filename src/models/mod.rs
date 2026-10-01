pub mod action_target;
pub mod conversation;
pub mod draft;
pub mod parcel;
pub mod pending_actions;
pub mod read_state;
pub mod sender_action;
pub mod sender_pane;
pub mod senders;
pub mod sync;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Account {
    pub path: String,
    pub email: String,
    pub name: String,
    pub host: String,
    pub username: String,
    pub port: u16,
    pub ssl: bool,
    pub tls: bool,
    pub oauth2: bool,
    pub smtp: Option<SmtpSettings>,
}

#[derive(Clone, Debug)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub ssl: bool,
    pub tls: bool,
    pub auth: bool,
    pub login: bool,
    pub plain: bool,
    pub xoauth2: bool,
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub attachments: Vec<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SentMessage {
    pub folder: String,
    pub message: Message,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct Message {
    pub uid: u32,
    #[serde(default)]
    pub uid_validity: Option<u32>,
    pub message_id: String,
    pub subject: String,
    pub sender: String,
    pub recipients: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    pub date: String,
    pub timestamp: i64,
    pub references: Vec<String>,
    pub is_read: bool,
    #[serde(default)]
    pub read_revision: i64,
    #[serde(default)]
    pub is_draft: bool,
    #[serde(default)]
    pub is_flagged: bool,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub inline_media: Vec<InlineMedia>,
    #[serde(default)]
    pub inline_media_loaded: bool,
    #[serde(default)]
    pub unsubscribe: Option<Unsubscribe>,
    #[serde(default)]
    pub parcels: Vec<parcel::Parcel>,
    #[serde(default)]
    pub body_loaded: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Unsubscribe {
    pub url: String,
    #[serde(default)]
    pub one_click: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InlineMedia {
    pub content_id: String,
    pub mime: String,
    pub data: String,
}

impl Message {
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        [&self.sender, &self.subject, &self.body_text]
            .iter()
            .any(|v| v.to_lowercase().contains(query))
    }
}

pub fn threads(messages: &[Message], query: &str) -> Vec<Vec<Message>> {
    let matched: Vec<&Message> = messages.iter().filter(|m| m.matches(query)).collect();
    let mut parents: Vec<usize> = (0..matched.len()).collect();
    fn root(parents: &mut [usize], index: usize) -> usize {
        if parents[index] != index {
            parents[index] = root(parents, parents[index]);
        }
        parents[index]
    }
    fn union(parents: &mut [usize], a: usize, b: usize) {
        let a = root(parents, a);
        let b = root(parents, b);
        parents[b] = a;
    }
    let mut subjects = std::collections::HashMap::new();
    let mut ids = std::collections::HashMap::new();
    let mut referenced_by: std::collections::HashMap<&str, Vec<usize>> =
        std::collections::HashMap::new();
    for (index, message) in matched.iter().enumerate() {
        let subject = subject_key(&message.subject);
        if !subject.is_empty() {
            if let Some(other) = subjects.insert(subject, index) {
                union(&mut parents, index, other);
            }
        }
        if !message.message_id.is_empty() {
            if let Some(other) = ids.insert(message.message_id.as_str(), index) {
                union(&mut parents, index, other);
            }
            if let Some(others) = referenced_by.get(message.message_id.as_str()) {
                for other in others {
                    union(&mut parents, index, *other);
                }
            }
        }
        for reference in &message.references {
            if let Some(other) = ids.get(reference.as_str()) {
                union(&mut parents, index, *other);
            }
            referenced_by
                .entry(reference.as_str())
                .or_default()
                .push(index);
        }
    }
    let mut by_root: std::collections::HashMap<usize, Vec<Message>> =
        std::collections::HashMap::new();
    for (index, message) in matched.into_iter().enumerate() {
        by_root
            .entry(root(&mut parents, index))
            .or_default()
            .push(message.clone());
    }
    let mut groups: Vec<Vec<Message>> = by_root.into_values().collect();
    for group in &mut groups {
        group.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.uid)));
    }
    groups.sort_by_key(|g| {
        std::cmp::Reverse((
            g.iter().any(|m| !m.is_read),
            g.first().map(|m| m.timestamp).unwrap_or_default(),
            g.first().map(|m| m.uid).unwrap_or_default(),
        ))
    });
    groups
}

pub fn subject_key(subject: &str) -> String {
    static PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(?:(?:re|fw|fwd|aw|antw|回复|转发):\s*)+").unwrap()
    });
    PREFIX.replace_all(subject, "").trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_reference_bridges_and_sorts_unread_first() {
        let messages = vec![
            Message {
                uid: 1,
                message_id: "a".into(),
                timestamp: 1,
                subject: "First".into(),
                is_read: true,
                ..Default::default()
            },
            Message {
                uid: 2,
                message_id: "b".into(),
                timestamp: 3,
                subject: "Second".into(),
                is_read: true,
                ..Default::default()
            },
            Message {
                uid: 3,
                timestamp: 2,
                references: vec!["a".into(), "b".into()],
                subject: "Bridge".into(),
                ..Default::default()
            },
            Message {
                uid: 4,
                subject: "Separate".into(),
                is_read: true,
                timestamp: 100,
                ..Default::default()
            },
        ];
        let groups = threads(&messages, "");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 3);
        assert_eq!(
            groups[0].iter().map(|m| m.uid).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    fn normalizes_repeated_reply_prefixes_and_searches_sender() {
        let messages = vec![
            Message {
                subject: "Topic".into(),
                sender: "Alice".into(),
                ..Default::default()
            },
            Message {
                subject: "Re: Fwd: Topic".into(),
                ..Default::default()
            },
        ];
        assert_eq!(threads(&messages, "").len(), 1);
        assert_eq!(threads(&messages, "alice")[0].len(), 1);
        assert!(threads(&messages, "missing").is_empty());
    }

    #[test]
    fn shared_reference_without_matching_message_does_not_join_threads() {
        let messages = vec![
            Message {
                uid: 1,
                subject: "One".into(),
                references: vec!["missing".into()],
                ..Default::default()
            },
            Message {
                uid: 2,
                subject: "Two".into(),
                references: vec!["missing".into()],
                ..Default::default()
            },
        ];
        assert_eq!(threads(&messages, "").len(), 2);
        let mut linked = messages;
        linked.push(Message {
            uid: 3,
            message_id: "missing".into(),
            subject: "Three".into(),
            ..Default::default()
        });
        assert_eq!(threads(&linked, "").len(), 1);
    }
}
