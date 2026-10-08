use super::{avatars::Avatars, column, display, horizontal, label, preview};
use crate::{models::Message, models::senders, theme};
use adw::prelude::*;

pub fn message_row(group: &[Message]) -> gtk::Box {
    let message = group.first().expect("Thread has a message");
    let unread = group.iter().any(|m| !m.is_read);
    let container = horizontal("message-row-container", theme::ROW_GAP);
    container.add_css_class("message-row");
    container.add_css_class("message-row-with-icon");
    let left = horizontal("message-row-left", theme::ICON_GAP);
    left.set_hexpand(true);
    left.append(&gtk::Image::from_icon_name(if unread {
        "mail-unread-symbolic"
    } else {
        "mail-read-symbolic"
    }));
    let content = column("message-row-content");
    content.set_hexpand(true);
    content.set_spacing(theme::ROW_VERTICAL_GAP);
    let title_row = horizontal("message-row-sender-container", theme::ROW_GAP);
    if group.len() > 1 {
        title_row.append(&count_badge(group.len(), unread));
    }
    let subject = label(&display::subject(message), "message-row-sender");
    subject.add_css_class("heading");
    if unread {
        subject.add_css_class("message-row-sender-unread");
    }
    title_row.append(&subject);
    content.append(&title_row);
    let description = label(&preview::thread(group), "message-row-subject-label");
    description.add_css_class("dim-label");
    if unread {
        description.add_css_class("message-row-subject-unread");
    }
    content.append(&description);
    left.append(&content);
    container.append(&left);
    let latest = group.iter().max_by_key(|m| m.timestamp).unwrap();
    let right = metadata(
        latest,
        group.iter().any(|message| message.is_flagged),
        group.iter().any(|message| message.is_spam),
    );
    container.append(&right);
    container
}

pub fn sender_row(group: &[Message], avatars: &Avatars) -> gtk::Box {
    let unread = group.iter().any(|m| !m.is_read);
    let message = senders::preview(group).expect("Sender has a message");
    let container = horizontal("message-row-container", theme::SPACING);
    container.add_css_class("message-row");
    let name = display::sender_name(message);
    let avatar = adw::Avatar::new(theme::SENDER_AVATAR_SIZE, Some(&name), true);
    avatars.attach(&avatar, &senders::key(message));
    container.append(&avatar);
    let content = column("message-row-content");
    content.set_hexpand(true);
    content.set_spacing(theme::ROW_VERTICAL_GAP);
    let heading = horizontal("message-row-sender-container", theme::ROW_GAP);
    let matches = group.iter().filter(|message| message.search_match).count();
    if matches > 1 {
        let badge = count_badge(matches, unread);
        badge.set_tooltip_text(Some(&format!("{matches} matching messages")));
        heading.append(&badge);
    }
    let sender = label(&name, "message-row-sender");
    if unread {
        sender.add_css_class("message-row-sender-unread");
    }
    heading.append(&sender);
    content.append(&heading);
    let subject = if message.subject.trim().is_empty() {
        "(No Subject)"
    } else {
        &message.subject
    };
    let subject = label(subject, "message-row-subject-label");
    subject.add_css_class("dim-label");
    if unread {
        subject.add_css_class("message-row-subject-unread");
    }
    content.append(&subject);
    container.append(&content);
    container.append(&metadata(
        message,
        group.iter().any(|message| message.is_flagged),
        false,
    ));
    container.set_tooltip_text(Some(&display::sender(message).1));
    container
}

fn count_badge(count: usize, unread: bool) -> gtk::Box {
    let badge = horizontal("thread-count-container", 0);
    badge.set_valign(gtk::Align::Center);
    let count = label(&count.to_string(), "thread-count-badge-label");
    count.add_css_class("heading");
    count.add_css_class("message-row-sender");
    if unread {
        count.add_css_class("message-row-sender-unread");
    }
    count.set_halign(gtk::Align::Center);
    count.set_hexpand(false);
    count.set_ellipsize(gtk::pango::EllipsizeMode::None);
    badge.append(&count);
    badge
}

