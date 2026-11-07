use super::*;
use crate::models::senders;

pub(super) fn pending(button: &gtk::Button, pending: bool) {
    let label = button
        .child()
        .and_then(|content| content.last_child())
        .and_downcast::<gtk::Label>()
        .unwrap();
    label.set_text(if pending {
        "Unsubscribing…"
    } else {
        "Unsubscribe"
    });
    button.set_sensitive(!pending);
}

pub(super) fn with_actions(entry: &Entry, show_accounts: bool) -> (gtk::Box, gtk::FlowBox) {
    let widget = column("subscription-card");
    widget.set_spacing(theme::SPACING);
    let (name, address) = senders::identity(&entry.source.message);
    widget.append(&super::super::card_title(
        "mail-unread-symbolic",
        if name.is_empty() { &address } else { &name },
    ));
    let address_label = label(&address, "dim-label");
    address_label.set_tooltip_text(Some(&address));
    widget.append(&address_label);
    if show_accounts {
        let account = label(&entry.source.account, "dim-label");
        account.add_css_class("caption");
        account.set_tooltip_text(Some(&entry.source.account));
        widget.append(&account);
    }
    let actions = super::super::card_actions();
    widget.append(&actions);
    (widget, actions)
}
