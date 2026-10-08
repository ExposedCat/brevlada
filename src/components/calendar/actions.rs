use crate::{
    models::calendar::{Event, Response},
    theme,
};
use adw::prelude::*;

pub(super) fn responses(
    event: &Event,
    email: &str,
    pending: bool,
    respond: impl Fn(Response) + 'static,
) -> Option<gtk::Widget> {
    let current = event
        .attendees
        .iter()
        .find(|attendee| attendee.email.eq_ignore_ascii_case(email));
    if !event.can_reply(email) && event.rsvp.is_empty() {
        return None;
    }
    let section = super::super::column("calendar-response");
    section.set_spacing(theme::SMALL_SPACING);
    section.append(&super::super::label("Your response:", "dim-label"));
    let row = super::super::horizontal("calendar-response-actions", theme::SMALL_SPACING);
    if event.can_reply(email) {
        let respond = std::rc::Rc::new(respond);
        for (label, icon, response) in [
            ("Yes", "object-select-symbolic", Response::Accepted),
            ("Maybe", "dialog-question-symbolic", Response::Tentative),
            ("No", "window-close-symbolic", Response::Declined),
        ] {
            let button = super::super::action_button(icon, label);
            button.set_sensitive(!pending);
            button.set_tooltip_text(Some(&format!(
                "Send RSVP to {}",
                event.organizer_email.as_deref().unwrap_or_default()
            )));
            if current.is_some_and(|attendee| attendee.status == response.status()) {
                button.add_css_class("suggested-action");
            }
            let respond = respond.clone();
            button.connect_clicked(move |_| respond(response));
            row.append(&button);
        }
    } else {
        for action in &event.rsvp {
            let button = super::super::action_button("web-browser-symbolic", &action.label);
            button.set_tooltip_text(Some("Respond on the organizer’s website"));
            let url = action.url.clone();
            button.connect_clicked(move |_| super::super::links::open(&url));
            row.append(&button);
        }
    }
    section.append(
        &gtk::ScrolledWindow::builder()
            .child(&row)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    );
    Some(section.upcast())
}

pub(in crate::components) fn save(event: &Event, error: &gtk::Label) -> gtk::Button {
    let button = super::super::action_button("x-office-calendar-symbolic", "Save to Calendar");
    button.set_tooltip_text(Some("Open this event in your default calendar app"));
    let event = event.clone();
    let error = error.clone();
    button.connect_clicked(move |button| open_calendar(button, &event, &error));
    button
}

fn open_calendar(button: &gtk::Button, event: &Event, error_label: &gtk::Label) {
    let Some(calendar) = event.to_ical() else {
        error_label.set_text("This event has an invalid date and cannot be saved.");
        error_label.set_visible(true);
        return;
    };
    let digest = gtk::glib::compute_checksum_for_string(
        gtk::glib::ChecksumType::Sha256,
        serde_json::to_string(event).unwrap(),
    )
    .unwrap();
    let directory = gtk::glib::user_cache_dir().join("brevlada/calendar");
    let path = directory.join(format!("{digest}.ics"));
    let result = std::fs::create_dir_all(&directory).and_then(|_| std::fs::write(&path, calendar));
    if let Err(error) = result {
        error_label.set_text(&format!("Unable to save event: {error}"));
        error_label.set_visible(true);
        return;
    }
    error_label.set_visible(false);
    button.set_sensitive(false);
    let button = button.clone();
    let error_label = error_label.clone();
    let window = button.root().and_downcast::<gtk::Window>();
    super::super::links::open_file(&path, window.as_ref(), move |result| {
        button.set_sensitive(true);
        if let Err(error) = result {
            error_label.set_text(&format!("Unable to open calendar app: {error}"));
            error_label.set_visible(true);
        }
    });
}
