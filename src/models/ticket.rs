mod dates;
mod fields;
mod parser;

use super::calendar::Event;
use serde::{Deserialize, Serialize};

pub use dates::date_label;
pub use dates::{day_label, time_label};
pub use fields::web_url;
pub use parser::parse;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Event,
    Bus,
    Train,
    Flight,
    Ticket,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reservation {
    pub kind: Kind,
    pub number: Option<String>,
    pub status: Option<String>,
    pub name: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub departure: Option<String>,
    pub arrival: Option<String>,
    pub location: Option<String>,
    pub provider: Option<String>,
    pub service: Option<String>,
    pub url: Option<String>,
    pub holder: Option<String>,
    pub tickets: Vec<Ticket>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ticket {
    pub number: Option<String>,
    pub holder: Option<String>,
    pub seat: Option<String>,
    pub token: Option<String>,
}

impl Reservation {
    pub fn same_reservation(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.name == other.name
            && self.start == other.start
            && self.departure == other.departure
            && self.arrival == other.arrival
            && self.location == other.location
            && match (&self.number, &other.number) {
                (Some(a), Some(b)) => a == b,
                (None, None) => self
                    .tickets
                    .iter()
                    .any(|ticket| other.tickets.iter().any(|other| ticket.same_ticket(other))),
                _ => false,
            }
    }

    pub fn merge(&mut self, newer: &Self) {
        for ticket in &newer.tickets {
            if let Some(old) = self.tickets.iter_mut().find(|old| old.same_ticket(ticket)) {
                old.holder = ticket.holder.clone().or(old.holder.take());
                old.seat = ticket.seat.clone().or(old.seat.take());
                old.token = ticket.token.clone().or(old.token.take());
            } else {
                self.tickets.push(ticket.clone());
            }
        }
        self.status = newer.status.clone().or(self.status.take());
        self.end = newer.end.clone().or(self.end.take());
        self.provider = newer.provider.clone().or(self.provider.take());
        self.service = newer.service.clone().or(self.service.take());
        self.url = newer.url.clone().or(self.url.take());
        self.holder = newer.holder.clone().or(self.holder.take());
    }

    pub fn calendar_event(&self) -> Option<Event> {
        let start = self.start.clone()?;
        let location = self.location.clone().or(self.departure.clone());
        let mut description = Vec::new();
        if let Some(number) = &self.number {
            description.push(format!("Booking: {number}"));
        }
        if let Some(arrival) = &self.arrival {
            description.push(format!("Arrival: {arrival}"));
        }
        for ticket in &self.tickets {
            let text = [ticket.holder.clone(), ticket.seat.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            if !text.is_empty() {
                description.push(text);
            }
        }
        Some(Event {
            name: self.name.clone(),
            start,
            end: self.end.clone(),
            location,
            organizer: self.provider.clone(),
            description: (!description.is_empty()).then(|| description.join("\n")),
            url: self.url.clone(),
            status: (self.status.as_deref() == Some("ReservationCancelled"))
                .then(|| "CANCELLED".into()),
            ..Default::default()
        })
    }

    pub fn status_label(&self) -> Option<&str> {
        match self.status.as_deref()? {
            "ReservationConfirmed" => Some("Confirmed"),
            "ReservationCancelled" => Some("Cancelled"),
            "ReservationPending" => Some("Pending"),
            "ReservationHold" => Some("On hold"),
            _ => None,
        }
    }
}

impl Ticket {
    fn same_ticket(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        if let (Some(a), Some(b)) = (&self.number, &other.number) {
            return a == b && self.holder == other.holder;
        }
        self.token.is_some() && self.token == other.token && self.holder == other.holder
    }
}
