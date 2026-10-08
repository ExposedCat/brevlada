mod dates;
mod export;
mod ical;
mod participation;
mod reply;
mod responses;
mod rsvp;
mod scheduling;
mod schema;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Event {
    pub uid: Option<String>,
    pub recurrence_id: Option<String>,
    pub sequence: u32,
    pub stamp: i64,
    pub method: Option<String>,
    pub organizer_email: Option<String>,
    pub attendees: Vec<Attendee>,
    pub name: String,
    pub start: String,
    pub end: Option<String>,
    pub timezone: Option<String>,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub rsvp: Vec<Rsvp>,
    pub invitation: bool,
    pub calendar: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rsvp {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Attendee {
    pub email: String,
    pub name: Option<String>,
    pub status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Accepted,
    Tentative,
    Declined,
}

impl Response {
    pub fn status(self) -> &'static str {
        match self {
            Self::Accepted => "ACCEPTED",
            Self::Tentative => "TENTATIVE",
            Self::Declined => "DECLINED",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::Tentative => "Maybe",
            Self::Declined => "Declined",
        }
    }
}

pub use ical::parse as parse_ical;
pub use reply::reply;
pub use responses::{Participant, compact_reply, participants};
pub use scheduling::{snapshot, status_label};

pub use schema::parse as parse_html;

pub fn is_reply(message: &super::Message) -> bool {
    if message.is_draft {
        return false;
    }
    let events = events(message);
    !events.is_empty()
        && events
            .iter()
            .all(|event| event.method.as_deref() == Some("REPLY"))
}

pub fn events(message: &super::Message) -> Vec<Event> {
    if message.display_prepared {
        return message.calendar_events.clone();
    }
    let mut events = Vec::new();
    for cached in &message.calendar_events {
        let hydrated = cached
            .calendar
            .as_deref()
            .and_then(|calendar| {
                parse_ical(calendar)
                    .into_iter()
                    .find(|event| event.same_event(cached))
                    .or_else(|| parse_ical(calendar).into_iter().next())
            })
            .unwrap_or_else(|| cached.clone());
        if !events.iter().any(|old: &Event| old.same_event(&hydrated)) {
            events.push(hydrated);
        }
    }
    for event in parse_html(&message.body_html) {
        if let Some(old) = events.iter_mut().find(|old| old.same_event(&event)) {
            for action in event.rsvp {
                if !old.rsvp.contains(&action) {
                    old.rsvp.push(action);
                }
            }
        } else {
            events.push(event);
        }
    }
    events
}

impl Event {
    pub fn date_label(&self) -> String {
        dates::label(self)
    }

    pub fn same_event(&self, other: &Self) -> bool {
        match (&self.uid, &other.uid) {
            (Some(left), Some(right)) => left == right && self.recurrence_id == other.recurrence_id,
            _ => {
                self.name == other.name
                    && (self.start == other.start
                        || match (
                            chrono::DateTime::parse_from_rfc3339(&self.start),
                            chrono::DateTime::parse_from_rfc3339(&other.start),
                        ) {
                            (Ok(left), Ok(right)) => left == right,
                            _ => false,
                        })
            }
        }
    }

    pub fn can_reply(&self, email: &str) -> bool {
        self.invitation
            && self.uid.is_some()
            && self.organizer_email.is_some()
            && self.status.as_deref() != Some("CANCELLED")
            && self
                .attendees
                .iter()
                .any(|attendee| attendee.email.eq_ignore_ascii_case(email))
    }

    pub fn to_ical(&self) -> Option<String> {
        export::serialize(self)
    }
}
