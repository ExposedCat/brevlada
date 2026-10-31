use adw::prelude::*;
use gtk::gio;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub struct AccountSwitcher {
    pub widget: gtk::MenuButton,
    pub title: gtk::Label,
    menu: gio::Menu,
    action: gio::SimpleAction,
    targets: Rc<RefCell<HashMap<String, gtk::Button>>>,
}

impl AccountSwitcher {
    pub fn new() -> Self {
        let title = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(40)
            .css_classes(["title"])
            .build();
        let menu = gio::Menu::new();
        let action = gio::SimpleAction::new_stateful(
            "select",
            Some(gtk::glib::VariantTy::STRING),
            &"".to_variant(),
        );
        let targets = Rc::new(RefCell::new(HashMap::<String, gtk::Button>::new()));
        let selected = targets.clone();
        action.connect_activate(move |_, target| {
            let button = target
                .and_then(|target| target.str())
                .and_then(|email| selected.borrow().get(email).cloned());
            if let Some(button) = button {
                button.emit_clicked();
            }
        });
        let actions = gio::SimpleActionGroup::new();
        actions.add_action(&action);
        let widget = gtk::MenuButton::builder()
            .child(&title)
            .always_show_arrow(true)
            .menu_model(&menu)
            .tooltip_text("Switch account")
            .css_classes(["flat"])
            .visible(false)
            .build();
        widget.insert_action_group("account", Some(&actions));
        Self {
            widget,
            title,
            menu,
            action,
            targets,
        }
    }

    pub fn set_accounts(&self, accounts: Vec<(String, gtk::Button)>) {
        self.menu.remove_all();
        let mut targets = self.targets.borrow_mut();
        targets.clear();
        for (email, button) in accounts {
            let item = gio::MenuItem::new(Some(&email), None);
            item.set_action_and_target_value(Some("account.select"), Some(&email.to_variant()));
            self.menu.append_item(&item);
            targets.insert(email, button);
        }
    }

    pub fn set_title(&self, title: &str, visible: bool) {
        self.title.set_label(title);
        self.title
            .set_tooltip_text((!title.is_empty()).then_some(title));
        self.action.set_state(&title.to_variant());
        self.widget.set_visible(visible && !title.is_empty());
    }
}
