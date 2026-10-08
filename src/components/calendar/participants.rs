use crate::{models::calendar::Participant, theme};
use adw::prelude::*;

fn line(orientation: gtk::Orientation, class: &str) -> gtk::Separator {
    let line = gtk::Separator::new(orientation);
    line.add_css_class(class);
    line
}

fn header(person: &Participant, avatars: &super::super::avatars::Avatars) -> gtk::Box {
    let row = super::super::horizontal("calendar-participant-header", theme::SMALL_SPACING);
    let attendee = &person.attendee;
    let name = attendee.name.as_deref().unwrap_or(&attendee.email);
    let avatar = adw::Avatar::new(theme::CALENDAR_AVATAR_SIZE, Some(name), true);
    avatar.set_valign(gtk::Align::Center);
    avatars.attach(&avatar, &attendee.email);
    row.append(&avatar);
    let name = super::super::label(name, "calendar-participant-name");
    name.set_hexpand(false);
    name.set_tooltip_text(Some(&attendee.email));
    if person.unread {
        name.add_css_class("heading");
    }
    row.append(&name);
    let separator = super::super::label("·", "dim-label");
    separator.set_hexpand(false);
    row.append(&separator);
    let status = super::super::label(&person.response_label(), "dim-label");
    status.set_hexpand(false);
    status.set_halign(gtk::Align::Start);
    row.append(&status);
    row
}

pub fn rows(people: &[Participant], avatars: &super::super::avatars::Avatars) -> gtk::Box {
    let tree = super::super::column("calendar-participants");
    for (index, person) in people.iter().enumerate() {
        let branch = gtk::Grid::new();
        branch.add_css_class("calendar-participant");
        let connector = super::super::column("calendar-branch");
        connector.append(&line(gtk::Orientation::Vertical, "calendar-branch-top"));
        if index + 1 < people.len() {
            let stem = line(gtk::Orientation::Vertical, "calendar-branch-stem");
            stem.set_vexpand(true);
            connector.append(&stem);
        } else {
            connector.set_valign(gtk::Align::Start);
        }
        branch.attach(&connector, 0, 0, 1, 1);
        let twig = line(gtk::Orientation::Horizontal, "calendar-branch-twig");
        twig.set_valign(gtk::Align::Start);
        branch.attach(&twig, 1, 0, 1, 1);
        let header = header(person, avatars);
        branch.attach(&header, 2, 0, 1, 1);
        tree.append(&branch);
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::calendar::Attendee;
    use chrono::TimeZone;

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn connects_static_response_rows_without_expanders() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let stamp = chrono::Local
            .with_ymd_and_hms(2026, 10, 3, 16, 35, 0)
            .unwrap()
            .timestamp();
        let people = [
            Participant {
                attendee: Attendee {
                    email: "alex@example.com".into(),
                    name: Some("Alex".into()),
                    status: "ACCEPTED".into(),
                },
                responded_at: Some(stamp),
                unread: false,
            },
            Participant {
                attendee: Attendee {
                    email: "sam@example.com".into(),
                    name: Some("Sam".into()),
                    status: "NEEDS-ACTION".into(),
                },
                responded_at: None,
                unread: false,
            },
        ];
        let avatars = super::super::super::avatars::Avatars::new(|_| {});
        let tree = rows(&people, &avatars);
        let first = tree.first_child().unwrap().downcast::<gtk::Grid>().unwrap();
        assert!(
            first
                .child_at(0, 0)
                .unwrap()
                .last_child()
                .unwrap()
                .has_css_class("calendar-branch-stem")
        );
        let header = first
            .child_at(2, 0)
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
        assert_eq!(
            header
                .first_child()
                .unwrap()
                .downcast::<adw::Avatar>()
                .unwrap()
                .size(),
            theme::CALENDAR_AVATAR_SIZE
        );
        assert_eq!(
            header
                .last_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "Accepted · 3 Oct 2026 16:35"
        );
        let last = tree.last_child().unwrap().downcast::<gtk::Grid>().unwrap();
        assert!(
            last.child_at(0, 0)
                .unwrap()
                .last_child()
                .unwrap()
                .has_css_class("calendar-branch-top")
        );
        assert_eq!(
            last.child_at(2, 0)
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "Awaiting response"
        );
        let provider = gtk::CssProvider::new();
        provider.connect_parsing_error(|_, _, error| panic!("{error}"));
        provider.load_from_string(theme::CSS);
    }
}
