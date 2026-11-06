use adw::prelude::*;
use gtk::gio;

pub struct SortMenu {
    pub widget: gtk::MenuButton,
    action: gio::SimpleAction,
    only_unread: gio::SimpleAction,
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
        let only_unread = gio::SimpleAction::new_stateful("only-unread", None, &false.to_variant());
        only_unread.connect_activate(|action, _| {
            let active = action
                .state()
                .and_then(|state| state.get::<bool>())
                .unwrap();
            action.set_state(&(!active).to_variant());
        });
        actions.add_action(&only_unread);
        let menu = gio::Menu::new();
        menu.append(Some("Sort unread first"), Some("sort.unread-first"));
        menu.append(Some("Only unread"), Some("sort.only-unread"));
        let widget = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Sorting and filtering options")
            .menu_model(&menu)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        widget.insert_action_group("sort", Some(&actions));
        Self {
            widget,
            action,
            only_unread,
        }
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
        let changed = std::rc::Rc::new(changed);
        for action in [&self.action, &self.only_unread] {
            let changed = changed.clone();
            action.connect_state_notify(move |_| changed());
        }
    }

    pub fn only_unread(&self) -> bool {
        self.only_unread
            .state()
            .and_then(|state| state.get::<bool>())
            .unwrap()
    }

    pub fn set_only_unread(&self, active: bool) {
        self.only_unread.set_state(&active.to_variant());
    }
}
