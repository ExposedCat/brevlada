use super::{Message, senders};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reply,
    ReplyAll,
    Forward,
}

pub fn recipients(message: &Message, account: &str, action: Action) -> (String, String) {
    if action == Action::Forward {
        return (String::new(), String::new());
    }
    let own = senders::key(message) == senders::address(account);
    let target = if own {
        &message.recipients
    } else if !addresses(&message.reply_to).is_empty() {
        &message.reply_to
    } else {
        &message.sender
    };
    let mut seen = HashSet::from([senders::address(account)]);
    let mut unique = |value: &str| {
        addresses(value)
            .into_iter()
            .filter(|address| seen.insert(address.addr.to_ascii_lowercase()))
            .map(|address| address.to_string())
            .collect::<Vec<_>>()
    };
    let mut to = unique(target);
    let mut cc = Vec::new();
    if action == Action::ReplyAll {
        to.extend(unique(&message.recipients));
        cc = unique(&message.cc);
    }
    (to.join(", "), cc.join(", "))
}

fn addresses(value: &str) -> Vec<mailparse::SingleInfo> {
    mailparse::addrparse(value)
        .map(|addresses| {
            addresses
                .iter()
                .flat_map(|address| match address {
                    mailparse::MailAddr::Single(address) => vec![address.clone()],
                    mailparse::MailAddr::Group(group) => group.addrs.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn subject(message: &Message, action: Action) -> String {
    let subject = message.subject.trim();
    let lower = subject.to_ascii_lowercase();
    let already_prefixed = match action {
        Action::Forward => lower.starts_with("fwd:") || lower.starts_with("fw:"),
        _ => lower.starts_with("re:"),
    };
    if already_prefixed {
        subject.to_owned()
    } else {
        let prefix = if action == Action::Forward {
            "Fwd"
        } else {
            "Re"
        };
        format!("{prefix}: {subject}")
    }
}

pub fn forward_header(message: &Message) -> String {
    let mut header = format!(
        "---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n",
        message.sender, message.date, message.subject, message.recipients
    );
    if !message.cc.trim().is_empty() {
        header.push_str(&format!("Cc: {}\n", message.cc));
    }
    header.push('\n');
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Message {
        Message {
            sender: "Sender <sender@example.com>".into(),
            reply_to: "Support <support@example.com>, Backup <backup@example.com>".into(),
            recipients: "Me <ME@example.com>, Support <SUPPORT@example.com>, Team: Alice <alice@example.com>, Bob <bob@example.com>;".into(),
            cc: "Alice <ALICE@example.com>, Copy <copy@example.com>, me@example.com".into(),
            ..Default::default()
        }
    }

    #[test]
    fn reply_honors_all_reply_to_addresses_and_falls_back_to_from() {
        let mut message = message();
        assert_eq!(
            recipients(&message, "me@example.com", Action::Reply),
            (
                "\"Support\" <support@example.com>, \"Backup\" <backup@example.com>".into(),
                String::new()
            )
        );
        for reply_to in ["", "invalid <"] {
            message.reply_to = reply_to.into();
            assert_eq!(
                recipients(&message, "me@example.com", Action::Reply).0,
                "\"Sender\" <sender@example.com>"
            );
        }
    }

    #[test]
    fn reply_all_flattens_groups_deduplicates_and_excludes_self() {
        let (to, cc) = recipients(&message(), "me@example.com", Action::ReplyAll);
        assert_eq!(
            to,
            "\"Support\" <support@example.com>, \"Backup\" <backup@example.com>, \"Alice\" <alice@example.com>, \"Bob\" <bob@example.com>"
        );
        assert_eq!(cc, "\"Copy\" <copy@example.com>");
    }

    #[test]
    fn own_messages_reply_to_original_recipients() {
        let message = Message {
            sender: "ME@example.com".into(),
            ..message()
        };
        let (to, cc) = recipients(&message, "me@example.com", Action::ReplyAll);
        assert!(to.contains("SUPPORT@example.com"));
        assert!(!to.contains("backup@example.com"));
        assert!(!to.contains("ME@example.com"));
        assert_eq!(cc, "\"Copy\" <copy@example.com>");
    }

    #[test]
    fn forward_starts_without_recipients_and_does_not_repeat_prefixes() {
        let message = Message {
            subject: " FW: Plans ".into(),
            ..message()
        };
        assert_eq!(
            recipients(&message, "me@example.com", Action::Forward),
            (String::new(), String::new())
        );
        assert_eq!(subject(&message, Action::Forward), "FW: Plans");
        assert_eq!(subject(&message, Action::Reply), "Re: FW: Plans");
        assert!(forward_header(&message).contains("From: Sender <sender@example.com>"));
        assert!(forward_header(&message).contains("Cc: Alice"));
    }
}
