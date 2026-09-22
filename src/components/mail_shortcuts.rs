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

/// Fall back to the open conversation when focus is outside a message row.
pub fn attach_open_message(
    window: &impl IsA<gtk::Widget>,
    message_list: &gtk::ListBox,
    compose: &impl IsA<gtk::Widget>,
    activate: impl Fn(SenderAction) -> bool + 'static,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_name(Some("open-message-shortcuts"));
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let list = message_list.downgrade();
    let compose = compose.as_ref().downgrade();
    keys.connect_key_pressed(move |controller, key, _, modifiers| {
        let Some(action) = action(key, modifiers) else {
            return gtk::glib::Propagation::Proceed;
        };
        let mut focus = controller
            .widget()
            .and_then(|widget| widget.root())
            .and_then(|root| root.focus());
        let list = list.upgrade();
        let compose = compose.upgrade();
        while let Some(widget) = focus {
            // Leave text editing, draft controls, context menus, and focused
            // message-row shortcuts to their own handlers.
            if widget.is::<gtk::Editable>()
                || widget.is::<gtk::TextView>()
                || widget.is::<gtk::Popover>()
                || compose.as_ref() == Some(&widget)
                || (widget.is::<gtk::ListBoxRow>()
                    && widget.parent().as_ref() == list.as_ref().map(|list| list.upcast_ref()))
            {
                return gtk::glib::Propagation::Proceed;
            }
            focus = widget.parent();
        }
        if activate(action) {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    #[ignore = "Requires a graphical session; constructs shortcut focus targets"]
    fn open_message_shortcuts_respect_editors_and_row_handlers() {
        gtk::init().unwrap();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let list = gtk::ListBox::new();
        list.append(&gtk::Label::new(Some("Message")));
        let reader = gtk::Button::with_label("Reader control");
        let entry = gtk::Entry::new();
        let text = gtk::TextView::new();
        let compose = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let draft_control = gtk::Button::with_label("Draft control");
        compose.append(&draft_control);
        for widget in [
            list.upcast_ref::<gtk::Widget>(),
            reader.upcast_ref(),
            entry.upcast_ref(),
            text.upcast_ref(),
            compose.upcast_ref(),
        ] {
            content.append(widget);
        }
        let window = gtk::Window::builder().child(&content).build();
        let called = Rc::new(Cell::new(0));
        let recorder = called.clone();
        attach_open_message(&window, &list, &compose, move |_| {
            recorder.set(recorder.get() + 1);
            true
        });
        let keys = window
            .observe_controllers()
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|controller| controller.name().as_deref() == Some("open-message-shortcuts"))
            .unwrap();
        let press =
            |key, modifiers| keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers]);
        for key in [
            gtk::gdk::Key::Delete,
            gtk::gdk::Key::BackSpace,
            gtk::gdk::Key::KP_Delete,
        ] {
            reader.grab_focus();
            assert!(press(key, gtk::gdk::ModifierType::empty()));
            assert!(!press(key, gtk::gdk::ModifierType::CONTROL_MASK));
            for widget in [
                entry.upcast_ref::<gtk::Widget>(),
                text.upcast_ref(),
                draft_control.upcast_ref(),
                list.row_at_index(0).unwrap().upcast_ref(),
            ] {
                widget.grab_focus();
                assert!(!press(key, gtk::gdk::ModifierType::empty()));
            }
        }
        assert_eq!(called.get(), 3);
        window.close();
    }
}
