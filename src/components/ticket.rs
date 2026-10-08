use crate::{
    models::ticket::{self, Kind, Reservation},
    theme,
};
use adw::prelude::*;

mod passenger;
mod timeline;

fn text(value: &str, class: &str) -> gtk::Label {
    let label = super::label(value, class);
    label.set_halign(gtk::Align::Fill);
    label.set_wrap(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::None);
    label
}

fn detail(icon: &str, title: &str, value: &str) -> gtk::Box {
    let row = super::horizontal("ticket-detail", theme::SMALL_SPACING);
    let icon = gtk::Image::from_icon_name(icon);
    icon.set_valign(gtk::Align::Start);
    row.append(&icon);
    let content = super::column("ticket-detail-content");
    content.append(&text(title, "dim-label"));
    content.append(&text(value, ""));
    row.append(&content);
    row
}

fn link(url: &str, title: &str) -> gtk::Button {
    let button = super::action_button("web-browser-symbolic", title);
    let url = url.to_owned();
    button.connect_clicked(move |_| super::links::open(&url));
    button
}

pub fn card(reservation: &Reservation) -> gtk::Box {
    let card = super::column("ticket-card");
    card.set_spacing(theme::SPACING);
    let header = super::horizontal("ticket-header", theme::SMALL_SPACING);
    let (icon, title) = match reservation.kind {
        Kind::Bus => ("ticket-symbolic", "Bus tickets"),
        Kind::Train => ("ticket-symbolic", "Train tickets"),
        Kind::Flight => ("airplane-mode-symbolic", "Flight reservation"),
        Kind::Event => ("ticket-symbolic", "Event tickets"),
        Kind::Ticket => ("ticket-symbolic", "Tickets"),
    };
    header.append(&gtk::Image::from_icon_name(icon));
    header.append(&text(title, "dim-label"));
    if let Some(status) = reservation.status_label() {
        let status_label = text(status, "ticket-status");
        status_label.set_hexpand(false);
        status_label.add_css_class(match status {
            "Confirmed" => "ticket-confirmed",
            "Cancelled" => "ticket-cancelled",
            _ => "ticket-pending",
        });
        header.append(&status_label);
    }
    card.append(&header);
    card.append(&text(&reservation.name, "title-2"));
    let provider = [
        reservation.provider.clone(),
        reservation.service.clone(),
        reservation
            .number
            .as_ref()
            .map(|number| format!("Booking {number}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    if !provider.is_empty() {
        let provider = text(&provider, "dim-label");
        provider.set_selectable(true);
        card.append(&provider);
    }
    if matches!(reservation.kind, Kind::Bus | Kind::Train | Kind::Flight) {
        card.append(&timeline::view(reservation));
    }
    if matches!(reservation.kind, Kind::Event | Kind::Ticket) {
        if let Some(start) = &reservation.start {
            let range = [
                Some(ticket::date_label(start)),
                reservation.end.as_deref().map(ticket::date_label),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" – ");
            card.append(&detail("x-office-calendar-symbolic", "When", &range));
        }
        if let Some(location) = &reservation.location {
            card.append(&detail(
                "location-services-active-symbolic",
                "Venue",
                location,
            ));
        }
    }
    if !reservation.tickets.is_empty() {
        let people = super::attachments::grid();
        people.add_css_class("ticket-passengers");
        for (index, ticket) in reservation.tickets.iter().enumerate() {
            people.append(&passenger::card(
                ticket,
                reservation.holder.as_deref(),
                index,
            ));
        }
        card.append(&people);
    } else if let Some(holder) = &reservation.holder {
        card.append(&detail("avatar-default-symbolic", "Passenger", holder));
    }
    if let Some(url) = reservation
        .url
        .as_deref()
        .filter(|url| ticket::web_url(url))
    {
        card.append(&link(url, "View Booking"));
    }
    if let Some(event) = reservation.calendar_event() {
        let error = super::error_label();
        card.append(&super::calendar::save(&event, &error));
        card.append(&error);
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ticket::Ticket;
    use std::{cell::RefCell, rc::Rc};

    fn layout() {
        let context = gtk::glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_millis(150);
        while std::time::Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn inspect(widget: &gtk::Widget, labels: &mut Vec<String>, buttons: &mut Vec<gtk::Button>) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            labels.push(label.text().to_string());
            assert!(!label.uses_markup());
        }
        if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            buttons.push(button.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            inspect(&widget, labels, buttons);
            child = widget.next_sibling();
        }
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect GTK widgets"]
    fn ticket_card_keeps_passenger_actions_and_wraps_at_narrow_widths() {
        gtk::init().unwrap();
        adw::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        let display = gtk::gdk::Display::default().unwrap();
        let icons = gtk::IconTheme::for_display(&display);
        icons.add_resource_path("/org/gtk/example/icons");
        assert!(icons.has_icon("ticket-symbolic"));
        assert!(icons.has_icon("person-symbolic"));
        let provider = gtk::CssProvider::new();
        let errors = Rc::new(RefCell::new(Vec::new()));
        let reported = errors.clone();
        provider.connect_parsing_error(move |_, _, error| {
            reported.borrow_mut().push(error.to_string())
        });
        provider.load_from_string(theme::CSS);
        assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let reservation = Reservation {
            kind: Kind::Bus,
            name: "Prague → Dresden".into(),
            number: Some("booking-one".into()),
            status: Some("ReservationConfirmed".into()),
            provider: Some("Coach Co".into()),
            service: Some("100".into()),
            departure: Some("Prague central bus station".into()),
            arrival: Some("Dresden central bus station".into()),
            start: Some("2026-10-08T13:00:00+02:00".into()),
            end: Some("2026-10-08T15:00:00+02:00".into()),
            tickets: vec![
                Ticket {
                    holder: Some("Alex <Smith>".into()),
                    number: Some("one".into()),
                    seat: Some("Seat 12A".into()),
                    token: Some("https://example.com/pass".into()),
                },
                Ticket {
                    holder: Some("Sam".into()),
                    number: Some("two".into()),
                    seat: Some("Seat 12B".into()),
                    token: Some("qrCode:abcd".into()),
                },
                Ticket {
                    holder: Some("Lee".into()),
                    number: Some("three".into()),
                    token: Some("javascript:alert(1)".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let card = card(&reservation);
        assert!(card.has_css_class("ticket-card"));
        let mut labels = Vec::new();
        let mut buttons = Vec::new();
        inspect(card.upcast_ref(), &mut labels, &mut buttons);
        for expected in [
            "Alex <Smith>",
            "Sam",
            "Lee",
            "Confirmed",
            "Seat 12A",
            "Seat 12B",
            "Coach Co · 100 · Booking booking-one",
            "8 Oct 2026",
            "13:00 · Prague central bus station",
            "15:00 · Dresden central bus station",
            "Save to Calendar",
        ] {
            assert!(
                labels.iter().any(|label| label == expected),
                "Missing {expected}"
            );
        }
        assert!(
            !labels
                .iter()
                .any(|label| matches!(label.as_str(), "Departure" | "Arrival"))
        );
        assert_eq!(
            buttons
                .iter()
                .filter(|button| button.has_css_class("ticket-open"))
                .count(),
            3
        );
        assert_eq!(
            buttons
                .iter()
                .filter(|button| button.tooltip_text().as_deref() == Some("Copy ticket ID"))
                .count(),
            3
        );
        assert_eq!(
            buttons
                .iter()
                .filter(|button| button.has_css_class("ticket-stop"))
                .count(),
            2
        );
        assert!(card.measure(gtk::Orientation::Horizontal, -1).0 <= 320);
        assert!(card.measure(gtk::Orientation::Vertical, 320).0 > 0);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&card)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        let window = gtk::Window::builder()
            .title("Ticket card diagnostic")
            .default_width(800)
            .child(&scroll)
            .build();
        window.present();
        layout();
        let mut child = card.first_child();
        let grid = loop {
            let widget = child.unwrap();
            child = widget.next_sibling();
            if let Ok(grid) = widget.downcast::<gtk::FlowBox>() {
                break grid;
            }
        };
        let first = grid.child_at_index(0).unwrap();
        let second = grid.child_at_index(1).unwrap();
        assert_eq!(
            first.compute_bounds(&grid).unwrap().y(),
            second.compute_bounds(&grid).unwrap().y()
        );
        let copies: Vec<_> = buttons
            .iter()
            .filter(|button| button.tooltip_text().as_deref() == Some("Copy ticket ID"))
            .collect();
        GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
        layout();
        let tile = first.child().unwrap();
        assert!(!tile.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN));
        buttons
            .iter()
            .find(|button| button.has_css_class("ticket-open"))
            .unwrap()
            .grab_focus();
        layout();
        assert!(tile.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN));
        copies[0].emit_clicked();
        let copied = gtk::glib::MainContext::default()
            .block_on(display.clipboard().read_text_future())
            .unwrap()
            .unwrap();
        assert_eq!(copied.as_str(), "one");
        let departure = buttons
            .iter()
            .find(|button| button.tooltip_text().as_deref() == Some("Show Departure location"))
            .unwrap();
        departure.emit_clicked();
        layout();
        let mut popup_labels = Vec::new();
        inspect(departure.upcast_ref(), &mut popup_labels, &mut Vec::new());
        assert!(
            popup_labels
                .iter()
                .any(|value| value.contains("Prague central bus station"))
        );
        let popover = departure
            .last_child()
            .unwrap()
            .downcast::<gtk::Popover>()
            .unwrap();
        popover.popdown();
        layout();
        window.set_default_size(320, 0);
        layout();
        assert!(
            second.compute_bounds(&grid).unwrap().y() > first.compute_bounds(&grid).unwrap().y()
        );
        let timeline = departure.ancestor(gtk::Grid::static_type()).unwrap();
        let connector = timeline
            .downcast_ref::<gtk::Grid>()
            .unwrap()
            .child_at(0, 1)
            .unwrap();
        assert!((1..=3).contains(&connector.width()));
        assert!(connector.height() >= theme::SMALL_SPACING);
        let mut previous_bottom = 0.0;
        for stop in buttons
            .iter()
            .filter(|button| button.has_css_class("ticket-stop"))
        {
            let bounds = stop.compute_bounds(&timeline).unwrap();
            assert!(bounds.x() >= 0.0);
            assert!(bounds.x() + bounds.width() <= timeline.width() as f32);
            assert!(bounds.y() + bounds.height() <= timeline.height() as f32);
            assert!(bounds.y() >= previous_bottom);
            previous_bottom = bounds.y() + bounds.height();
        }
        if let Ok(path) = std::env::var("BREVLADA_TICKET_SCREENSHOT") {
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(path)
                .unwrap();
        }
        window.close();
        gtk::style_context_remove_provider_for_display(&display, &provider);
    }
}
