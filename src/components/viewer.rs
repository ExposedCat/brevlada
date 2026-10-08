use super::{avatars::Avatars, button, column, display, horizontal};
use crate::{
    models::response::Action,
    models::{Account, Message},
    models::{calendar, senders},
    theme,
};
use adw::prelude::*;
use std::{cell::Cell, rc::Rc};

type Respond = dyn Fn(&Message, Action);

struct Content {
    message: std::cell::RefCell<Message>,
    body: gtk::Revealer,
    webview: std::cell::RefCell<Option<webkit6::WebView>>,
    expanded: Cell<bool>,
    rendered: Cell<bool>,
    media: Cell<bool>,
    has_media: Cell<bool>,
    trusted: Cell<bool>,
    hide_quotes: bool,
    threaded: bool,
    attachment_account: Option<Account>,
    attachment_folder: String,
    star: gtk::Image,
    spam: gtk::Image,
    status_icons: gtk::Box,
    media_button: gtk::Button,
    unsubscribe_button: gtk::Button,
    on_media: Rc<dyn Fn()>,
    open: Rc<dyn Fn()>,
    respond: Rc<Respond>,
    unsubscribe: Rc<dyn Fn(&Message)>,
    response_pending: std::cell::RefCell<Vec<Action>>,
    signals: std::cell::RefCell<Vec<(gtk::Adjustment, gtk::glib::SignalHandlerId)>>,
}

#[derive(Clone)]
pub struct Card {
    pub widget: gtk::Box,
    content: Rc<Content>,
}

impl Card {
    pub fn finish_unsubscribe(&self, done: bool) {
        self.content.unsubscribe_button.set_label(if done {
            "Unsubscribed"
        } else {
            "Unsubscribe"
        });
        self.content.unsubscribe_button.set_sensitive(!done);
    }

    pub fn set_trusted(&self, trusted: bool) {
        self.content.trusted.set(trusted);
        if trusted && self.content.rendered.get() {
            self.content.download_media();
        }
        self.content.update_media_button();
    }

    pub fn is_response_pending(&self) -> bool {
        !self.content.response_pending.borrow().is_empty()
    }

    pub fn is_expanded(&self) -> bool {
        self.content.expanded.get()
    }
    pub fn layout_matches(&self, message: &Message, threaded: bool, hide_quotes: bool) -> bool {
        let current = self.content.message.borrow();
        current.uid == message.uid
            && current.message_id == message.message_id
            && current.is_draft == message.is_draft
            && self.content.threaded == threaded
            && self.content.hide_quotes == hide_quotes
    }

    pub fn update(&self, message: &Message) {
        let changed = {
            let old = self.content.message.borrow();
            old.body_loaded
                && (old.body_html != message.body_html
                    || old.body_text != message.body_text
                    || calendar::is_reply(&old) != calendar::is_reply(message)
                    || old.attachments != message.attachments
                    || old.attachment_details != message.attachment_details)
        };
        let state_changed = {
            let old = self.content.message.borrow();
            old.body_loaded != message.body_loaded
                || (!old.body_loaded && old.is_spam != message.is_spam)
        };
        let inline_changed =
            self.content.message.borrow().inline_media_loaded != message.inline_media_loaded;
        *self.content.message.borrow_mut() = message.clone();
        self.content.star.set_visible(message.is_flagged);
        self.content.spam.set_visible(message.is_spam);
        self.content
            .status_icons
            .set_visible(message.is_flagged || message.is_spam);
        self.content
            .has_media
            .set(!calendar::is_reply(message) && super::html::has_remote_media(message));
        self.content.update_media_button();
        self.content
            .unsubscribe_button
            .set_visible(message.unsubscribe.is_some());
        if changed
            || state_changed
            || (inline_changed && self.content.media.get() && message.body_html.contains("cid:"))
        {
            self.content.rendered.set(false);
            self.content.webview.borrow_mut().take();
        }
        self.content.show();
        if message.body_loaded {
            let actions = self.content.response_pending.take();
            for action in actions {
                (self.content.respond)(message, action);
            }
        }
    }
    pub fn error(&self, error: &str) {
        if !self.content.message.borrow().body_loaded {
            self.content.webview.borrow_mut().take();
            self.content
                .body
                .set_child(Some(&super::body::error(error, self.content.open.clone())));
        }
    }
    pub fn loading(&self) {
        if !self.content.message.borrow().body_loaded {
            self.content.webview.borrow_mut().take();
            self.content.body.set_child(Some(&super::body::loading()));
        }
    }
}

