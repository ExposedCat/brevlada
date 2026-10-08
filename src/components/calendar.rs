use crate::{
    models::calendar::{Event, Response},
    theme,
};
use adw::prelude::*;

mod actions;
mod participants;

pub(super) use actions::save;
pub use participants::rows as participants;

pub fn with_participants(card: &gtk::Box, participants: &gtk::Widget) -> gtk::Box {
    let event = super::column("calendar-event");
    event.append(card);
    event.append(participants);
    event
}

fn detail(icon: &str, text: &str) -> gtk::Box {
    let row = super::horizontal("calendar-detail", theme::SMALL_SPACING);
    row.append(&gtk::Image::from_icon_name(icon));
    let label = super::label(text, "");
    label.set_wrap(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::None);
    row.append(&label);
    row
}

pub fn card(
    event: &Event,
    email: &str,
    pending: bool,
    respond: impl Fn(Response) + 'static,
) -> gtk::Box {
    let card = super::column("calendar-card");
    card.set_spacing(theme::SPACING);
    card.append(&super::label(&event.name, "title-2"));
    let date = event.date_label();
    if !date.is_empty() {
        card.append(&detail("x-office-calendar-symbolic", &date));
    }
    if let Some(location) = &event.location {
        card.append(&detail("location-services-active-symbolic", location));
    }
    if let Some(organizer) = &event.organizer {
        card.append(&super::label(
            &format!("Organized by {organizer}"),
            "dim-label",
        ));
    }
    if let Some(status) = event.status.as_deref() {
        let status = match status.rsplit('/').next().unwrap_or(status) {
            "EventCancelled" | "CANCELLED" => Some("Cancelled"),
            "EventPostponed" => Some("Postponed"),
            "EventRescheduled" => Some("Rescheduled"),
            "TENTATIVE" => Some("Tentative"),
            _ => None,
        };
        if let Some(status) = status {
            card.append(&super::label(status, "dim-label"));
        }
    }
    let error = super::label("", "error");
    error.set_wrap(true);
    error.set_ellipsize(gtk::pango::EllipsizeMode::None);
    error.set_visible(false);
    if let Some(response) = actions::responses(event, email, pending, respond) {
        card.append(&response);
    }
    card.append(&actions::save(event, &error));
    card.append(&error);
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::calendar;
    use std::{cell::RefCell, rc::Rc};

    fn buttons(widget: &gtk::Widget, result: &mut Vec<gtk::Button>) {
        if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            result.push(button.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            buttons(&widget, result);
            child = widget.next_sibling();
        }
    }

    fn button_title(button: &gtk::Button) -> String {
        button
            .child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .text()
            .to_string()
    }

    fn labels(widget: &gtk::Widget, result: &mut Vec<String>) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            result.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            labels(&widget, result);
            child = widget.next_sibling();
        }
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect GTK widgets"]
    fn attendee_can_rsvp_but_organizer_sees_attendance_and_save() {
        gtk::init().unwrap();
        adw::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        let theme = gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap());
        theme.add_resource_path("/org/gtk/example/icons");
        assert!(theme.has_icon("location-services-active-symbolic"));
        let event = calendar::parse_ical("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:demo\r\nDTSTART:20261002T151500Z\r\nDTEND:20261002T160000Z\r\nSUMMARY:Demo\r\nLOCATION:Meeting\r\nORGANIZER:mailto:artem13.prokop@gmail.com\r\nATTENDEE;CN=Artem Prokop Dev;PARTSTAT=ACCEPTED:mailto:artem.prokop.dev@gmail.com\r\nATTENDEE;CN=Alex;PARTSTAT=TENTATIVE:mailto:alex@example.com\r\nATTENDEE;PARTSTAT=DECLINED:mailto:sam@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").remove(0);
        let received = Rc::new(RefCell::new(Vec::new()));
        let responses = received.clone();
        let attendee = card(
            &event,
            "artem.prokop.dev@gmail.com",
            false,
            move |response| {
                responses.borrow_mut().push(response);
            },
        );
        let mut actions = Vec::new();
        buttons(attendee.upcast_ref(), &mut actions);
        assert_eq!(
            actions.iter().map(button_title).collect::<Vec<_>>(),
            ["Yes", "Maybe", "No", "Save to Calendar"]
        );
        assert!(actions.iter().all(|button| button.has_css_class("pill")));
        assert!(actions[0].has_css_class("suggested-action"));
        let row = actions[0].parent().unwrap();
        assert_eq!(row.first_child().as_ref(), Some(actions[0].upcast_ref()));
        assert_eq!(actions[3].parent().as_ref(), Some(attendee.upcast_ref()));
        let mut texts = Vec::new();
        labels(attendee.upcast_ref(), &mut texts);
        assert!(texts.iter().any(|text| text == "Your response:"));
        assert!(!texts.iter().any(|text| text == "RSVP"));
        let mut child = attendee.first_child();
        while let Some(widget) = child {
            if widget
                .downcast_ref::<gtk::Label>()
                .is_some_and(|label| label.text().starts_with("Organized by "))
            {
                let response = widget.next_sibling().unwrap();
                assert!(response.has_css_class("calendar-response"));
                assert_eq!(
                    response
                        .first_child()
                        .unwrap()
                        .downcast::<gtk::Label>()
                        .unwrap()
                        .text(),
                    "Your response:"
                );
                assert!(response.last_child().unwrap().is::<gtk::ScrolledWindow>());
                break;
            }
            child = widget.next_sibling();
        }
        for action in &actions[..3] {
            action.emit_clicked();
        }
        assert_eq!(
            *received.borrow(),
            [Response::Accepted, Response::Tentative, Response::Declined]
        );
        let pending = card(
            &event,
            "artem.prokop.dev@gmail.com",
            true,
            |_| unreachable!(),
        );
        actions.clear();
        buttons(pending.upcast_ref(), &mut actions);
        assert!(actions[..3].iter().all(|action| !action.is_sensitive()));
        let organizer = card(
            &event,
            "artem13.prokop@gmail.com",
            false,
            |_| unreachable!(),
        );
        actions.clear();
        buttons(organizer.upcast_ref(), &mut actions);
        assert_eq!(actions.len(), 1);
        assert_eq!(button_title(&actions[0]), "Save to Calendar");
        texts.clear();
        labels(organizer.upcast_ref(), &mut texts);
        assert!(!texts.iter().any(|text| text.starts_with("Your response:")));
    }
}
