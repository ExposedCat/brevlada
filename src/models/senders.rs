use super::Message;
use std::{borrow::Borrow, collections::HashMap};

pub fn identity(message: &Message) -> (String, String) {
    if let Ok(addresses) = mailparse::addrparse(&message.sender)
        && let Some(mailparse::MailAddr::Single(address)) = addresses.first()
    {
        return (
            address.display_name.clone().unwrap_or_default(),
            address.addr.clone(),
        );
    }
    (String::new(), message.sender.trim().to_owned())
}

/// The cache key for an address: no display name, no brackets, no casing.
pub fn address(raw: &str) -> String {
    raw.trim().trim_matches(['<', '>']).trim().to_lowercase()
}

pub fn key(message: &Message) -> String {
    address(&identity(message).1)
}

pub fn preview(messages: &[Message]) -> Option<&Message> {
    if let Some(message) = messages.iter().find(|message| message.search_match) {
        return Some(message);
    }
    messages
        .iter()
        .max_by_key(|message| (!message.is_read, message.timestamp, message.uid))
}

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