fn metadata(message: &Message, starred: bool, spam: bool) -> gtk::Box {
    let right = column("message-row-right");
    right.set_spacing(theme::ROW_VERTICAL_GAP);
    let date = label(&display::date(message, false), "message-row-date");
    date.set_halign(gtk::Align::End);
    date.set_hexpand(false);
    right.append(&date);
    let icons = horizontal("message-row-icons", theme::ROW_GAP);
    icons.set_halign(gtk::Align::End);
    if spam {
        let icon = gtk::Image::from_icon_name("mail-mark-junk-symbolic");
        icon.add_css_class("message-row-spam-icon");
        icon.set_tooltip_text(Some("Spam"));
        icons.append(&icon);
    }
    if starred {
        let star = gtk::Image::from_icon_name("starred-symbolic");
        star.add_css_class("message-row-flag-icon");
        star.set_halign(gtk::Align::End);
        star.set_tooltip_text(Some("Starred"));
        icons.append(&star);
    }
    right.append(&icons);
    right
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session to inspect stars below message dates"]
    fn stars_follow_dates_in_sender_and_conversation_rows() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let css = gtk::CssProvider::new();
        css.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let starred = Message {
            uid: 1,
            sender: "Alice <alice@example.com>".into(),
            subject: "Weekend plans".into(),
            body_text: "See you on Saturday.".into(),
            timestamp: chrono::Utc::now().timestamp(),
            is_flagged: true,
            is_spam: true,
            ..Default::default()
        };
        let plain = Message {
            uid: 2,
            is_flagged: false,
            is_spam: false,
            subject: "Monthly newsletter".into(),
            ..starred.clone()
        };
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.set_margin_top(24);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.set_margin_end(24);
        let avatars = Avatars::new(|_| {});
        for (spam, row) in [
            (false, sender_row(std::slice::from_ref(&starred), &avatars)),
            (true, message_row(std::slice::from_ref(&starred))),
        ] {
            let metadata = row.last_child().unwrap();
            let date = metadata.first_child().unwrap();
            let star = date.next_sibling().unwrap().last_child().unwrap();
            assert!(date.has_css_class("message-row-date"));
            assert!(star.has_css_class("message-row-flag-icon"));
            assert_eq!(
                date.next_sibling()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .has_css_class("message-row-spam-icon"),
                spam,
            );
            content.append(&row);
        }
        content.append(&sender_row(std::slice::from_ref(&plain), &avatars));
        content.append(&message_row(std::slice::from_ref(&plain)));
        let window = gtk::Window::builder()
            .default_width(450)
            .child(&content)
            .build();
        window.present();
        for _ in 0..20 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for row in [
            content.first_child().unwrap(),
            content.first_child().unwrap().next_sibling().unwrap(),
        ] {
            let metadata = row.last_child().unwrap();
            let date = metadata.first_child().unwrap();
            let star = date.next_sibling().unwrap().last_child().unwrap();
            let date_bounds = date.compute_bounds(&metadata).unwrap();
            let star_bounds = star.compute_bounds(&metadata).unwrap();
            assert!(star_bounds.y() >= date_bounds.y() + date_bounds.height());
            let spam = date.next_sibling().unwrap().first_child().unwrap();
            if spam.has_css_class("message-row-spam-icon") {
                let spam_bounds = spam.compute_bounds(&metadata).unwrap();
                assert!(spam_bounds.x() + spam_bounds.width() <= star_bounds.x());
            }
        }
        if let Ok(path) = std::env::var("BREVLADA_STAR_SCREENSHOT") {
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            let texture = window
                .renderer()
                .unwrap()
                .render_texture(&snapshot.to_node().unwrap(), None);
            texture.save_to_png(path).unwrap();
        }
        window.close();
    }
}
