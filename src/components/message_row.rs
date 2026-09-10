use super::{column, display, horizontal, label};
use crate::{models::Message, theme};
use adw::prelude::*;

pub fn message_row(group: &[Message]) -> gtk::ListBoxRow {
    let message = group.first().expect("Thread has a message");
    let unread = group.iter().any(|m| !m.is_read);
    let row = gtk::ListBoxRow::builder()
        .activatable(true)
        .selectable(true)
        .css_classes(["message-row"])
        .build();
    let container = horizontal("message-row-container", theme::ROW_GAP);
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
    let sender_row = horizontal("message-row-sender-container", theme::ROW_GAP);
    if group.len() > 1 {
        let badge = horizontal("thread-count-container", 0);
        badge.set_valign(gtk::Align::Center);
        let count = label(&group.len().to_string(), "thread-count-badge-label");
        count.add_css_class("heading");
        count.add_css_class("message-row-sender");
        if unread {
            count.add_css_class("message-row-sender-unread");
        }
        count.set_halign(gtk::Align::Center);
        count.set_hexpand(false);
        count.set_ellipsize(gtk::pango::EllipsizeMode::None);
        badge.append(&count);
        sender_row.append(&badge);
    }
    let sender = label(&display::sender_name(message), "message-row-sender");
    sender.add_css_class("heading");
    if unread {
        sender.add_css_class("message-row-sender-unread");
    }
    sender_row.append(&sender);
    content.append(&sender_row);
    let subject = label(&display::subject(message), "message-row-subject-label");
    subject.add_css_class("dim-label");
    if unread {
        subject.add_css_class("message-row-subject-unread");
    }
    content.append(&subject);
    left.append(&content);
    container.append(&left);
    let right = column("message-row-right");
    right.set_spacing(theme::ROW_VERTICAL_GAP);
    let latest = group.iter().max_by_key(|m| m.timestamp).unwrap();
    let date = label(&display::date(latest, false), "message-row-date");
    date.set_halign(gtk::Align::End);
    date.set_hexpand(false);
    right.append(&date);
    let icons = horizontal("message-row-icons", theme::ROW_GAP);
    icons.set_halign(gtk::Align::End);
    if group.iter().any(|message| message.is_flagged) {
        let flag = gtk::Image::from_icon_name("starred-symbolic");
        flag.add_css_class("message-row-flag-icon");
        icons.append(&flag);
    }
    right.append(&icons);
    container.append(&right);
    row.set_child(Some(&container));
    row
}

pub fn update(row: &gtk::ListBoxRow, group: &[Message]) {
    let replacement = message_row(group);
    let child = replacement.child();
    replacement.set_child(None::<&gtk::Widget>);
    row.set_child(child.as_ref());
}
