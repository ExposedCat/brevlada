use super::Event;
use crate::models::{Message, senders};

pub fn status_label(status: &str) -> &str {
    match status {
        "ACCEPTED" => "Accepted",
        "TENTATIVE" => "Maybe",
        "DECLINED" => "Declined",
        "DELEGATED" => "Delegated",
        "NEEDS-ACTION" => "Awaiting response",
        _ => status,
    }
}

pub fn snapshot<'a>(source: &Event, messages: impl IntoIterator<Item = &'a Message>) -> Event {
    let mut updates = Vec::new();
    for message in messages {
        for event in super::events(message) {
            if source.same_event(&event) && event.organizer_email == source.organizer_email {
                updates.push((event, message));
            }
        }
    }
    let mut result = updates
        .iter()
        .filter(|(event, _)| event.method.as_deref() != Some("REPLY"))
        .max_by_key(|(event, message)| (event.sequence, event.stamp, message.timestamp))
        .map(|(event, _)| event.clone())
        .unwrap_or_else(|| source.clone());
    let mut replies: Vec<_> = updates
        .iter()
        .filter(|(event, _)| {
            event.method.as_deref() == Some("REPLY")
                && event.sequence == result.sequence
                && event.stamp >= result.stamp
        })
        .collect();
    replies.sort_by_key(|(event, message)| (event.stamp, message.timestamp, message.uid));
    for (reply, message) in replies {
        let sender = senders::key(message);
        for attendee in &reply.attendees {
            if !attendee.email.eq_ignore_ascii_case(&sender) {
                continue;
            }
            if let Some(current) = result
                .attendees
                .iter_mut()
                .find(|current| current.email.eq_ignore_ascii_case(&attendee.email))
            {
                current.status = attendee.status.clone();
            }
        }
    }
    if result.method.as_deref() == Some("CANCEL") {
        result.status = Some("CANCELLED".into());
        result.invitation = false;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::calendar::{Attendee, Response};

    #[test]
    fn correlates_replies_by_uid_instance_sequence_and_sender() {
        let event = Event {
            uid: Some("one".into()),
            name: "Meeting".into(),
            start: "2026-10-02T12:00:00Z".into(),
            organizer_email: Some("host@example.com".into()),
            method: Some("REQUEST".into()),
            sequence: 2,
            stamp: 20,
            attendees: vec![Attendee {
                email: "me@example.com".into(),
                name: None,
                status: "NEEDS-ACTION".into(),
            }],
            ..Default::default()
        };
        let response = |sender: &str, sequence, stamp, status: Response| Message {
            sender: sender.into(),
            timestamp: stamp,
            calendar_events: vec![Event {
                method: Some("REPLY".into()),
                sequence,
                stamp,
                attendees: vec![Attendee {
                    email: "me@example.com".into(),
                    name: None,
                    status: status.status().into(),
                }],
                ..event.clone()
            }],
            ..Default::default()
        };
        let messages = [
            response("me@example.com", 1, 99, Response::Declined),
            response("me@example.com", 2, 21, Response::Accepted),
            response("stranger@example.com", 2, 23, Response::Declined),
            response("me@example.com", 2, 22, Response::Tentative),
        ];
        assert_eq!(snapshot(&event, &messages).attendees[0].status, "TENTATIVE");
        let mut exception = messages[1].clone();
        exception.calendar_events[0].recurrence_id = Some("RECURRENCE-ID:20261003T120000Z".into());
        assert_eq!(
            snapshot(&event, [&exception]).attendees[0].status,
            "NEEDS-ACTION"
        );
    }

    #[test]
    fn keeps_the_roster_and_updates_each_invitees_own_response() {
        let event = Event {
            uid: Some("meeting".into()),
            organizer_email: Some("host@example.com".into()),
            method: Some("REQUEST".into()),
            stamp: 10,
            attendees: ["me@example.com", "alex@example.com", "sam@example.com"]
                .into_iter()
                .map(|email| Attendee {
                    email: email.into(),
                    name: None,
                    status: "NEEDS-ACTION".into(),
                })
                .collect(),
            ..Default::default()
        };
        let reply = |email: &str, status: Response| Message {
            sender: email.into(),
            calendar_events: vec![Event {
                method: Some("REPLY".into()),
                stamp: 11,
                attendees: vec![Attendee {
                    email: email.into(),
                    name: None,
                    status: status.status().into(),
                }],
                ..event.clone()
            }],
            ..Default::default()
        };
        let replies = [
            reply("alex@example.com", Response::Accepted),
            reply("sam@example.com", Response::Declined),
        ];
        let updated = snapshot(&event, &replies);
        assert_eq!(updated.attendees.len(), 3);
        assert_eq!(updated.attendees[0].status, "NEEDS-ACTION");
        assert_eq!(updated.attendees[1].status, "ACCEPTED");
        assert_eq!(updated.attendees[2].status, "DECLINED");
    }
}
