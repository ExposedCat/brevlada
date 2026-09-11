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
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct Message {
    pub uid: u32,
    pub message_id: String,
    pub subject: String,
    pub sender: String,
    pub recipients: String,
    pub date: String,
    pub timestamp: i64,
    pub references: Vec<String>,
    pub is_read: bool,
    #[serde(default)]
    pub is_flagged: bool,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub body_loaded: bool,
}

impl Message {
    pub fn matches(&self, query: &str) -> bool {
        [&self.sender, &self.subject, &self.body_text]
            .iter()
            .any(|v| v.to_lowercase().contains(query))
    }
}

pub fn threads(messages: &[Message], query: &str) -> Vec<Vec<Message>> {
    static PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(?:(?:re|fw|fwd|aw|antw|回复|转发):\s*)+").unwrap()
    });
    let prefix = &*PREFIX;
    let subject = |m: &Message| prefix.replace_all(&m.subject, "").trim().to_lowercase();
    let mut groups: Vec<Vec<Message>> = Vec::new();
    for message in messages.iter().filter(|m| m.matches(query)) {
        let normalized = subject(message);
        let mut merged = vec![message.clone()];
        let mut index = 0;
        while index < groups.len() {
            let related = groups[index].iter().any(|existing| {
                (!normalized.is_empty() && subject(existing) == normalized)
                    || merged.iter().any(|m| {
                        (!existing.message_id.is_empty()
                            && m.references.contains(&existing.message_id))
                            || (!m.message_id.is_empty()
                                && existing.references.contains(&m.message_id))
                    })
            });
            if related {
                merged.extend(groups.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        groups.push(merged);
    }
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
}
