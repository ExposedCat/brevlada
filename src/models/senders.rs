use super::Message;
#[cfg(test)]
use std::borrow::Borrow;
use std::{borrow::Cow, collections::HashMap};

pub fn identity(message: &Message) -> (String, String) {
    let (name, address) = mailbox(&message.sender);
    (name.into_owned(), address.into_owned())
}

/// The cache key for an address: no display name, no brackets, no casing.
pub fn address(raw: &str) -> String {
    raw.trim().trim_matches(['<', '>']).trim().to_lowercase()
}

pub fn key(message: &Message) -> String {
    address(&mailbox(&message.sender).1)
}

pub fn recipients(message: &Message) -> Vec<(String, String)> {
    let Ok(parsed) = mailparse::addrparse(&message.recipients) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    parsed
        .into_inner()
        .into_iter()
        .flat_map(|item| match item {
            mailparse::MailAddr::Single(item) => vec![item],
            mailparse::MailAddr::Group(group) => group.addrs,
        })
        .filter_map(|item| {
            let email = address(&item.addr);
            (!email.is_empty() && seen.insert(email.clone()))
                .then(|| (item.display_name.unwrap_or_default(), email))
        })
        .collect()
}

pub fn list_identity(message: &Message) -> (String, String) {
    message
        .list_recipient
        .clone()
        .unwrap_or_else(|| identity(message))
}

pub fn list_key(message: &Message) -> String {
    message
        .list_recipient
        .as_ref()
        .map_or_else(|| key(message), |(_, email)| email.clone())
}

