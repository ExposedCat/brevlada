use super::{Message, SentMessage, senders, subject_key};
use std::collections::HashSet;

pub fn quoted_message_visible<'a>(
    message: &Message,
    older: impl IntoIterator<Item = &'a Message>,
) -> bool {
    older.into_iter().any(|item| {
        (!item.message_id.is_empty() && message.references.contains(&item.message_id))
            || same_conversation(message, item)
            || same_conversation(item, message)
    })
}

pub fn related(received: &[Message], candidates: &[SentMessage]) -> Vec<SentMessage> {
    related_direction(received, candidates, false)
}

pub fn related_direction(
    received: &[Message],
    candidates: &[SentMessage],
    outgoing: bool,
) -> Vec<SentMessage> {
    let received_ids: HashSet<_> = received
        .iter()
        .map(|message| message.message_id.trim().trim_matches(['<', '>']))
        .collect();
    let mut linked: HashSet<String> = received
        .iter()
        .flat_map(|message| {
            std::iter::once(&message.message_id)
                .chain(&message.references)
                .chain(&message.in_reply_to)
        })
        .filter(|id| !id.is_empty())
        .map(|id| id.trim().trim_matches(['<', '>']).to_owned())
        .collect();
    let mut found = HashSet::new();
    loop {
        let mut changed = false;
        for candidate in candidates {
            let message = &candidate.message;
            let id = message.message_id.trim().trim_matches(['<', '>']);
            if id.is_empty() || received_ids.contains(id) || found.contains(id) {
                continue;
            }
            if linked.contains(id)
                || message
                    .references
                    .iter()
                    .chain(&message.in_reply_to)
                    .any(|id| linked.contains(id.trim().trim_matches(['<', '>'])))
                || received.iter().any(|incoming| {
                    if outgoing {
                        same_conversation(message, incoming)
                            || ((message.is_draft
                                || senders::key(message) == senders::key(incoming))
                                && subject_key(&message.subject) == subject_key(&incoming.subject)
                                && !subject_key(&message.subject).is_empty()
                                && senders::recipients(message).iter().any(|(_, email)| {
                                    senders::recipients(incoming)
                                        .iter()
                                        .any(|(_, other)| email == other)
                                }))
                    } else {
                        same_conversation(incoming, message)
                            || (!senders::key(incoming).is_empty()
                                && senders::key(incoming) == senders::key(message)
                                && !subject_key(&incoming.subject).is_empty()
                                && subject_key(&incoming.subject) == subject_key(&message.subject))
                    }
                })
            {
                found.insert(id.to_owned());
                linked.insert(id.to_owned());
                linked.extend(
                    message
                        .references
                        .iter()
                        .chain(&message.in_reply_to)
                        .map(|id| id.trim().trim_matches(['<', '>']).to_owned()),
                );
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut result = Vec::new();
    for candidate in candidates {
        if found.remove(candidate.message.message_id.trim().trim_matches(['<', '>'])) {
            result.push(candidate.clone());
        }
    }
    result.sort_by_key(|item| std::cmp::Reverse(item.message.timestamp));
    result
}

fn same_conversation(incoming: &Message, sent: &Message) -> bool {
    let subject = subject_key(&incoming.subject);
    if subject.is_empty() || subject != subject_key(&sent.subject) {
        return false;
    }
    let sender = senders::key(incoming);
    mailparse::addrparse(&sent.recipients).is_ok_and(|recipients| {
        recipients.iter().any(|recipient| match recipient {
            mailparse::MailAddr::Single(address) => address.addr.eq_ignore_ascii_case(&sender),
            mailparse::MailAddr::Group(group) => group
                .addrs
                .iter()
                .any(|address| address.addr.eq_ignore_ascii_case(&sender)),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_sent_replies_and_replies_to_sent_by_message_id() {
        let received = vec![Message {
            message_id: "received".into(),
            references: vec!["original-sent".into()],
            ..Default::default()
        }];
        let sent = vec![
            SentMessage {
                folder: String::new(),
                message: Message {
                    message_id: "reply-sent".into(),
                    references: vec!["received".into()],
                    ..Default::default()
                },
            },
            SentMessage {
                folder: "Sent".into(),
                message: Message {
                    message_id: "original-sent".into(),
                    ..Default::default()
                },
            },
            SentMessage {
                folder: "Sent".into(),
                message: Message {
                    message_id: "unrelated".into(),
                    subject: "Same subject".into(),
                    ..Default::default()
                },
            },
        ];
        let result = related(&received, &sent);
        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .any(|item| item.message.message_id == "reply-sent")
        );
        assert!(
            result
                .iter()
                .any(|item| item.message.message_id == "original-sent")
        );
    }

    #[test]
    fn hides_quotes_only_when_the_referenced_message_is_below() {
        let original = Message {
            message_id: "original".into(),
            ..Default::default()
        };
        let reply = Message {
            references: vec!["original".into()],
            ..Default::default()
        };
        assert!(quoted_message_visible(&reply, [&original]));
        assert!(!quoted_message_visible(&reply, std::iter::empty()));
        assert!(!quoted_message_visible(&original, [&reply]));
    }

    #[test]
    fn includes_own_message_with_matching_recipient_and_topic_without_reply_headers() {
        let received = Message {
            message_id: "received".into(),
            sender: "Other <other@example.com>".into(),
            subject: "Re: Plans".into(),
            ..Default::default()
        };
        let sent = SentMessage {
            folder: "Sent".into(),
            message: Message {
                message_id: "sent".into(),
                recipients: "other@example.com".into(),
                subject: "Plans".into(),
                ..Default::default()
            },
        };
        assert_eq!(
            related(&[received.clone()], &[sent.clone()]),
            vec![sent.clone()]
        );
        let unrelated = SentMessage {
            message: Message {
                recipients: "someone@example.com".into(),
                ..sent.message.clone()
            },
            ..sent
        };
        assert!(related(&[received], &[unrelated]).is_empty());
    }
}
