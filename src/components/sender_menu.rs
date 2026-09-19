use crate::models::sender_action::SenderAction;
use adw::prelude::*;
use std::rc::Rc;

pub fn attach(
    list: &gtk::ListBox,
    sender_at: impl Fn(i32) -> Option<String> + 'static,
    activate: impl Fn(String, SenderAction) + 'static,
) {
    let activate = Rc::new(activate);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = list.downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(list) = weak.upgrade() else { return };
        let Some(row) = list.row_at_y(y as i32) else {
            return;
        };
        let Some(sender) = sender_at(row.index()) else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let menu = gtk::gio::Menu::new();
        let actions = gtk::gio::SimpleActionGroup::new();
        for (index, (action, title)) in SenderAction::ALL.iter().enumerate() {
            let name = format!("action{index}");
            menu.append(Some(title), Some(&format!("sender.{name}")));
            let item = gtk::gio::SimpleAction::new(&name, None);
            let activate = activate.clone();
            let sender = sender.clone();
            let action = *action;
            item.connect_activate(move |_, _| activate(sender.clone(), action));
            actions.add_action(&item);
        }
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("sender", Some(&actions));
        popover.set_has_arrow(false);
        popover.set_parent(&list);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|popover| popover.unparent());
        popover.popup();
    });
    list.add_controller(click);
}
