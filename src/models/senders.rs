use super::Message;
use std::collections::HashMap;

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

pub fn groups(messages: &[Message], query: &str) -> Vec<Vec<Message>> {
    let mut senders: HashMap<String, Vec<Message>> = HashMap::new();
    for message in messages {
        senders
            .entry(key(message))
            .or_default()
            .push(message.clone());
    }
    let mut groups: Vec<_> = senders
        .into_values()
        .filter(|group| group.iter().any(|message| message.matches(query)))
        .collect();
    for group in &mut groups {
        group.sort_by_key(|message| std::cmp::Reverse((message.timestamp, message.uid)));
    }
    groups.sort_by_key(|group| {
        std::cmp::Reverse((
            group.iter().any(|message| !message.is_read),
            group[0].timestamp,
            group[0].uid,
        ))
    });
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
    fn latest_subject_does_not_depend_on_read_status_or_cached_body() {
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
        messages[0].is_read = false;
        messages[1].is_read = true;
        assert_eq!(groups(&messages, "")[0][0].uid, 2);
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
}
