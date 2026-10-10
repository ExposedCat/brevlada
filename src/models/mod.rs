pub mod action_target;
pub mod attachment;
pub mod calendar;
pub mod conversation;
pub mod draft;
pub mod parcel;
pub mod pending_actions;
pub mod read_state;
pub mod response;
mod schema;
pub mod search;
pub mod sender_action;
pub mod sender_pane;
pub mod senders;
pub mod settings;
pub mod sync;
pub mod ticket;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, collections::HashMap};

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
    pub attachments_loaded: bool,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub attachments: Vec<String>,
    pub attachment_names: Vec<String>,
    pub removed_attachments: Vec<usize>,
    pub attachment_source: Option<draft::Target>,
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
    #[serde(default)]
    pub reply_to: String,
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
    #[serde(default)]
    pub is_spam: bool,
    #[serde(default)]
    pub flag_revision: i64,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<String>,
    #[serde(default)]
    pub attachment_details: Vec<attachment::Details>,
    #[serde(default)]
    pub inline_media: Vec<InlineMedia>,
    #[serde(default)]
    pub inline_media_loaded: bool,
    #[serde(default)]
    pub unsubscribe: Option<Unsubscribe>,
    #[serde(default)]
    pub parcels: Vec<parcel::Parcel>,
    #[serde(default)]
    pub calendar_events: Vec<calendar::Event>,
    #[serde(default)]
    pub tickets: Vec<ticket::Reservation>,
    #[serde(default)]
    pub body_loaded: bool,
    /// Prepared on a cache worker; never persisted or recomputed while scrolling.
    #[serde(skip)]
    pub list_preview: Option<String>,
    #[serde(skip)]
    pub display_prepared: bool,
    #[serde(skip)]
    pub remote_media: Option<bool>,
    #[serde(skip)]
    pub search_match: bool,
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
    pub fn prepare_display(&mut self) {
        if self.display_prepared {
            return;
        }
        if self.tickets.is_empty() && !self.body_html.is_empty() {
            self.tickets = ticket::parse(&self.body_html);
        }
        if self.body_loaded {
            if self.parcels.is_empty() {
                self.parcels = parcel::parse_message(self);
            }
            self.calendar_events = calendar::events(self);
        }
        if self.body_loaded || !self.body_text.is_empty() || !self.body_html.is_empty() {
            self.list_preview = Some(crate::components::preview::message(self));
        }
        self.remote_media = Some(crate::components::html::has_remote_media(self));
        self.display_prepared = true;
    }

    /// List rows carry identities, flags, and a short preview, never body payloads.
    pub fn list_header(&self, preview: bool) -> Self {
        let text = if preview {
            self.list_preview
                .clone()
                .unwrap_or_else(|| self.body_text.chars().take(161).collect())
        } else {
            String::new()
        };
        Self {
            uid: self.uid,
            uid_validity: self.uid_validity,
            message_id: self.message_id.clone(),
            subject: self.subject.clone(),
            sender: self.sender.clone(),
            reply_to: self.reply_to.clone(),
            recipients: self.recipients.clone(),
            cc: self.cc.clone(),
            in_reply_to: self.in_reply_to.clone(),
            references: self.references.clone(),
            timestamp: self.timestamp,
            date: self.date.clone(),
            is_read: self.is_read,
            read_revision: self.read_revision,
            is_flagged: self.is_flagged,
            flag_revision: self.flag_revision,
            is_spam: self.is_spam,
            is_draft: self.is_draft,
            body_text: text,
            list_preview: preview.then(|| self.list_preview.clone()).flatten(),
            calendar_events: self.calendar_events.clone(),
            tickets: self.tickets.clone(),
            display_prepared: true,
            search_match: self.search_match,
            ..Default::default()
        }
    }

    /// Searching needs body text, but not inline images, attachment data or cards.
    pub fn search_document(&self) -> Self {
        Self {
            body_text: self.body_text.clone(),
            body_html: self.body_html.clone(),
            body_loaded: self.body_loaded,
            display_prepared: self.display_prepared,
            remote_media: self.remote_media,
            ..self.list_header(true)
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        search::fields(self)
            .iter()
            .any(|v| v.to_lowercase().contains(query))
    }
}

pub fn threads<M: Borrow<Message>>(
    messages: impl IntoIterator<Item = M>,
    query: &str,
) -> Vec<Vec<M>> {
    let matched: Vec<M> = messages
        .into_iter()
        .filter(|message| message.borrow().matches(query))
        .collect();
    let mut parents: Vec<usize> = (0..matched.len()).collect();
    fn root(parents: &mut [usize], mut index: usize) -> usize {
        while parents[index] != index {
            parents[index] = parents[parents[index]];
            index = parents[index];
        }
        index
    }
    fn union(parents: &mut [usize], a: usize, b: usize) {
        let a = root(parents, a);
        let b = root(parents, b);
        parents[b] = a;
    }
    {
        let mut subjects = HashMap::new();
        let mut ids = HashMap::new();
        for (index, message) in matched.iter().enumerate() {
            let message = message.borrow();
            let subject = subject_key(&message.subject);
            if !subject.is_empty() {
                union(
                    &mut parents,
                    *subjects.entry(subject).or_insert(index),
                    index,
                );
            }
            if !message.message_id.is_empty() {
                union(
                    &mut parents,
                    *ids.entry(message.message_id.as_str()).or_insert(index),
                    index,
                );
            }
        }
        // References join only once every message id is known, in either direction.
        for (index, message) in matched.iter().enumerate() {
            for reference in &message.borrow().references {
                if let Some(&other) = ids.get(reference.as_str()) {
                    union(&mut parents, other, index);
                }
            }
        }
    }
    let mut slots = vec![None; matched.len()];
    let mut groups: Vec<Vec<M>> = Vec::new();
    for (index, message) in matched.into_iter().enumerate() {
        let slot = *slots[root(&mut parents, index)].get_or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(message);
    }
    sort_groups(&mut groups);
    groups
}

