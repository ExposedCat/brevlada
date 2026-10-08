mod submission;

use super::{button, column, horizontal};
use crate::models::{
    Draft, Message,
    draft::Target,
    response::{self, Action},
};
use crate::theme;
use adw::prelude::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Reply {
    in_reply_to: Option<String>,
    references: Vec<String>,
    attachment_source: Option<Target>,
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
    cancel: gtk::Button,
    save_on_close: Rc<Cell<bool>>,
    send_error: gtk::Label,
    attachments: Rc<super::compose_attachments::Attachments>,
    reply_to: Rc<RefCell<Reply>>,
    cancel_action: CancelAction,
    pub autosave: Rc<super::autosave::Autosave>,
    draft_target: RefCell<Option<Target>>,
}

impl Compose {
    pub fn new(trigger: &gtk::Button) -> Self {
        let autosave = super::autosave::Autosave::new();
        let widget = column("compose-card");
        widget.add_css_class("message-row-widget");
        widget.set_visible(false);
        widget.set_vexpand(false);
        let send_error = super::error_label();
        widget.append(&send_error);
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
        let cancel = button("window-close-symbolic", "Close");
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
        let html_autosave = autosave.clone();
        let html_editor =
            super::html_editor::Editor::new(&body.buffer(), &html_mode, move |height, edited| {
                if edited {
                    html_autosave.changed();
                }
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
        let attachments = super::compose_attachments::Attachments::new();
        widget.append(&attachments.toolbar);
        widget.append(&attachments.widget);
        let send_button = send.downgrade();
        let receiver_input = receiver.clone();
        let subject_input = subject.clone();
        let body_buffer = body.buffer();
        let loading_autosave = autosave.clone();
        attachments.connect_loading(move |loading| {
            if !loading {
                loading_autosave.changed();
            }
            if let Some(send) = send_button.upgrade() {
                if loading {
                    send.set_sensitive(false);
                } else {
                    update_send(&send, &receiver_input, &subject_input, &body_buffer);
                }
            }
        });
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
        let cancelled_error = send_error.clone();
        let cancelled_attachments = attachments.clone();
        widget.append(&autosave.status);
        let cancelled_autosave = autosave.clone();
        let save_on_close = Rc::new(Cell::new(true));
        let flush_on_close = save_on_close.clone();
        cancel.connect_clicked(move |_| {
            let on_cancel = on_cancel.clone();
            let card = card.clone();
            let receiver_input = receiver_input.clone();
            let cc_input = cc_input.clone();
            let subject_input = subject_input.clone();
            let plain_mode = plain_mode.clone();
            let body_input = body_input.clone();
            let rich_input = rich_input.clone();
            let cancelled_attachments = cancelled_attachments.clone();
            let cancelled_reply = cancelled_reply.clone();
            let cancelled_error = cancelled_error.clone();
            let trigger = trigger.clone();
            cancelled_autosave.defer(flush_on_close.get(), move || {
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
                cancelled_attachments.clear();
                *cancelled_reply.borrow_mut() = Reply::default();
                cancelled_error.set_visible(false);
                if let Some(trigger) = trigger.upgrade() {
                    trigger.grab_focus();
                }
            });
        });
        for input in [&receiver, &cc, &subject] {
            let autosave = autosave.clone();
            input.connect_changed(move |_| autosave.changed());
        }
        let changed = autosave.clone();
        let mode = html_mode.downgrade();
        body.buffer().connect_changed(move |_| {
            if mode.upgrade().is_some_and(|mode| !mode.is_active()) {
                changed.changed();
            }
        });
        let changed = autosave.clone();
        html_mode.connect_toggled(move |_| changed.changed());
        let changed = autosave.clone();
        attachments.connect_changed(move || changed.changed());

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
            cancel,
            save_on_close,
            send_error,
            attachments,
            reply_to,
            cancel_action,
            autosave,
            draft_target: RefCell::default(),
        }
    }

    pub fn discard_on_close(&self) {
        self.save_on_close.set(false);
        self.cancel.set_tooltip_text(Some("Delete draft"));
    }

    pub fn connect_cancel(&self, callback: impl Fn() + 'static) {
        *self.cancel_action.borrow_mut() = Some(Rc::new(callback));
    }

    pub fn load_draft(&self, draft: &Draft) {
        self.attachments.load(draft);
        self.send_error.set_visible(false);
        self.text_mode.set_active(true);
        self.receiver.set_text(&draft.to);
        self.cc.set_text(&draft.cc);
        self.subject.set_text(&draft.subject);
        self.body.buffer().set_text(&draft.text);
        *self.reply_to.borrow_mut() = Reply {
            in_reply_to: draft.in_reply_to.clone(),
            references: draft.references.clone(),
            attachment_source: draft.attachment_source.clone(),
        };
        if let Some(html) = &draft.html {
            self.html_mode.set_active(true);
            self.html_editor.set_html(html);
        }
        self.widget.set_visible(true);
        self.autosave.reset();
    }

    pub fn set_ready(&self, ready: bool) {
        self.receiver.set_sensitive(ready);
        self.cc.set_sensitive(ready);
        self.subject.set_sensitive(ready);
        let mut child = self
            .receiver
            .parent()
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

    pub fn finish_send(&self, success: bool) {
        self.widget.set_sensitive(true);
        if success {
            self.autosave.reset();
            self.send_error.set_visible(false);
            self.widget.set_visible(false);
            self.receiver.set_text("");
            self.cc.set_text("");
            self.subject.set_text("");
            self.text_mode.set_active(true);
            self.body.buffer().set_text("");
            self.html_editor.set_html("");
            self.attachments.clear();
            *self.reply_to.borrow_mut() = Reply::default();
        }
    }

    pub fn send_failed(&self, error: &str) {
        self.finish_send(false);
        self.send_error
            .set_text(&format!("Could not send message: {error}"));
        self.send_error.set_visible(true);
        self.autosave.resume();
    }

    pub fn show(&self, receiver: &str) {
        if !self.widget.get_visible() {
            self.receiver.set_text(receiver);
        }
        self.widget.set_visible(true);
        self.receiver.grab_focus();
    }

    pub fn respond(&self, message: &Message, account: &str, folder: &str, action: Action) {
        if !self.widget.get_visible() {
            let (to, cc) = response::recipients(message, account, action);
            self.receiver.set_text(&to);
            self.cc.set_text(&cc);
            self.subject.set_text(&response::subject(message, action));
            let mut context = Reply::default();
            if action == Action::Forward {
                if !message.attachments.is_empty()
                    || !message.inline_media.is_empty()
                    || message.body_html.contains("cid:")
                {
                    context.attachment_source = Some(Target::new(folder, message));
                }
                self.attachments.load(&Draft::from(message));
            } else {
                context.references = message.references.clone();
                if !message.message_id.is_empty() {
                    context.references.push(message.message_id.clone());
                    context.in_reply_to = Some(message.message_id.clone());
                }
            }
            *self.reply_to.borrow_mut() = context;
            self.text_mode.set_active(true);
            self.body.buffer().set_text("");
            self.html_mode
                .set_active(!message.body_html.trim().is_empty());
            if action == Action::Forward {
                let header = response::forward_header(message);
                if self.html_mode.is_active() {
                    self.html_editor.forward(&header, &message.body_html);
                } else {
                    self.body
                        .buffer()
                        .set_text(&format!("\n\n{header}{}", message.body_text));
                    self.body
                        .buffer()
                        .place_cursor(&self.body.buffer().start_iter());
                }
            } else if self.html_mode.is_active() {
                self.html_editor.reply(&message.body_html);
            } else {
                super::reply_quote::insert(&self.body.buffer(), message);
            }
        }
        self.widget.set_visible(true);
        if action == Action::Forward {
            self.receiver.grab_focus();
        } else if self.html_mode.is_active() {
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

#[cfg(test)]
mod diagnostics {
    use super::*;
    use webkit6::prelude::WebViewExt;

    #[test]
    #[ignore = "Requires a graphical session and WebKit"]
    fn autosaves_html_formatting_without_resaving_loaded_content() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let compose = Rc::new(Compose::new(&gtk::Button::new()));
        let submissions = Rc::new(RefCell::new(Vec::new()));
        let sent = submissions.clone();
        let weak = Rc::downgrade(&compose);
        compose.connect_autosave(move || {
            let sent = sent.clone();
            let weak = weak.clone();
            Box::new(move |draft| {
                sent.borrow_mut().push(draft.unwrap());
                if let Some(compose) = weak.upgrade() {
                    compose.autosave.complete(Ok(()));
                }
            })
        });
        compose.load_draft(&Draft::from(&Message {
            recipients: "other@example.com".into(),
            subject: "Formatted".into(),
            body_text: "Body".into(),
            body_html: "<p>Body</p>".into(),
            ..Default::default()
        }));
        let window = gtk::Window::new();
        window.set_child(Some(&compose.widget));
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(async {
            compose.html_editor.message().await.unwrap();
            gtk::glib::timeout_future(std::time::Duration::from_millis(2200)).await;
            assert!(submissions.borrow().is_empty());
            super::super::html_editor::run(
                &compose.html_editor.view,
                "document.body.innerHTML = '<p><b>Body</b></p>'; report(true)",
            );
            gtk::glib::timeout_future(std::time::Duration::from_millis(2300)).await;
        });
        let sent = submissions.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].html.as_deref(), Some("<p><b>Body</b></p>"));
        assert_eq!(sent[0].text.trim(), "Body");
        assert!(compose.widget.is_sensitive());
        compose.html_editor.view.stop_loading();
        window.destroy();
    }

    #[test]
    #[ignore = "Requires a graphical session and WebKit"]
    fn responses_submit_correct_recipients_thread_headers_and_forward_source() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let message = Message {
            uid: 42,
            uid_validity: Some(7),
            message_id: "original@example.com".into(),
            sender: "sender@example.com".into(),
            reply_to: "support@example.com".into(),
            recipients: "me@example.com, team@example.com".into(),
            cc: "copy@example.com, TEAM@example.com".into(),
            subject: "Plans".into(),
            date: "Thu, 1 Oct 2026 10:00:00 +0000".into(),
            references: vec!["parent@example.com".into()],
            body_text: "Original body".into(),
            attachments: vec!["report.pdf".into()],
            body_loaded: true,
            ..Default::default()
        };
        for action in [Action::Reply, Action::ReplyAll, Action::Forward] {
            let compose = Rc::new(Compose::new(&gtk::Button::new()));
            compose.respond(&message, "me@example.com", "INBOX", action);
            if action == Action::Forward {
                assert!(compose.receiver.text().is_empty());
                assert!(compose.cc.text().is_empty());
                assert_eq!(compose.subject.text(), "Fwd: Plans");
                assert_eq!(compose.attachments.names(), ["report.pdf"]);
                compose.receiver.set_text("forward@example.com");
            } else {
                assert!(compose.receiver.text().contains("support@example.com"));
                assert!(!compose.receiver.text().contains("sender@example.com"));
                assert_eq!(compose.subject.text(), "Re: Plans");
                assert!(compose.attachments.names().is_empty());
                if action == Action::ReplyAll {
                    assert_eq!(
                        compose.receiver.text(),
                        "support@example.com, team@example.com"
                    );
                    assert_eq!(compose.cc.text(), "copy@example.com");
                } else {
                    assert!(compose.cc.text().is_empty());
                }
            }
            let submitted = Rc::new(RefCell::new(None));
            let draft = submitted.clone();
            compose.connect_send(move || {
                let draft = draft.clone();
                Box::new(move |result| *draft.borrow_mut() = Some(result.unwrap()))
            });
            compose.send.emit_clicked();
            let context = gtk::glib::MainContext::default();
            while context.pending() {
                context.iteration(false);
            }
            let draft = submitted.borrow_mut().take().unwrap();
            assert!(draft.text.contains("Original body"));
            if action == Action::Forward {
                assert!(draft.in_reply_to.is_none());
                assert!(draft.references.is_empty());
                assert!(draft.text.contains("From: sender@example.com"));
                let source = draft.attachment_source.unwrap();
                assert_eq!(source.folder, "INBOX");
                assert_eq!(source.uid, 42);
                assert_eq!(source.validity, Some(7));
                assert_eq!(source.message_id, "original@example.com");
            } else {
                assert_eq!(draft.in_reply_to.as_deref(), Some("original@example.com"));
                assert_eq!(
                    draft.references,
                    ["parent@example.com", "original@example.com"]
                );
                assert!(draft.attachment_source.is_none());
            }
        }
    }
}
