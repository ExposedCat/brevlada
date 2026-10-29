use crate::models::{Message, senders, subject_key};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Term {
    Subject { subject: String, peer: String },
    MessageId(String),
}

impl Term {
    pub(super) fn needs_utf8(&self) -> bool {
        match self {
            Self::Subject { subject, peer } => !subject.is_ascii() || !peer.is_ascii(),
            Self::MessageId(id) => !id.is_ascii(),
        }
    }

    fn encode(&self, literal_limit: usize) -> Option<String> {
        match self {
            Self::Subject { subject, peer } => {
                let peer = value(peer, literal_limit)?;
                let peers = format!("OR FROM {peer} OR TO {peer} CC {peer}");
                Some(match value(subject, literal_limit) {
                    Some(subject) => format!("(SUBJECT {subject} {peers})"),
                    None => format!("({peers})"),
                })
            }
            Self::MessageId(id) => {
                let id = value(id, literal_limit)?;
                Some(format!(
                    "(OR HEADER Message-ID {id} OR HEADER References {id} HEADER In-Reply-To {id})"
                ))
            }
        }
    }
}

pub(super) fn terms<'a>(
    account: &str,
    messages: impl IntoIterator<Item = &'a Message>,
) -> Vec<Term> {
    let mut terms = BTreeSet::new();
    for message in messages {
        let subject = subject_key(&message.subject);
        if valid(&subject) {
            let peers = std::iter::once(senders::key(message)).chain(
                senders::recipients(message)
                    .into_iter()
                    .map(|(_, email)| email),
            );
            for peer in peers.filter(|peer| valid(peer) && !peer.eq_ignore_ascii_case(account)) {
                terms.insert(Term::Subject {
                    subject: subject.clone(),
                    peer,
                });
            }
        }
        for id in std::iter::once(&message.message_id)
            .chain(&message.references)
            .chain(&message.in_reply_to)
        {
            let id = super::parser::normalize_message_id(id);
            if valid(id) {
                terms.insert(Term::MessageId(id.to_owned()));
            }
        }
    }
    terms.into_iter().collect()
}

fn valid(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn value(value: &str, literal_limit: usize) -> Option<String> {
    if !value.is_ascii() && value.len() <= literal_limit {
        return Some(format!("{{{}+}}\r\n{value}", value.len()));
    }
    let value = if value.is_ascii() {
        value
    } else {
        value
            .split(|character: char| !character.is_ascii())
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .max_by_key(|part| part.len())?
    };
    super::special_use::quoted(value).ok()
}

pub(super) fn query(terms: &[Term], literal_limit: usize) -> String {
    fn join(terms: &[String]) -> String {
        match terms.len() {
            0 => String::new(),
            1 => terms[0].clone(),
            length => {
                let (left, right) = terms.split_at(length / 2);
                format!("OR {} {}", join(left), join(right))
            }
        }
    }
    let terms: Vec<_> = terms
        .iter()
        .filter_map(|term| term.encode(literal_limit))
        .collect();
    let joined = join(&terms);
    format!(
        "{}{joined}",
        if joined.is_ascii() {
            ""
        } else {
            "CHARSET UTF-8 "
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_uses_byte_counted_literals_and_preserves_quotes_and_backslashes() {
        let query = query(
            &[Term::Subject {
                subject: "příští \"termín\" \\".into(),
                peer: "salon@example.com".into(),
            }],
            usize::MAX,
        );
        assert!(
            query.starts_with("CHARSET UTF-8 (SUBJECT {22+}\r\npříští \"termín\" \\"),
            "{query:?}"
        );
        assert!(query.contains("FROM \"salon@example.com\""));
    }

    #[test]
    fn servers_without_literals_get_ascii_candidates_and_never_search_all() {
        for subject in ["příští schůzka", "予約"] {
            let query = query(
                &[Term::Subject {
                    subject: subject.into(),
                    peer: "salon@example.com".into(),
                }],
                0,
            );
            assert!(query.is_ascii());
            assert!(query.contains("FROM \"salon@example.com\""));
            assert!(!query.contains("CHARSET") && !query.contains("ALL"));
        }
    }

    #[test]
    fn rejects_control_characters_and_keeps_search_nesting_bounded() {
        let message = Message {
            subject: "bad\r\nALL".into(),
            message_id: "bad\r\nALL".into(),
            sender: "a@example.com".into(),
            ..Default::default()
        };
        assert!(terms("me@example.com", [&message]).is_empty());
        let terms: Vec<_> = (0..16)
            .map(|id| Term::MessageId(format!("id-{id}")))
            .collect();
        let query = query(&terms, 0);
        assert_eq!(query.matches("HEADER Message-ID").count(), 16);
        assert!(!query.starts_with(&"OR ".repeat(5)));
    }
}