/// Newest messages first within each group; groups with unread mail, then newest first.
fn sort_groups<M: Borrow<Message>>(groups: &mut [Vec<M>]) {
    for group in groups.iter_mut() {
        group.sort_by_key(|message| {
            let message = message.borrow();
            std::cmp::Reverse((message.timestamp, message.uid))
        });
    }
    groups.sort_by_cached_key(|group| {
        let newest = group[0].borrow();
        std::cmp::Reverse((
            group.iter().any(|message| !message.borrow().is_read),
            newest.timestamp,
            newest.uid,
        ))
    });
}

pub fn subject_key(mut subject: &str) -> String {
    const PREFIXES: [&str; 7] = ["re", "fw", "fwd", "aw", "antw", "回复", "转发"];
    while let Some(rest) = PREFIXES.iter().find_map(|prefix| {
        let (head, rest) = subject.split_at_checked(prefix.len())?;
        rest.strip_prefix(':')
            .filter(|_| head.eq_ignore_ascii_case(prefix))
    }) {
        subject = rest.trim_start();
    }
    subject.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_subject_history_does_not_recurse_through_parent_chain() {
        let messages: Vec<_> = (1..=20000)
            .map(|uid| Message {
                uid,
                subject: "Same topic".into(),
                ..Default::default()
            })
            .collect();
        let groups = threads(&messages, "");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 20000);
    }

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

    /// The previous single-pass grouping, kept to check the rewrite against it.
    fn pairwise_threads(messages: &[Message]) -> Vec<Vec<Message>> {
        let mut parents: Vec<usize> = (0..messages.len()).collect();
        fn root(parents: &mut [usize], mut index: usize) -> usize {
            while parents[index] != index {
                index = parents[index];
            }
            index
        }
        let mut union = |a: usize, b: usize| {
            let (a, b) = (root(&mut parents, a), root(&mut parents, b));
            parents[b] = a;
        };
        let mut subjects = HashMap::new();
        let mut ids = HashMap::new();
        let mut referenced_by: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, message) in messages.iter().enumerate() {
            let subject = subject_key(&message.subject);
            if !subject.is_empty()
                && let Some(other) = subjects.insert(subject, index)
            {
                union(index, other);
            }
            if !message.message_id.is_empty() {
                if let Some(other) = ids.insert(message.message_id.as_str(), index) {
                    union(index, other);
                }
                for other in referenced_by
                    .get(message.message_id.as_str())
                    .into_iter()
                    .flatten()
                {
                    union(index, *other);
                }
            }
            for reference in &message.references {
                if let Some(other) = ids.get(reference.as_str()) {
                    union(index, *other);
                }
                referenced_by
                    .entry(reference.as_str())
                    .or_default()
                    .push(index);
            }
        }
        let mut by_root: HashMap<usize, Vec<Message>> = HashMap::new();
        for (index, message) in messages.iter().enumerate() {
            by_root
                .entry(root(&mut parents, index))
                .or_default()
                .push(message.clone());
        }
        let mut groups: Vec<_> = by_root.into_values().collect();
        for group in &mut groups {
            group.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.uid)));
        }
        groups.sort_by_key(|g| {
            std::cmp::Reverse((g.iter().any(|m| !m.is_read), g[0].timestamp, g[0].uid))
        });
        groups
    }

    #[test]
    fn threads_match_the_pairwise_grouping() {
        let prefixes = ["", "Re: ", "RE:", "Fwd: Re: ", "aw:  "];
        let messages: Vec<_> = (0..900u32)
            .map(|uid| Message {
                uid,
                timestamp: i64::from(uid * 13 % 70),
                is_read: uid % 4 != 0,
                subject: match uid * 7 % 300 {
                    topic @ 0..120 => format!("{}Topic {topic}", prefixes[uid as usize % 5]),
                    _ => String::new(),
                },
                message_id: if uid % 5 == 0 {
                    String::new()
                } else {
                    format!("id{}", uid * 3 % 700)
                },
                references: (0..uid % 3)
                    .map(|step| format!("id{}", (uid * 31 + step * 17) % 900))
                    .collect(),
                ..Default::default()
            })
            .collect();
        let grouped: Vec<Vec<Message>> = threads(&messages, "")
            .into_iter()
            .map(|group| group.into_iter().cloned().collect())
            .collect();
        assert!(grouped.len() > 50 && grouped.iter().any(|group| group.len() > 5));
        assert_eq!(grouped, pairwise_threads(&messages));
    }

    #[test]
    fn subject_key_strips_what_the_reply_prefix_pattern_matched() {
        let pattern = regex::Regex::new(r"(?i)^(?:(?:re|fw|fwd|aw|antw|回复|转发):\s*)+").unwrap();
        let parts = [
            "re", "RE", "Fw", "fWD", "aw", "AnTw", "回复", "转发", ":", " ", "\u{3000}", "\t", "x",
            "Ре", "é",
        ];
        for a in parts {
            for b in parts {
                for c in parts {
                    for d in parts {
                        let subject = [a, b, c, d, " Topic "].concat();
                        assert_eq!(
                            subject_key(&subject),
                            pattern.replace_all(&subject, "").trim().to_lowercase(),
                            "{subject:?}"
                        );
                    }
                }
            }
        }
    }
}
