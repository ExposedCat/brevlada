use adw::prelude::*;
use gtk::gio;

pub struct SortMenu {
    pub widget: gtk::MenuButton,
    action: gio::SimpleAction,
}

impl Default for SortMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl SortMenu {
    pub fn new() -> Self {
        let action = gio::SimpleAction::new_stateful("unread-first", None, &true.to_variant());
        action.connect_activate(|action, _| {
            let active = action
                .state()
                .and_then(|state| state.get::<bool>())
                .unwrap();
            action.set_state(&(!active).to_variant());
        });
        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&action);
        let menu = gio::Menu::new();
        menu.append(Some("Sort unread first"), Some("sort.unread-first"));
        let widget = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Sorting options")
            .menu_model(&menu)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        widget.insert_action_group("sort", Some(&actions));
        Self { widget, action }
    }

    pub fn unread_first(&self) -> bool {
        self.action
            .state()
            .and_then(|state| state.get::<bool>())
            .unwrap()
    }

    pub fn set_unread_first(&self, active: bool) {
        self.action.set_state(&active.to_variant());
    }

    pub fn connect_changed(&self, changed: impl Fn() + 'static) {
        self.action.connect_state_notify(move |_| changed());
    }
}
