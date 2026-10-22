use crate::models::Account;
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct Sender {
    pub widget: gtk::DropDown,
    accounts: RefCell<Vec<Account>>,
    updating: Rc<Cell<bool>>,
}

impl Sender {
    pub fn new() -> Self {
        let widget = gtk::DropDown::from_strings(&[]);
        widget.set_tooltip_text(Some("From"));
        widget.set_valign(gtk::Align::Center);
        widget.add_css_class("compose-from");
        Self {
            widget,
            accounts: RefCell::default(),
            updating: Rc::default(),
        }
    }

    pub fn set_accounts(&self, accounts: &[Account], selected: &str) {
        self.updating.set(true);
        let model = gtk::StringList::new(
            &accounts
                .iter()
                .map(|account| account.email.as_str())
                .collect::<Vec<_>>(),
        );
        let selected = accounts
            .iter()
            .position(|account| account.email.eq_ignore_ascii_case(selected))
            .map_or(gtk::INVALID_LIST_POSITION, |index| index as u32);
        *self.accounts.borrow_mut() = accounts.to_vec();
        self.widget.set_model(Some(&model));
        self.widget.set_selected(selected);
        self.updating.set(false);
    }

    pub fn connect_changed(&self, callback: impl Fn() + 'static) {
        let updating = self.updating.clone();
        self.widget.connect_selected_notify(move |_| {
            if !updating.get() {
                callback();
            }
        });
    }

    pub fn account(&self) -> Option<Account> {
        self.accounts
            .borrow()
            .get(self.widget.selected() as usize)
            .cloned()
    }
}