impl Content {
    fn update_media_button(&self) {
        self.media_button
            .set_visible(!self.trusted.get() && !self.media.get() && self.has_media.get());
    }

    fn download_media(&self) {
        if self.media.replace(true) {
            return;
        }
        self.media_button.set_visible(false);
        (self.on_media)();
        if self.message.borrow().body_html.contains("cid:") {
            if self.message.borrow().inline_media_loaded {
                self.rendered.set(false);
                self.webview.borrow_mut().take();
                self.show();
            }
        } else if let Some(view) = self.webview.borrow().as_ref() {
            super::body::enable_media(view);
        }
    }

    fn show(&self) {
        if !self.body.is_mapped() {
            return;
        }
        if let Some(scroll) = self
            .body
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
        {
            let Some(bounds) = self.body.compute_bounds(&scroll) else {
                return;
            };
            if bounds.y() > scroll.height() as f32 || bounds.y() + bounds.height() < 0.0 {
                return;
            }
        }
        if self.expanded.get()
            && self.message.borrow().is_spam
            && !self.message.borrow().body_loaded
        {
            if !self.rendered.replace(true) {
                let notice = gtk::Label::builder()
                    .label("Marked as spam. Use “Not spam” to restore this message.")
                    .wrap(true)
                    .xalign(0.0)
                    .margin_top(theme::SPACING)
                    .margin_bottom(theme::SPACING)
                    .css_classes(["dim-label"])
                    .build();
                self.body.set_child(Some(&notice));
            }
            return;
        }
        if self.expanded.get() && self.message.borrow().body_loaded && !self.rendered.replace(true)
        {
            // A reply can arrive before its invitation. Never create a body
            // renderer while waiting for the thread to compact it into attendance.
            if calendar::is_reply(&self.message.borrow()) {
                self.body.set_child(Some(&super::attachments::view(
                    &self.message.borrow(),
                    self.attachment_account.as_ref(),
                    &self.attachment_folder,
                )));
                self.webview.borrow_mut().take();
                return;
            }
            if self.trusted.get() {
                self.media.set(true);
                self.media_button.set_visible(false);
                (self.on_media)();
            }
            let (body, view) =
                super::body::view(&self.message.borrow(), self.media.get(), self.hide_quotes);
            let content = column("message-content");
            content.append(&body);
            content.append(&super::attachments::view(
                &self.message.borrow(),
                self.attachment_account.as_ref(),
                &self.attachment_folder,
            ));
            self.body.set_child(Some(&content));
            *self.webview.borrow_mut() = Some(view);
        }
    }
}

