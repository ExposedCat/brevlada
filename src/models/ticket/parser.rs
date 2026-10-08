use super::{Kind, Reservation, Ticket, fields::*};
use scraper::{Html, Selector};
use serde_json::Value;

pub fn parse(html: &str) -> Vec<Reservation> {
    if !html.contains("Reservation") && !html.contains("Ticket") {
        return Vec::new();
    }
    let document = Html::parse_fragment(html);
    let mut reservations = Vec::new();
    for script in document.select(&Selector::parse("script[type='application/ld+json']").unwrap()) {
        if let Ok(value) = serde_json::from_str::<Value>(&script.text().collect::<String>()) {
            collect(&value, &mut reservations);
        }
    }
    for element in document.select(&Selector::parse("[itemscope][itemtype]").unwrap()) {
        if element
            .ancestors()
            .filter_map(scraper::ElementRef::wrap)
            .any(|parent| {
                parent.value().attr("itemtype").is_some_and(|types| {
                    types.split_whitespace().any(|value| {
                        matches!(
                            term(value),
                            "EventReservation"
                                | "BusReservation"
                                | "TrainReservation"
                                | "FlightReservation"
                                | "Ticket"
                        )
                    })
                })
            })
        {
            continue;
        }
        collect(
            &crate::models::schema::microdata(&element),
            &mut reservations,
        );
    }
    reservations
}

fn tickets(value: &Value, holder: &Option<String>) -> Vec<Ticket> {
    if let Some(values) = value.as_array() {
        return values
            .iter()
            .flat_map(|value| tickets(value, holder))
            .collect();
    }
    if !value.is_object() {
        return Vec::new();
    }
    let ticket = Ticket {
        number: text(&value["ticketNumber"]),
        token: text(&value["ticketToken"]),
        holder: named(&value["underName"]).or(holder.clone()),
        seat: seat(&value["ticketedSeat"]),
    };
    if ticket.number.is_none() && ticket.token.is_none() && ticket.seat.is_none() {
        return Vec::new();
    }
    vec![ticket]
}

fn reservation(value: &Value) -> Option<Reservation> {
    let kind = [
        ("BusReservation", Kind::Bus),
        ("TrainReservation", Kind::Train),
        ("FlightReservation", Kind::Flight),
        ("EventReservation", Kind::Event),
        ("Ticket", Kind::Ticket),
    ]
    .into_iter()
    .find(|(name, _)| kind(value, name))?
    .1;
    let holder = named(&value["underName"]);
    let tickets = tickets(
        if kind == Kind::Ticket {
            value
        } else {
            &value["reservedTicket"]
        },
        &holder,
    );
    if matches!(kind, Kind::Event | Kind::Ticket) && tickets.is_empty() {
        return None;
    }
    let source = if kind == Kind::Ticket {
        value
    } else {
        &value["reservationFor"]
    };
    let (departure_key, arrival_key, provider_key, service_key) = match kind {
        Kind::Bus => (
            "departureBusStop",
            "arrivalBusStop",
            "busCompany",
            "busNumber",
        ),
        Kind::Train => (
            "departureStation",
            "arrivalStation",
            "trainCompany",
            "trainNumber",
        ),
        Kind::Flight => (
            "departureAirport",
            "arrivalAirport",
            "airline",
            "flightNumber",
        ),
        _ => ("", "", "organizer", ""),
    };
    let departure = place(&source[departure_key]);
    let arrival = place(&source[arrival_key]);
    let name = match (&departure, &arrival) {
        (Some(from), Some(to)) => format!(
            "{} → {}",
            named(&source[departure_key]).unwrap_or(from.clone()),
            named(&source[arrival_key]).unwrap_or(to.clone())
        ),
        _ => named(source)
            .or_else(|| text(&value["name"]))
            .unwrap_or_else(|| {
                match kind {
                    Kind::Bus => "Bus journey",
                    Kind::Train => "Train journey",
                    Kind::Flight => "Flight",
                    Kind::Event => "Event ticket",
                    Kind::Ticket => "Ticket",
                }
                .into()
            }),
    };
    let start = text(&source["departureTime"])
        .or_else(|| text(&source["startDate"]))
        .map(super::dates::normalize);
    if start.is_none() && tickets.is_empty() {
        return None;
    }
    let mut link = url(&value["url"]).or_else(|| url(&source["url"]));
    if kind == Kind::Flight {
        link = link.or_else(|| url(&value["checkinUrl"]));
    }
    Some(Reservation {
        kind,
        number: text(&value["reservationId"]).or_else(|| text(&value["reservationNumber"])),
        status: text(&value["reservationStatus"]).map(|value| term(&value).to_owned()),
        name,
        start,
        end: text(&source["arrivalTime"])
            .or_else(|| text(&source["endDate"]))
            .map(super::dates::normalize),
        departure,
        arrival,
        location: place(&source["location"]),
        provider: named(&source[provider_key]).or_else(|| named(&value["provider"])),
        service: text(&source[service_key]),
        url: link,
        holder,
        tickets,
    })
}