pub fn matches(message: &Message, correspondent: &str, outgoing: bool) -> bool {
    if outgoing {
        recipients(message)
            .iter()
            .any(|(_, email)| email == correspondent)
    } else {
        key(message) == correspondent
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Correspondence {
    pub outgoing: bool,
    pub own: std::collections::HashSet<String>,
}

impl Correspondence {
    pub fn matches(&self, message: &Message, correspondent: &str) -> bool {
        matches(
            message,
            correspondent,
            self.outgoing || self.own.contains(&key(message)),
        )
    }

    pub fn groups(&self, messages: Vec<Message>) -> Vec<Vec<Message>> {
        let mut slots = HashMap::new();
        let mut grouped: Vec<Vec<Message>> = Vec::new();
        for message in messages {
            let outgoing = self.outgoing || self.own.contains(&key(&message));
            let identities = if outgoing {
                recipients(&message)
                    .into_iter()
                    .filter(|(_, email)| !self.own.contains(email))
                    .map(Some)
                    .collect()
            } else {
                vec![None]
            };
            for recipient in identities {
                let key = recipient
                    .as_ref()
                    .map_or_else(|| key(&message), |(_, email)| email.clone());
                let slot = *slots.entry(key).or_insert_with(|| {
                    grouped.push(Vec::new());
                    grouped.len() - 1
                });
                grouped[slot].push(Message {
                    list_recipient: recipient,
                    ..message.clone()
                });
            }
        }
        super::sort_groups(&mut grouped);
        grouped
    }
}

pub fn is_sent_folder(folder: &str) -> bool {
    matches!(
        folder
            .rsplit(['/', '.'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "sent" | "sent mail" | "sent items" | "sent messages"
    )
}

#[cfg(test)]
pub fn list_groups(messages: Vec<Message>, outgoing: bool) -> Vec<Vec<Message>> {
    Correspondence {
        outgoing,
        ..Default::default()
    }
    .groups(messages)
}

fn mailbox(sender: &str) -> (Cow<'_, str>, Cow<'_, str>) {
    if let Some((name, address)) = simple_mailbox(sender) {
        return (name.into(), address.into());
    }
    if let Ok(addresses) = mailparse::addrparse(sender)
        && let Some(mailparse::MailAddr::Single(address)) =
            addresses.into_inner().into_iter().next()
    {
        return (
            address.display_name.unwrap_or_default().into(),
            address.addr.into(),
        );
    }
    ("".into(), sender.trim().into())
}

/// Reads `addr`, `Name <addr>` and `"Name" <addr>`, nearly every sender, exactly as
/// `mailparse::addrparse` would but without allocating; None leaves the rest to it.
fn simple_mailbox(sender: &str) -> Option<(&str, &str)> {
    let sender = sender.trim();
    let (name, address) = if let Some(quoted) = sender.strip_prefix('"') {
        let (name, rest) = quoted.split_once('"')?;
        if name.contains('\\') {
            return None;
        }
        (
            name,
            rest.trim_start().strip_prefix('<')?.strip_suffix('>')?,
        )
    } else {
        match sender.find(['<', ';', ',', ':', '(']) {
            None => ("", sender),
            Some(bracket) if sender[bracket..].starts_with('<') => (
                sender[..bracket].trim_end(),
                sender[bracket + 1..].strip_suffix('>')?,
            ),
            Some(_) => return None,
        }
    };
    if address.contains('>') {
        return None;
    }
    Some(if address.contains('@') {
        (name, address)
    } else {
        ("", sender)
    })
}

pub fn preview(messages: &[Message]) -> Option<&Message> {
    if let Some(message) = messages.iter().find(|message| message.search_match) {
        return Some(message);
    }
    messages
        .iter()
        .max_by_key(|message| (!message.is_read, message.timestamp, message.uid))
}

#[cfg(test)]
pub fn groups<M: Borrow<Message>>(
    messages: impl IntoIterator<Item = M>,
    query: &str,
) -> Vec<Vec<M>> {
    let mut slots = HashMap::new();
    let mut groups: Vec<Vec<M>> = Vec::new();
    for message in messages {
        let slot = *slots.entry(key(message.borrow())).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[slot].push(message);
    }
    groups.retain(|group| group.iter().any(|message| message.borrow().matches(query)));
    super::sort_groups(&mut groups);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_inbox_copies_join_the_other_person_instead_of_creating_a_chat_with_me() {
        let scope = Correspondence {
            outgoing: false,
            own: std::collections::HashSet::from([
                "me@example.com".into(),
                "alias@example.com".into(),
            ]),
        };
        let incoming = Message {
            uid: 1,
            sender: "Alice <alice@example.com>".into(),
            recipients: "me@example.com".into(),
            ..Default::default()
        };
        let outgoing = Message {
            uid: 2,
            timestamp: 2,
            sender: "Alias <alias@example.com>".into(),
            recipients: "Alice <alice@example.com>, Me <me@example.com>".into(),
            ..Default::default()
        };
        let groups = scope.groups(vec![incoming.clone(), outgoing.clone()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(list_key(&groups[0][0]), "alice@example.com");
        assert!(scope.matches(&incoming, "alice@example.com"));
        assert!(scope.matches(&outgoing, "alice@example.com"));
        let sent = Correspondence {
            outgoing: true,
            ..scope
        }
        .groups(vec![outgoing]);
        assert_eq!(sent.len(), 1);
        assert_eq!(list_key(&sent[0][0]), "alice@example.com");
    }

    #[test]
    fn sent_groups_use_each_unique_to_address_without_changing_from() {
        let messages = vec![
            Message {
                uid: 3,
                timestamp: 3,
                sender: "Me <me@example.com>".into(),
                recipients:
                    "\"Alice Smith\" <ALICE@example.com>, Bob <bob@example.com>, alice@example.com"
                        .into(),
                ..Default::default()
            },
            Message {
                uid: 2,
                timestamp: 2,
                sender: "Alias <alias@example.com>".into(),
                recipients: "Old Alice <alice@example.com>".into(),
                ..Default::default()
            },
            Message {
                uid: 1,
                timestamp: 1,
                sender: "Me <me@example.com>".into(),
                recipients: "Carol <carol@example.com>".into(),
                ..Default::default()
            },
        ];
        let grouped = list_groups(messages.clone(), true);
        assert_eq!(grouped.len(), 3);
        let alice = grouped
            .iter()
            .find(|group| list_key(&group[0]) == "alice@example.com")
            .unwrap();
        assert_eq!(
            alice.iter().map(|message| message.uid).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(
            list_identity(&alice[0]),
            ("Alice Smith".into(), "alice@example.com".into())
        );
        assert_eq!(alice[0].sender, messages[0].sender);
        assert_eq!(key(&alice[0]), "me@example.com");
        assert!(matches(&alice[0], "bob@example.com", true));
        assert!(!matches(&alice[0], "me@example.com", true));
        assert!(alice[0].list_header(false).list_recipient.is_some());
        let inbox = list_groups(messages, false);
        assert_eq!(inbox.len(), 2);
        assert!(
            inbox
                .iter()
                .flatten()
                .all(|message| message.list_recipient.is_none())
        );
    }

    #[test]
    fn unread_senders_come_first_with_newest_first_within_each_section() {
        let message = |uid, sender: &str, is_read| Message {
            uid,
            timestamp: uid as i64,
            sender: sender.into(),
            is_read,
            ..Default::default()
        };
        let messages = vec![
            message(1, "mixed@example.com", false),
            message(5, "mixed@example.com", true),
            message(3, "unread@example.com", false),
            message(9, "read@example.com", true),
            message(7, "other-read@example.com", true),
        ];
        assert_eq!(
            groups(&messages, "")
                .iter()
                .map(|group| group[0].uid)
                .collect::<Vec<_>>(),
            vec![5, 3, 9, 7]
        );
    }

    #[test]
    fn preview_prefers_first_unread_then_latest_read_regardless_of_cached_body() {
        let mut messages = vec![
            Message {
                uid: 1,
                timestamp: 1,
                sender: "sender@example.com".into(),
                subject: "Older read message".into(),
                is_read: true,
                body_loaded: true,
                body_text: "Cached".into(),
                ..Default::default()
            },
            Message {
                uid: 2,
                timestamp: 2,
                sender: "sender@example.com".into(),
                subject: "Latest unread message".into(),
                ..Default::default()
            },
        ];
        assert_eq!(groups(&messages, "")[0][0].uid, 2);
        assert_eq!(preview(&messages).unwrap().uid, 2);
        messages[0].is_read = false;
        messages[1].is_read = true;
        assert_eq!(groups(&messages, "")[0][0].uid, 2);
        assert_eq!(preview(&messages).unwrap().uid, 1);
        assert_eq!(
            preview(&groups(messages.clone(), "latest")[0]).unwrap().uid,
            1
        );
        messages[1].is_read = false;
        assert_eq!(preview(&messages).unwrap().uid, 2);
        messages[1].timestamp = messages[0].timestamp;
        assert_eq!(preview(&messages).unwrap().uid, 2);
        for message in &mut messages {
            message.is_read = true;
        }
        assert_eq!(preview(&messages).unwrap().uid, 2);
        assert!(preview(&[]).is_none());
    }

    #[test]
    fn groups_mailboxes_despite_name_changes_and_keeps_latest_preview_when_searching() {
        let message = |uid, sender: &str, subject: &str| Message {
            uid,
            timestamp: uid as i64,
            sender: sender.into(),
            subject: subject.into(),
            ..Default::default()
        };
        let messages = vec![
            message(1, "Old Name <ALICE@example.com>", "Find me"),
            message(3, "Alice <alice@example.com>", "Latest"),
            message(2, "Alice <other@example.com>", "Separate"),
        ];
        let grouped = groups(&messages, "");
        assert_eq!(grouped.len(), 2);
        assert_eq!(
            grouped[0].iter().map(|m| m.uid).collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert_eq!(groups(&messages, "find me")[0][0].subject, "Latest");
        assert!(groups(&messages, "missing").is_empty());
        let filtered: Vec<_> = messages
            .into_iter()
            .filter(|m| key(m) == "alice@example.com")
            .collect();
        assert!(
            super::super::threads(&filtered, "")
                .iter()
                .flatten()
                .all(|m| m.uid != 2)
        );
    }

    fn mailparse_identity(sender: &str) -> (String, String) {
        if let Ok(addresses) = mailparse::addrparse(sender)
            && let Some(mailparse::MailAddr::Single(address)) = addresses.first()
        {
            return (
                address.display_name.clone().unwrap_or_default(),
                address.addr.clone(),
            );
        }
        (String::new(), sender.trim().to_owned())
    }

    fn senders() -> impl Iterator<Item = String> {
        let parts = [
            "", " ", "\u{3000}", "\"", "\\", "<", ">", ",", ";", ":", "(", ")", "Na me", "a@B.c",
            "\"Name\"", "<a@b.c>", "@",
        ];
        parts.into_iter().flat_map(move |a| {
            parts.into_iter().flat_map(move |b| {
                parts
                    .into_iter()
                    .flat_map(move |c| parts.into_iter().map(move |d| [a, b, c, d].concat()))
            })
        })
    }

    #[test]
    fn identity_matches_mailparse_for_every_sender_shape() {
        for sender in senders() {
            let message = Message {
                sender,
                ..Default::default()
            };
            assert_eq!(
                identity(&message),
                mailparse_identity(&message.sender),
                "{:?}",
                message.sender
            );
        }
    }

    #[test]
    fn groups_match_grouping_every_message_by_its_parsed_address() {
        let messages: Vec<_> = senders()
            .step_by(7)
            .enumerate()
            .map(|(uid, sender)| Message {
                uid: uid as u32,
                timestamp: (uid * 13 % 500) as i64,
                is_read: uid % 3 != 0,
                subject: format!("Subject {}", uid % 11),
                sender,
                ..Default::default()
            })
            .collect();
        for query in ["", "subject 3"] {
            let mut expected: HashMap<String, Vec<Message>> = HashMap::new();
            for message in &messages {
                expected
                    .entry(address(&mailparse_identity(&message.sender).1))
                    .or_default()
                    .push(message.clone());
            }
            let mut expected: Vec<_> = expected
                .into_values()
                .filter(|group| group.iter().any(|message| message.matches(query)))
                .collect();
            for group in &mut expected {
                group.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.uid)));
            }
            expected.sort_by_key(|group| {
                std::cmp::Reverse((
                    group.iter().any(|m| !m.is_read),
                    group[0].timestamp,
                    group[0].uid,
                ))
            });
            assert!(expected.len() > 100);
            assert_eq!(groups(messages.clone(), query), expected);
        }
    }
}
