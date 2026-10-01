use super::{button, column, horizontal};
use crate::models::Draft;
use crate::theme;
use adw::prelude::*;
use std::cell::RefCell;

#[derive(Default)]
struct Reply {
    in_reply_to: Option<String>,
    references: Vec<String>,
}

type CancelAction = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
pub type Complete = Box<dyn FnOnce(Result<Draft, String>)>;
use std::rc::Rc;

pub struct Compose {
    pub widget: gtk::Box,
    receiver: gtk::Entry,
    cc: gtk::Entry,
    subject: gtk::Entry,
    body: gtk::TextView,
    html_mode: gtk::ToggleButton,
    html_editor: super::html_editor::Editor,
    text_mode: gtk::ToggleButton,
    send: gtk::Button,
    save: gtk::Button,
    reply_to: Rc<RefCell<Reply>>,
    cancel_action: CancelAction,
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
        let save = button("folder-download-symbolic", "Save draft");
        header.append(&save);
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
        let receiver_input = receiver.clone();
        subject.connect_changed(move |subject| {
            if let Some(send) = send_button.upgrade() {
                update_send(&send, &receiver_input, subject, &body_buffer);
            }
        });
        let send_button = send.downgrade();
        let receiver_input = receiver.clone();
        let subject_input = subject.downgrade();
        buffer.connect_changed(move |buffer| {
            if let (Some(send), Some(subject)) = (send_button.upgrade(), subject_input.upgrade()) {
                update_send(&send, &receiver_input, &subject, buffer);
            }
        });
        let send_button = send.downgrade();
        let subject_input = subject.downgrade();
        let body_buffer = buffer.clone();
        receiver.connect_changed(move |receiver| {
            if let (Some(send), Some(subject)) = (send_button.upgrade(), subject_input.upgrade()) {
                update_send(&send, receiver, &subject, &body_buffer);
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
        let cc_input = cc.clone();
        let subject_input = subject.clone();
        let body_input = body.clone();
        let rich_input = html_editor.clone();
        let plain_mode = text_mode.clone();
        let reply_to = Rc::new(RefCell::new(Reply::default()));
        let cancel_action: CancelAction = Rc::default();
        let on_cancel = cancel_action.clone();
        let cancelled_reply = reply_to.clone();
        cancel.connect_clicked(move |_| {
            if let Some(callback) = on_cancel.borrow().as_ref() {
                callback();
                return;
            }
            if let Some(card) = card.upgrade() {
                card.set_visible(false);
            }
            receiver_input.set_text("");
            cc_input.set_text("");
            subject_input.set_text("");
            plain_mode.set_active(true);
            body_input.buffer().set_text("");
            rich_input.set_html("");
            *cancelled_reply.borrow_mut() = Reply::default();
            if let Some(trigger) = trigger.upgrade() {
                trigger.grab_focus();
            }
        });
        Self {
            widget,
            receiver,
            cc,
            subject,
            body,
            html_mode,
            html_editor,
            text_mode,
            send,
            save,
            reply_to,
            cancel_action,
        }
    }

    pub fn connect_cancel(&self, callback: impl Fn() + 'static) {
        *self.cancel_action.borrow_mut() = Some(Rc::new(callback));
    }

    pub fn load_draft(&self, draft: &Draft) {
        self.text_mode.set_active(true);
        self.receiver.set_text(&draft.to);
        self.cc.set_text(&draft.cc);
        self.subject.set_text(&draft.subject);
        self.body.buffer().set_text(&draft.text);
        *self.reply_to.borrow_mut() = Reply {
            in_reply_to: draft.in_reply_to.clone(),
            references: draft.references.clone(),
        };
        if let Some(html) = &draft.html {
            self.html_mode.set_active(true);
            self.html_editor.set_html(html);
        }
        self.widget.set_visible(true);
    }

    pub fn set_ready(&self, ready: bool) {
        self.receiver.set_sensitive(ready);
        self.cc.set_sensitive(ready);
        self.subject.set_sensitive(ready);
        self.save.set_sensitive(ready);
        let mut child = self
            .widget
            .first_child()
            .and_then(|header| header.next_sibling());
        while let Some(widget) = child {
            child = widget.next_sibling();
            widget.set_sensitive(ready);
        }
        if ready {
            update_send(
                &self.send,
                &self.receiver,
                &self.subject,
                &self.body.buffer(),
            );
        } else {
            self.send.set_sensitive(false);
        }
    }

    pub fn connect_send(&self, callback: impl Fn() -> Complete + 'static) {
        self.connect_submit(&self.send, callback);
    }

    pub fn connect_save(&self, callback: impl Fn() -> Complete + 'static) {
        self.connect_submit(&self.save, callback);
    }

    fn connect_submit(&self, button: &gtk::Button, submit: impl Fn() -> Complete + 'static) {
        let receiver = self.receiver.clone();
        let cc = self.cc.clone();
        let subject = self.subject.clone();
        let body = self.body.clone();
        let mode = self.html_mode.clone();
        let editor = self.html_editor.clone();
        let widget = self.widget.clone();
        let callback = Rc::new(submit);
        let reply_to = self.reply_to.clone();
        button.connect_clicked(move |_| {
            if !widget.is_sensitive() {
                return;
            }
            widget.set_sensitive(false);
            let draft = Draft {
                to: receiver.text().to_string(),
                cc: cc.text().to_string(),
                subject: subject.text().to_string(),
                text: body
                    .buffer()
                    .text(&body.buffer().start_iter(), &body.buffer().end_iter(), true)
                    .to_string(),
                html: None,
                attachments: Vec::new(),
                in_reply_to: reply_to.borrow().in_reply_to.clone(),
                references: reply_to.borrow().references.clone(),
            };
            let callback = callback();
            let mode = mode.clone();
            let editor = editor.clone();
            gtk::glib::MainContext::default().spawn_local(async move {
                let result = if mode.is_active() {
                    editor
                        .message()
                        .await
                        .map(|(html, text)| Draft {
                            html: Some(html),
                            text,
                            ..draft
                        })
                        .map_err(|error| format!("Could not read formatted message: {error}"))
                } else {
                    Ok(draft)
                };
                callback(result);
            });
        });
    }

    pub fn finish_send(&self, success: bool) {
        self.widget.set_sensitive(true);
        if success {
            self.widget.set_visible(false);
            self.receiver.set_text("");
            self.cc.set_text("");
            self.subject.set_text("");
            self.text_mode.set_active(true);
            self.body.buffer().set_text("");
            self.html_editor.set_html("");
            *self.reply_to.borrow_mut() = Reply::default();
        }
    }

    pub fn show(&self, receiver: &str) {
        if !self.widget.get_visible() {
            self.receiver.set_text(receiver);
        }
        self.widget.set_visible(true);
        self.receiver.grab_focus();
    }

    pub fn reply(&self, message: &crate::models::Message, recipient: &str) {
        if !self.widget.get_visible() {
            if !message.message_id.is_empty() {
                let mut references = message.references.clone();
                references.push(message.message_id.clone());
                *self.reply_to.borrow_mut() = Reply {
                    in_reply_to: Some(message.message_id.clone()),
                    references,
                };
            }
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
        self.show(recipient);
        if self.html_mode.is_active() {
            self.html_editor.focus();
        } else {
            self.body.grab_focus();
        }
    }
}

fn update_send(
    send: &gtk::Button,
    receiver: &gtk::Entry,
    subject: &gtk::Entry,
    body: &gtk::TextBuffer,
) {
    send.set_sensitive(
        !receiver.text().trim().is_empty()
            && !subject.text().trim().is_empty()
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
