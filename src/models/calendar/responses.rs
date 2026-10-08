use super::{Attendee, Event};
use crate::models::{Message, attachment, senders};

pub struct Participant {
    pub attendee: Attendee,
    pub responded_at: Option<i64>,
    pub unread: bool,
}

impl Participant {
    pub fn response_label(&self) -> String {
        let status = super::status_label(&self.attendee.status);
        self.responded_at
            .and_then(|stamp| chrono::DateTime::from_timestamp(stamp, 0))
            .map(|date| {
                format!(
                    "{status} · {}",
                    date.with_timezone(&chrono::Local).format("%-d %b %Y %H:%M")
                )
            })
            .unwrap_or_else(|| status.to_owned())
    }
}

fn matches(source: &Event, reply: &Event, message: &Message) -> bool {
    source.uid.is_some()
        && source.same_event(reply)
        && source.organizer_email.is_some()
        && source.organizer_email == reply.organizer_email
        && reply.method.as_deref() == Some("REPLY")
        && reply.attendees.iter().any(|attendee| {
            attendee.email.eq_ignore_ascii_case(&senders::key(message))
                && source
                    .attendees
                    .iter()
                    .any(|invited| invited.email.eq_ignore_ascii_case(&attendee.email))
        })
}

pub fn participants<'a>(
    event: &Event,
    messages: impl IntoIterator<Item = &'a Message>,
) -> Vec<Participant> {
    let mut result: Vec<_> = event
        .attendees
        .iter()
        .cloned()
        .map(|attendee| Participant {
            attendee,
            responded_at: None,
            unread: false,
        })
        .collect();
    let mut replies = Vec::new();
    for message in messages {
        for reply in super::events(message) {
            if matches(event, &reply, message)
                && reply.sequence == event.sequence
                && reply.stamp >= event.stamp
            {
                replies.push((reply, message));
            }
        }
    }
    replies.sort_by_key(|(reply, message)| {
        std::cmp::Reverse((reply.sequence, reply.stamp, message.timestamp, message.uid))
    });
    for (reply, message) in replies {
        if let Some(person) = result.iter_mut().find(|person| {
            person
                .attendee
                .email
                .eq_ignore_ascii_case(&senders::key(message))
        }) && person.responded_at.is_none()
        {
            if person.attendee.name.is_none() {
                let (name, _) = senders::identity(message);
                if !name.is_empty() {
                    person.attendee.name = Some(name);
                }
            }
            person.responded_at = Some(if reply.stamp > 0 {
                reply.stamp
            } else {
                message.timestamp
            });
            person.unread = !message.is_read;
        }
    }
    result
}

pub fn compact_reply(message: &Message, invitations: &[Event]) -> bool {
    if message.is_draft
        || !message.parcels.is_empty()
        || message
            .attachments
            .iter()
            .enumerate()
            .any(|(index, _)| !attachment::rendered_calendar(message, index))
    {
        return false;
    }
    let replies = super::events(message);
    !replies.is_empty()
        && replies.iter().all(|reply| {
            invitations
                .iter()
                .any(|source| matches(source, reply, message))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connects_genuine_replies_and_preserves_other_mail() {
        let event = Event {
            uid: Some("meeting".into()),
            method: Some("REQUEST".into()),
            organizer_email: Some("host@example.com".into()),
            attendees: vec![Attendee {
                email: "me@example.com".into(),
                name: None,
                status: "TENTATIVE".into(),
            }],
            ..Default::default()
        };
        let reply = |stamp| Message {
            uid: stamp as u32,
            message_id: format!("reply-{stamp}"),
            sender: "Me <me@example.com>".into(),
            body_loaded: true,
            calendar_events: vec![Event {
                method: Some("REPLY".into()),
                stamp,
                ..event.clone()
            }],
            ..Default::default()
        };
        let old = reply(1);
        let current = reply(2);
        let people = participants(&event, [&old, &current, &current]);
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].attendee.status, "TENTATIVE");
        assert_eq!(people[0].attendee.name.as_deref(), Some("Me"));
        assert_eq!(people[0].responded_at, Some(2));
        assert!(compact_reply(&current, &[event.clone()]));
        let mut header = current.clone();
        header.body_loaded = false;
        assert!(compact_reply(&header, &[event.clone()]));
        header.calendar_events.clear();
        assert!(!compact_reply(&header, &[event.clone()]));
        assert!(!compact_reply(&current, &[]));
        let mut combined = current.clone();
        let another = Event {
            uid: Some("another-meeting".into()),
            ..event.clone()
        };
        combined.calendar_events.push(Event {
            method: Some("REPLY".into()),
            ..another.clone()
        });
        assert!(!compact_reply(&combined, &[event.clone()]));
        assert!(compact_reply(&combined, &[event.clone(), another]));
        combined.calendar_events[1].method = Some("REQUEST".into());
        assert!(!compact_reply(&combined, &[event.clone()]));
        let mut other = current.clone();
        other.sender = "stranger@example.com".into();
        assert!(!compact_reply(&other, &[event.clone()]));
        other = current.clone();
        other.attachments.push("notes.pdf".into());
        assert!(!compact_reply(&other, &[event.clone()]));
        other = current;
        other.calendar_events[0].recurrence_id = Some("RECURRENCE-ID:20261003".into());
        assert!(!compact_reply(&other, &[event]));
    }

    #[test]
    fn dates_only_the_current_response_and_ignores_stale_and_forged_replies() {
        let source = Event {
            uid: Some("meeting".into()),
            organizer_email: Some("host@example.com".into()),
            stamp: 10,
            sequence: 2,
            method: Some("REQUEST".into()),
            attendees: vec![Attendee {
                email: "me@example.com".into(),
                name: None,
                status: "NEEDS-ACTION".into(),
            }],
            ..Default::default()
        };
        let reply = |stamp, sequence, sender: &str, status: &str| Message {
            sender: sender.into(),
            calendar_events: vec![Event {
                method: Some("REPLY".into()),
                stamp,
                sequence,
                attendees: vec![Attendee {
                    status: status.into(),
                    ..source.attendees[0].clone()
                }],
                ..source.clone()
            }],
            ..Default::default()
        };
        let replies = [
            reply(20, 1, "me@example.com", "ACCEPTED"),
            reply(9, 2, "me@example.com", "ACCEPTED"),
            reply(12, 2, "me@example.com", "TENTATIVE"),
            reply(13, 2, "stranger@example.com", "DECLINED"),
        ];
        let current = super::super::snapshot(&source, &replies);
        let people = participants(&current, &replies);
        assert_eq!(people[0].attendee.status, "TENTATIVE");
        assert_eq!(people[0].responded_at, Some(12));
        assert_eq!(
            participants(&source, [&replies[0], &replies[1], &replies[3]])[0].responded_at,
            None
        );
    }
}
