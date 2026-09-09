use super::{column, label};
use crate::theme;
use adw::prelude::*;

pub fn placeholder(text: &str, class: &str, icon: &str, loading: bool) -> gtk::Box {
    let content = column(class);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_spacing(theme::SPACING);
    if loading {
        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .width_request(theme::AVATAR_SIZE)
            .height_request(theme::AVATAR_SIZE)
            .build();
        content.append(&spinner);
    } else {
        let image = gtk::Image::from_icon_name(icon);
        image.set_pixel_size(theme::STATE_ICON);
        image.set_opacity(0.5);
        content.append(&image);
    }
    let text = label(text, "state-text");
    text.set_halign(gtk::Align::Center);
    text.set_opacity(0.7);
    text.set_wrap(true);
    content.append(&text);
    content
}

pub fn select_message(viewer: &gtk::Box) {
    super::clear(viewer);
    viewer.set_vexpand(true);
    viewer.append(&placeholder(
        "Select message to view",
        "message-viewer-empty-state",
        "mail-unread-symbolic",
        false,
    ));
}

pub fn list_state(stack: &gtk::Stack, text: &str, loading: bool, error: bool) {
    if let Some(previous) = stack.child_by_name("state") {
        stack.remove(&previous);
    }
    let class = if error {
        "message-list-error-state"
    } else if loading {
        "message-list-loading-state"
    } else {
        "message-list-empty-state"
    };
    let state = placeholder(
        text,
        class,
        if error {
            "dialog-error-symbolic"
        } else {
            "mail-unread-symbolic"
        },
        loading,
    );
    if error {
        state.add_css_class("error");
    }
    stack.add_named(&state, Some("state"));
    stack.set_visible_child_name("state");
}

pub fn refreshing(button: &gtk::Button, loading: bool) {
    if loading {
        button.set_child(Some(&gtk::Spinner::builder().spinning(true).build()));
    } else {
        button.set_icon_name("view-refresh-symbolic");
    }
}
