use crate::models::sender_action::SenderAction;
use adw::prelude::*;

pub fn attach(list: &gtk::ListBox, activate: impl Fn(i32, SenderAction) -> bool + 'static) {
    let keys = gtk::EventControllerKey::new();
    keys.set_name(Some("mail-row-shortcuts"));
    let weak = list.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(action) = action(key, modifiers) else {
            return gtk::glib::Propagation::Proceed;
        };
        let Some(list) = weak.upgrade() else {
            return gtk::glib::Propagation::Proceed;
        };
        let row = list.root().and_then(|root| root.focus()).and_then(|focus| {
            focus
                .clone()
                .downcast::<gtk::ListBoxRow>()
                .ok()
                .or_else(|| {
                    focus
                        .ancestor(gtk::ListBoxRow::static_type())?
                        .downcast()
                        .ok()
                })
        });
        if let Some(row) = row
            && row.parent().as_ref() == Some(list.upcast_ref())
            && activate(row.index(), action)
        {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    list.add_controller(keys);
}

fn action(key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> Option<SenderAction> {
    if modifiers.intersects(gtk::accelerator_get_default_mod_mask()) {
        return None;
    }
    match key {
        gtk::gdk::Key::Delete | gtk::gdk::Key::KP_Delete => Some(SenderAction::Delete),
        gtk::gdk::Key::BackSpace => Some(SenderAction::Archive),
        _ => None,
    }
}