pub fn card(
    message: &Message,
    expanded: bool,
    threaded: bool,
    sent: bool,
    hide_quotes: bool,
    avatars: &Avatars,
    media_downloaded: bool,
    trusted: bool,
    attachment_account: Option<Account>,
    attachment_folder: String,
    open: impl Fn() + 'static,
    respond: impl Fn(&Message, Action) + 'static,
    download_media: impl Fn() + 'static,
    unsubscribe: impl Fn(&Message) + 'static,
) -> Card {
    let widget = column("message-row-widget");
    widget.set_vexpand(false);
    widget.set_valign(gtk::Align::Start);
    let header = horizontal("message-header", theme::SMALL_SPACING);
    let body = gtk::Revealer::builder()
        .vexpand(false)
        .reveal_child(expanded || !threaded)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(160)
        .build();
    body.set_child(Some(&super::body::loading()));
    let media_button = button("image-x-generic-symbolic", "Download media");
    let unsubscribe_button = gtk::Button::with_label("Unsubscribe");
    unsubscribe_button.add_css_class("flat");
    unsubscribe_button.set_valign(gtk::Align::Center);
    unsubscribe_button.set_visible(message.unsubscribe.is_some());
    media_button.set_valign(gtk::Align::Center);
    let has_media = !calendar::is_reply(message) && super::html::has_remote_media(message);
    media_button.set_visible(!media_downloaded && !trusted && has_media);
    let star = gtk::Image::from_icon_name("starred-symbolic");
    star.add_css_class("message-row-flag-icon");
    star.set_halign(gtk::Align::End);
    star.set_visible(message.is_flagged);
    star.set_tooltip_text(Some("Starred"));
    let spam = gtk::Image::from_icon_name("mail-mark-junk-symbolic");
    spam.add_css_class("message-row-spam-icon");
    spam.set_visible(message.is_spam);
    spam.set_tooltip_text(Some("Spam"));
    let icons = horizontal("message-row-icons", theme::ROW_GAP);
    icons.set_halign(gtk::Align::End);
    icons.set_visible(message.is_flagged || message.is_spam);
    icons.append(&spam);
    icons.append(&star);
    let content = Rc::new(Content {
        message: std::cell::RefCell::new(message.clone()),
        body: body.clone(),
        webview: std::cell::RefCell::new(None),
        expanded: Cell::new(expanded || !threaded),
        rendered: Cell::new(false),
        media: Cell::new(media_downloaded),
        has_media: Cell::new(has_media),
        trusted: Cell::new(trusted),
        hide_quotes,
        threaded,
        attachment_account,
        attachment_folder,
        star: star.clone(),
        spam: spam.clone(),
        status_icons: icons.clone(),
        media_button: media_button.clone(),
        unsubscribe_button: unsubscribe_button.clone(),
        on_media: Rc::new(download_media),
        open: Rc::new(open),
        respond: Rc::new(respond),
        unsubscribe: Rc::new(unsubscribe),
        response_pending: std::cell::RefCell::default(),
        signals: std::cell::RefCell::new(Vec::new()),
    });
    let weak = Rc::downgrade(&content);
    body.connect_map(move |body| {
        if let Some(content) = weak.upgrade() {
            if content.signals.borrow().is_empty()
                && let Some(scroll) = body
                    .ancestor(gtk::ScrolledWindow::static_type())
                    .and_downcast::<gtk::ScrolledWindow>()
            {
                let adjustment = scroll.vadjustment();
                let weak = Rc::downgrade(&content);
                let changed = adjustment.connect_changed(move |_| {
                    if let Some(content) = weak.upgrade() {
                        content.show();
                    }
                });
                let weak = Rc::downgrade(&content);
                let moved = adjustment.connect_value_changed(move |_| {
                    if let Some(content) = weak.upgrade() {
                        content.show();
                    }
                });
                content
                    .signals
                    .borrow_mut()
                    .extend([(adjustment.clone(), changed), (adjustment, moved)]);
            }
            let weak = Rc::downgrade(&content);
            gtk::glib::idle_add_local_once(move || {
                if let Some(content) = weak.upgrade() {
                    content.show();
                }
            });
        }
    });
    if threaded {
        let expand = button(
            if content.expanded.get() {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            },
            "Expand message",
        );
        expand.add_css_class("message-expand");
        expand.set_valign(gtk::Align::Center);
        let weak = Rc::downgrade(&content);
        expand.connect_clicked(move |button| {
            if let Some(content) = weak.upgrade() {
                let active = !content.expanded.get();
                content.expanded.set(active);
                content.body.set_reveal_child(active);
                button.set_icon_name(if active {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                });
                if active {
                    content.show();
                    (content.open)();
                }
            }
        });
        header.append(&expand);
    }
    let (name, email) = display::sender(message);
    let row = adw::ActionRow::builder()
        .title(if sent || message.is_draft {
            format!("To: {}", message.recipients)
        } else {
            display::sender_name(message)
        })
        .use_markup(false)
        .hexpand(true)
        .build();
    if message.is_draft {
        row.set_subtitle("Draft");
    } else if sent {
        row.set_subtitle("Sent");
    } else if !name.is_empty() && !email.is_empty() {
        row.set_subtitle(&email);
    }
    for (icon, label, action) in [
        ("mail-reply-sender-symbolic", "Reply", Action::Reply),
        ("mail-reply-all-symbolic", "Reply All", Action::ReplyAll),
        ("mail-forward-symbolic", "Forward", Action::Forward),
    ] {
        let button = button(icon, label);
        button.set_valign(gtk::Align::Center);
        let weak = Rc::downgrade(&content);
        button.connect_clicked(move |_| {
            if let Some(content) = weak.upgrade() {
                if content.message.borrow().body_loaded {
                    (content.respond)(&content.message.borrow(), action);
                } else {
                    content.response_pending.borrow_mut().push(action);
                    (content.open)();
                }
            }
        });
        row.add_suffix(&button);
    }
    let weak = Rc::downgrade(&content);
    media_button.connect_clicked(move |_| {
        if let Some(content) = weak.upgrade() {
            content.download_media();
        }
    });
    row.add_suffix(&media_button);
    let date = gtk::Label::builder()
        .label(display::date(message, true))
        .halign(gtk::Align::End)
        .css_classes(["message-row-date"])
        .build();
    let metadata = horizontal("message-row-right", theme::ROW_GAP);
    metadata.set_valign(gtk::Align::Center);
    metadata.append(&date);
    metadata.append(&icons);
    row.add_suffix(&metadata);
    {
        let weak = Rc::downgrade(&content);
        unsubscribe_button.connect_clicked(move |button| {
            let Some(content) = weak.upgrade() else {
                return;
            };
            if content.message.borrow().unsubscribe.is_none() {
                return;
            }
            button.set_sensitive(false);
            button.set_label("Unsubscribing…");
            (content.unsubscribe)(&content.message.borrow());
        });
        row.add_suffix(&unsubscribe_button);
    }
    let avatar = adw::Avatar::new(
        theme::AVATAR_SIZE,
        Some(&display::sender_name(message)),
        true,
    );
    avatar.add_css_class("message-avatar");
    avatars.attach(&avatar, &senders::key(message));
    row.add_prefix(&avatar);
    header.append(&row);
    widget.append(&header);
    widget.append(&body);
    content.show();
    Card { widget, content }
}

