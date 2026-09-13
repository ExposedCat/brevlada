use super::{button, column, horizontal};
use crate::theme;
use adw::prelude::*;

pub struct Compose {
    pub widget: gtk::Box,
    receiver: gtk::Entry,
    subject: gtk::Entry,
    body: gtk::TextView,
    html_mode: gtk::ToggleButton,
    html_editor: super::html_editor::Editor,
}

impl Compose {
    pub fn new(trigger: &gtk::Button) -> Self {
        let widget = column("compose-card");
        widget.add_css_class("message-row-widget");
        widget.set_visible(false);
        widget.set_vexpand(false);
        let header = horizontal("compose-header", theme::SMALL_SPACING);
        let receiver = entry("Receiver");
        let cc = entry("CC");
        header.append(&receiver);
        header.append(&cc);
        let send_content = horizontal("compose-send-content", theme::SMALL_SPACING);
        send_content.append(&gtk::Image::from_icon_name("paper-plane-symbolic"));
        send_content.append(&gtk::Label::new(Some("Send")));
        let send = gtk::Button::builder()
            .child(&send_content)
            .tooltip_text("Send")
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();
        send.add_css_class("suggested-action");
        send.add_css_class("compose-send");
        header.append(&send);
        header.append(&button("folder-download-symbolic", "Save draft"));
        let cancel = button("window-close-symbolic", "Cancel");
        header.append(&cancel);
        widget.append(&header);
        let subject = entry("Subject");
        widget.append(&subject);
        let body_group = adw::PreferencesGroup::builder()
            .css_classes(["compose-body"])
            .build();
        let body = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(theme::SPACING)
            .bottom_margin(theme::SPACING)
            .left_margin(theme::SPACING)
            .right_margin(theme::SPACING)
            .tooltip_text("Body")
            .build();
        let modes = horizontal("linked", 0);
        let text_mode = gtk::ToggleButton::with_label("Text");
        let html_mode = gtk::ToggleButton::with_label("HTML");
        html_mode.set_group(Some(&text_mode));
        text_mode.set_active(true);
        text_mode.set_tooltip_text(Some("Plain text"));
        html_mode.set_tooltip_text(Some("Formatted text"));
        modes.append(&text_mode);
        modes.append(&html_mode);
        modes.set_halign(gtk::Align::Start);
        let body_header = horizontal("compose-body-header", theme::SMALL_SPACING);
        body_header.append(&modes);
        let size = super::editor_size::Size::new();
        size.track_text(&body);
        let html_size = std::rc::Rc::downgrade(&size);
        let html_editor =
            super::html_editor::Editor::new(&body.buffer(), &html_mode, move |height, edited| {
                if let Some(size) = html_size.upgrade() {
                    size.measure(1, height, edited);
                }
            });
        let formatting = super::html_editor::toolbar(&html_editor, &html_mode);
        body_header.append(&formatting);
        widget.append(&body_header);
        let buffer = body.buffer();
        let send_button = send.downgrade();
        let body_buffer = buffer.clone();
        subject.connect_changed(move |subject| {
            if let Some(send) = send_button.upgrade() {
                update_send(&send, subject, &body_buffer);
            }
        });
        let send_button = send.downgrade();
        let subject_input = subject.downgrade();
        buffer.connect_changed(move |buffer| {
            if let (Some(send), Some(subject)) = (send_button.upgrade(), subject_input.upgrade()) {
                update_send(&send, &subject, buffer);
            }
        });
        let body_scroll = gtk::ScrolledWindow::builder()
            .child(&body)
            .min_content_height(theme::COMPOSE_HEIGHT)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let editors = &size.stack;
        editors.add_named(&body_scroll, Some("text"));
        editors.add_named(&html_editor.view, Some("html"));
        editors.set_visible_child_name("text");
        let dirty = std::rc::Rc::new(std::cell::Cell::new(true));
        let changed = dirty.clone();
        let mode = html_mode.downgrade();
        buffer.connect_changed(move |_| {
            if mode.upgrade().is_some_and(|mode| !mode.is_active()) {
                changed.set(true);
            }
        });
        let stack = editors.downgrade();
        let editor = html_editor.clone();
        let input = body.clone();
        html_mode.connect_toggled(move |mode| {
            if let Some(stack) = stack.upgrade() {
                if mode.is_active() {
                    if dirty.replace(false) {
                        let buffer = input.buffer();
                        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                        editor.set_html(&format!(
                            "<pre>{}</pre>",
                            gtk::glib::markup_escape_text(&text)
                        ));
                    }
                    stack.set_visible_child_name("html");
                    editor.focus();
                } else {
                    stack.set_visible_child_name("text");
                    input.grab_focus();
                }
            }
        });
        let body_row = adw::PreferencesRow::builder()
            .child(editors)
            .css_classes(["compose-editor"])
            .overflow(gtk::Overflow::Hidden)
            .activatable(false)
            .selectable(false)
            .focusable(false)
            .build();
        body_group.add(&body_row);
        widget.append(&body_group);
        widget.append(&size.handle());
        let card = widget.downgrade();
        let trigger = trigger.downgrade();
        let receiver_input = receiver.clone();
        let subject_input = subject.clone();
        let body_input = body.clone();
        let rich_input = html_editor.clone();
        cancel.connect_clicked(move |_| {
            if let Some(card) = card.upgrade() {
                card.set_visible(false);
            }
            receiver_input.set_text("");
            cc.set_text("");
            subject_input.set_text("");
            text_mode.set_active(true);
            body_input.buffer().set_text("");
            rich_input.set_html("");
            if let Some(trigger) = trigger.upgrade() {
                trigger.grab_focus();
            }
        });
        Self {
            widget,
            receiver,
            subject,
            body,
            html_mode,
            html_editor,
        }
    }

    pub fn show(&self, receiver: &str) {
        if !self.widget.get_visible() {
            self.receiver.set_text(receiver);
        }
        self.widget.set_visible(true);
        self.receiver.grab_focus();
    }

    pub fn reply(&self, message: &crate::models::Message) {
        if !self.widget.get_visible() {
            let subject = message.subject.trim();
            self.subject
                .set_text(&if subject.to_ascii_lowercase().starts_with("re:") {
                    subject.to_owned()
                } else {
                    format!("Re: {subject}")
                });
            self.html_mode
                .set_active(!message.body_html.trim().is_empty());
            if self.html_mode.is_active() {
                self.html_editor.reply(&message.body_html);
            } else {
                super::reply_quote::insert(&self.body.buffer(), message);
            }
        }
        self.show(&super::display::sender(message).1);
        if self.html_mode.is_active() {
            self.html_editor.focus();
        } else {
            self.body.grab_focus();
        }
    }
}

fn update_send(send: &gtk::Button, subject: &gtk::Entry, body: &gtk::TextBuffer) {
    send.set_sensitive(
        !subject.text().trim().is_empty()
            && !body
                .text(&body.start_iter(), &body.end_iter(), true)
                .trim()
                .is_empty(),
    );
}

fn entry(name: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .placeholder_text(name)
        .tooltip_text(name)
        .hexpand(true)
        .width_chars(8)
        .build()
}