fn collect(value: &Value, reservations: &mut Vec<Reservation>) {
    if let Some(reservation) = reservation(value) {
        if let Some(old) = reservations
            .iter_mut()
            .find(|old| old.same_reservation(&reservation))
        {
            old.merge(&reservation);
        } else {
            reservations.push(reservation);
        }
    } else {
        match value {
            Value::Array(values) => values.iter().for_each(|value| collect(value, reservations)),
            Value::Object(map) => map.values().for_each(|value| collect(value, reservations)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn html(value: Value) -> String {
        format!("<script type='application/ld+json'>{value}</script>")
    }

    fn bus(holder: &str, number: &str, from: &str, to: &str) -> Value {
        json!({
            "@type": "BusReservation", "reservationNumber": "booking-one",
            "reservationStatus": "http://schema.org/ReservationConfirmed",
            "reservationFor": {
                "@type": "BusTrip", "busNumber": "100", "busCompany": {"name": "Coach Co"},
                "departureBusStop": {"name": from}, "arrivalBusStop": {"name": to},
                "departureTime": "2026-10-08T13:50:00+02:00",
                "arrivalTime": "2026-10-08T16:30:00+02:00"
            },
            "reservedTicket": {"@type": "Ticket", "ticketNumber": number,
                "ticketToken": format!("https://example.com/ticket/{number}"),
                "underName": {"name": holder}, "ticketedSeat": {"seatNumber": "12A"}}
        })
    }

    #[test]
    fn groups_passengers_without_combining_separate_journeys() {
        let reservations = parse(&html(json!({"@graph": [
            bus("Alex", "one", "Prague", "Dresden"),
            bus("Sam", "two", "Prague", "Dresden"),
            bus("Alex", "three", "Dresden", "Prague")
        ]})));
        assert_eq!(reservations.len(), 2);
        assert_eq!(reservations[0].tickets.len(), 2);
        assert_eq!(reservations[0].tickets[0].holder.as_deref(), Some("Alex"));
        assert_eq!(reservations[0].tickets[0].seat.as_deref(), Some("Seat 12A"));
        assert_eq!(reservations[0].status_label(), Some("Confirmed"));
        let event = reservations[0].calendar_event().unwrap();
        let exported = event.to_ical().unwrap();
        let events = crate::models::calendar::parse_ical(&exported);
        assert_eq!(events[0].name, "Prague → Dresden");
        assert_eq!(events[0].start, "2026-10-08T11:50:00Z");
        assert!(
            events[0]
                .description
                .as_deref()
                .unwrap()
                .contains("Alex · Seat 12A")
        );
    }

    #[test]
    fn reads_flight_microdata_without_a_boarding_pass() {
        let reservations = parse(
            r#"<div itemscope itemtype="https://schema.org/FlightReservation">
            <meta itemprop="reservationNumber" content="flight-one">
            <link itemprop="checkinUrl" href="https://example.com/check-in">
            <div itemprop="underName" itemscope itemtype="https://schema.org/Person"><meta itemprop="name" content="Alex"></div>
            <div itemprop="reservationFor" itemscope itemtype="https://schema.org/Flight">
                <meta itemprop="flightNumber" content="FR123">
                <meta itemprop="departureTime" content="2026-10-08T13:50:00+02:00">
                <meta itemprop="arrivalTime" content="2026-10-08T15:00:00+01:00">
                <div itemprop="departureAirport" itemscope itemtype="https://schema.org/Airport"><meta itemprop="name" content="Prague"><meta itemprop="iataCode" content="PRG"></div>
                <div itemprop="arrivalAirport" itemscope itemtype="https://schema.org/Airport"><meta itemprop="name" content="London"><meta itemprop="iataCode" content="STN"></div>
            </div></div>"#,
        );
        assert_eq!(reservations.len(), 1);
        let reservation = &reservations[0];
        assert_eq!(reservation.kind, Kind::Flight);
        assert_eq!(reservation.name, "Prague → London");
        assert_eq!(reservation.departure.as_deref(), Some("Prague (PRG)"));
        assert_eq!(reservation.holder.as_deref(), Some("Alex"));
        assert_eq!(
            reservation.url.as_deref(),
            Some("https://example.com/check-in")
        );
        assert!(reservation.tickets.is_empty());
    }

    #[test]
    fn event_tickets_accept_arrays_and_leave_appointments_to_calendar() {
        let mut event = json!({
            "@type": ["https://schema.org/EventReservation"], "reservationId": "event-one",
            "underName": {"name": "Alex"},
            "reservationFor": {"@type": "MusicEvent", "name": "Concert", "startDate": "2026-10-09T20:00:00+02:00", "location": {"name": "Hall"}},
            "reservedTicket": [
                {"@type": "Ticket", "ticketNumber": 123, "ticketToken": "qrCode:abcd", "ticketedSeat": {"seatSection": "A", "seatRow": 4, "seatNumber": 12}},
                {"@type": "Ticket", "ticketNumber": 124}
            ]
        });
        let reservations = parse(&html(event.clone()));
        assert_eq!(reservations.len(), 1);
        assert_eq!(reservations[0].tickets.len(), 2);
        assert_eq!(reservations[0].tickets[0].number.as_deref(), Some("123"));
        assert_eq!(
            reservations[0].tickets[0].seat.as_deref(),
            Some("Section A · Row 4 · Seat 12")
        );
        assert_eq!(reservations[0].tickets[1].holder.as_deref(), Some("Alex"));
        event.as_object_mut().unwrap().remove("reservedTicket");
        let html = html(event);
        assert!(parse(&html).is_empty());
        assert_eq!(crate::models::calendar::parse_html(&html).len(), 1);
    }

    #[test]
    fn newer_invoice_keeps_seats_and_cancellation_status() {
        let mut original = bus("Alex", "one", "Prague", "Dresden");
        let mut invoice = original.clone();
        invoice["reservedTicket"]
            .as_object_mut()
            .unwrap()
            .remove("ticketedSeat");
        invoice["reservationStatus"] = json!("https://schema.org/ReservationCancelled");
        let reservations = parse(&html(json!([original.clone(), invoice])));
        assert_eq!(reservations.len(), 1);
        assert_eq!(reservations[0].tickets.len(), 1);
        assert_eq!(reservations[0].tickets[0].seat.as_deref(), Some("Seat 12A"));
        assert_eq!(reservations[0].status_label(), Some("Cancelled"));
        assert_eq!(
            reservations[0].calendar_event().unwrap().status.as_deref(),
            Some("CANCELLED")
        );
        original["url"] = json!("javascript:alert(1)");
        assert!(parse(&html(original))[0].url.is_none());
        assert!(!web_url("file:///tmp/pass"));
        assert!(!web_url("https:"));
        assert!(parse("<script type='application/ld+json'>{broken}</script>").is_empty());
    }
}