impl Drop for Content {
    fn drop(&mut self) {
        for (adjustment, signal) in self.signals.get_mut().drain(..) {
            adjustment.disconnect(signal);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; maps a message card"]
    fn rsvp_body_never_creates_a_webview_before_its_invitation_loads() {
        gtk::init().unwrap();
        let mut message = Message {
            body_text: "Accepted: Lunch".into(),
            body_html: "<p>Accepted: Lunch</p><img src='https://example.com/image'>".into(),
            ..Default::default()
        };
        let card = card(
            &message,
            true,
            false,
            false,
            false,
            &Avatars::new(|_| {}),
            false,
            true,
            None,
            String::new(),
            || {},
            |_, _| {},
            || panic!("RSVP bodies must not request media"),
            |_| {},
        );
        let window = gtk::Window::builder().child(&card.widget).build();
        window.present();
        let context = gtk::glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        assert!(card.content.body.is_mapped());
        assert!(card.content.webview.borrow().is_none());

        // The RSVP body arrives first; no invitation or history is available.
        message.body_loaded = true;
        message.calendar_events = vec![calendar::Event {
            method: Some("REPLY".into()),
            ..Default::default()
        }];
        card.update(&message);
        card.content.show();
        assert!(card.content.rendered.get());
        assert!(card.content.webview.borrow().is_none());
        assert!(!card.content.has_media.get());
        assert!(!card.content.media_button.is_visible());

        // Known replies also skip rendering when the card is initially mapped.
        let known = super::card(
            &message,
            true,
            false,
            false,
            false,
            &Avatars::new(|_| {}),
            false,
            true,
            None,
            String::new(),
            || {},
            |_, _| {},
            || panic!("RSVP bodies must not request media"),
            |_| {},
        );
        window.set_child(Some(&known.widget));
        while context.pending() {
            context.iteration(false);
        }
        known.content.show();
        assert!(known.content.rendered.get());
        assert!(known.content.webview.borrow().is_none());
        window.close();
    }
}
