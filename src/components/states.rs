use super::{column, label};
use crate::theme;
use adw::prelude::*;

pub fn placeholder(text: &str, class: &str, icon: &str, loading: bool) -> gtk::Box {
    placeholder_with_text_opacity(text, class, icon, loading, 0.7)
}

fn placeholder_with_text_opacity(
    text: &str,
    class: &str,
    icon: &str,
    loading: bool,
    text_opacity: f64,
) -> gtk::Box {
    let content = column(class);
    content.add_css_class("message-state");
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
    text.set_opacity(text_opacity);
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

pub fn loading_thread(viewer: &gtk::Box) {
    super::clear(viewer);
    viewer.set_vexpand(true);
    viewer.append(&placeholder(
        "Loading conversation...",
        "message-viewer-empty-state",
        "mail-unread-symbolic",
        true,
    ));
}

pub fn list_state(stack: &gtk::Stack, text: &str, loading: bool, error: bool) {
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
            "folder-symbolic"
        },
        loading,
    );
    if error {
        state.add_css_class("error");
    }
    show_list_state(stack, &state);
}

pub fn no_senders_found(stack: &gtk::Stack) {
    let state = placeholder_with_text_opacity(
        "No senders found",
        "message-list-empty-state",
        "system-search-symbolic",
        false,
        1.0,
    );
    show_list_state(stack, &state);
}

fn show_list_state(stack: &gtk::Stack, state: &gtk::Box) {
    if let Some(previous) = stack.child_by_name("state") {
        stack.remove(&previous);
    }
    stack.add_named(state, Some("state"));
    stack.set_visible_child_name("state");
}

pub fn refreshing(button: &gtk::Button, loading: bool) {
    if loading {
        button.set_child(Some(&gtk::Spinner::builder().spinning(true).build()));
    } else {
        button.set_icon_name("view-refresh-symbolic");
    }
}
