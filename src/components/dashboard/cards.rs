use super::*;
use crate::models::{
    calendar::Response,
    dashboard::{Content, Source},
    senders,
};

type Respond = Rc<RefCell<Option<Box<dyn Fn(Response)>>>>;

pub struct Tile {
    pub widget: gtk::Box,
    pub open: gtk::Button,
    pub dismiss: gtk::Button,
    pub unsubscribe: Option<gtk::Button>,
    pub entry: Entry,
    pub series_open: Vec<(gtk::Button, Source)>,
    respond: Respond,
}

impl Tile {
    pub fn new(entry: &Entry, show_accounts: bool, pending: bool) -> Self {
        let (name, address) = senders::identity(&entry.source.message);
        let sender = if name.is_empty() { address } else { name };
        let respond: Respond = Rc::default();
        let (widget, actions) = match &entry.content {
            Content::Subscription => super::subscriptions::with_actions(entry, show_accounts),
            Content::Parcel(parcel) => {
                super::super::parcel::with_actions(parcel, entry.source.message.timestamp)
            }
            Content::Event(event) => {
                let callback = respond.clone();
                super::super::calendar::with_actions(
                    event,
                    &entry.source.account,
                    pending,
                    move |response| {
                        if let Some(callback) = callback.borrow().as_ref() {
                            callback(response);
                        }
                    },
                )
            }
            Content::Ticket(reservation) => super::super::ticket::with_actions(reservation),
            Content::Receipt(receipt) => super::super::receipt::with_actions(receipt, &sender),
        };
        widget.add_css_class("dashboard-card");
        widget.set_hexpand(true);
        let unsubscribe = matches!(entry.content, Content::Subscription).then(|| {
            let button = super::super::action_button(
                "list-remove-symbolic",
                if pending {
                    "Unsubscribing…"
                } else {
                    "Unsubscribe"
                },
            );
            button.set_sensitive(!pending);
            super::super::append_action(&actions, &button);
            button
        });
        let open = super::super::action_button("mail-unread-symbolic", "Open email");
        if show_accounts {
            open.set_tooltip_text(Some(&entry.source.account));
        }
        super::super::append_action(&actions, &open);
        let dismiss = button("window-close-symbolic", "Dismiss from dashboard");
        dismiss.set_valign(gtk::Align::Start);
        let header = horizontal("dashboard-card-header", theme::SMALL_SPACING);
        if let Some(first) = widget.first_child() {
            widget.remove(&first);
            first.set_hexpand(true);
            header.append(&first);
        }
        let mut series_open = Vec::new();
        if !entry.series.is_empty() {
            let (series, buttons) = super::series::popover(&entry.series);
            header.append(&series);
            series_open = buttons;
        }
        header.append(&dismiss);
        widget.prepend(&header);
        Self {
            widget,
            open,
            dismiss,
            unsubscribe,
            entry: entry.clone(),
            series_open,
            respond,
        }
    }

    pub fn connect_response(&self, callback: impl Fn(Response) + 'static) {
        *self.respond.borrow_mut() = Some(Box::new(callback));
    }
}
