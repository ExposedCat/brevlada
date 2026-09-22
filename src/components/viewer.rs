use super::{avatars::Avatars, button, column, display, horizontal};
use crate::{models::Message, models::senders, theme};
use adw::prelude::*;
use std::{cell::Cell, rc::Rc};

struct Content {
    message: std::cell::RefCell<Message>,
    body: gtk::Revealer,
    expanded: Cell<bool>,
    rendered: Cell<bool>,
    open: Rc<dyn Fn()>,
    reply: Rc<dyn Fn(&Message)>,
    reply_pending: Cell<bool>,
    signals: std::cell::RefCell<Vec<(gtk::Adjustment, gtk::glib::SignalHandlerId)>>,
}

#[derive(Clone)]
pub struct Card {
    pub widget: gtk::Box,
    content: Rc<Content>,
}

impl Card {
    pub fn is_reply_pending(&self) -> bool {
        self.content.reply_pending.get()
    }

    pub fn is_expanded(&self) -> bool {
        self.content.expanded.get()
    }
    pub fn update(&self, message: &Message) {
        *self.content.message.borrow_mut() = message.clone();
        self.content.show();
        if message.body_loaded && self.content.reply_pending.replace(false) {
            (self.content.reply)(message);
        }
    }
    pub fn error(&self, error: &str) {
        self.content.reply_pending.set(false);
        if !self.content.message.borrow().body_loaded {
            self.content
                .body
                .set_child(Some(&super::body::error(error, self.content.open.clone())));
        }
    }
    pub fn loading(&self) {
        if !self.content.message.borrow().body_loaded {
            self.content.body.set_child(Some(&super::body::loading()));
        }
    }
}

impl Content {
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
        if self.expanded.get() && self.message.borrow().body_loaded && !self.rendered.replace(true)
        {
            self.body
                .set_child(Some(&super::body::view(&self.message.borrow())));
        }
    }
}

pub fn card(
    message: &Message,
    expanded: bool,
    threaded: bool,
    avatars: &Avatars,
    open: impl Fn() + 'static,
    reply: impl Fn(&Message) + 'static,
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
    let content = Rc::new(Content {
        message: std::cell::RefCell::new(message.clone()),
        body: body.clone(),
        expanded: Cell::new(expanded || !threaded),
        rendered: Cell::new(false),
        open: Rc::new(open),
        reply: Rc::new(reply),
        reply_pending: Cell::new(false),
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
        .title(display::sender_name(message))
        .use_markup(false)
        .hexpand(true)
        .build();
    if !name.is_empty() && !email.is_empty() {
        row.set_subtitle(&email);
    }
    let reply_button = button("mail-reply-sender-symbolic", "Reply");
    reply_button.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(&content);
    reply_button.connect_clicked(move |_| {
        if let Some(content) = weak.upgrade() {
            if content.message.borrow().body_loaded {
                (content.reply)(&content.message.borrow());
            } else {
                content.reply_pending.set(true);
                (content.open)();
            }
        }
    });
    row.add_suffix(&reply_button);
    let date = gtk::Label::builder()
        .label(display::date(message, true))
        .halign(gtk::Align::End)
        .css_classes(["message-row-date"])
        .build();
    row.add_suffix(&date);
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
