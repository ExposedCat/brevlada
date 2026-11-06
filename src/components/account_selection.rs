use crate::models::Account;
use adw::prelude::*;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

type Changed = Box<dyn Fn()>;

pub struct AccountSelection {
    pub row: adw::ActionRow,
    button: gtk::MenuButton,
    content: gtk::Box,
    all: gtk::CheckButton,
    accounts: RefCell<Vec<Account>>,
    selected: RefCell<Option<HashSet<String>>>,
    checks: RefCell<Vec<(String, gtk::CheckButton)>>,
    callbacks: RefCell<Vec<Changed>>,
    updating: std::cell::Cell<bool>,
}

impl AccountSelection {
    pub fn new() -> Rc<Self> {
        let row = adw::ActionRow::builder()
            .title("Accounts")
            .subtitle_lines(1)
            .build();
        let content = super::column("account-selection-content");
        for margin in ["margin-top", "margin-bottom", "margin-start", "margin-end"] {
            content.set_property(margin, crate::theme::SPACING);
        }
        content.set_spacing(crate::theme::SPACING);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(crate::theme::ACCOUNT_SELECTION_HEIGHT)
            .build();
        let popover = gtk::Popover::builder().child(&scroll).build();
        let button = gtk::MenuButton::builder()
            .label("Select accounts")
            .popover(&popover)
            .valign(gtk::Align::Center)
            .build();
        row.add_suffix(&button);
        let all = gtk::CheckButton::with_label("All accounts");
        let ui = Rc::new(Self {
            row,
            button,
            content,
            all,
            accounts: RefCell::default(),
            selected: RefCell::default(),
            checks: RefCell::default(),
            callbacks: RefCell::default(),
            updating: std::cell::Cell::new(false),
        });
        let weak = Rc::downgrade(&ui);
        ui.all.connect_toggled(move |check| {
            if let Some(ui) = weak.upgrade()
                && !ui.updating.get()
            {
                *ui.selected.borrow_mut() = Some(if check.is_active() {
                    ui.accounts
                        .borrow()
                        .iter()
                        .map(|account| account.email.clone())
                        .collect()
                } else {
                    HashSet::new()
                });
                ui.changed();
            }
        });
        ui
    }

    pub fn set_accounts(self: &Rc<Self>, accounts: Vec<Account>, current: Option<&str>) {
        {
            let mut saved = self.selected.borrow_mut();
            let selected = saved.get_or_insert_with(|| {
                current
                    .or_else(|| accounts.first().map(|account| account.email.as_str()))
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            });
            selected.retain(|email| accounts.iter().any(|account| &account.email == email));
        }
        *self.accounts.borrow_mut() = accounts;
        self.updating.set(true);
        super::clear(&self.content);
        self.checks.borrow_mut().clear();
        self.content.append(&self.all);
        for account in self.accounts.borrow().iter() {
            let check = gtk::CheckButton::with_label(&account.email);
            let weak = Rc::downgrade(self);
            let email = account.email.clone();
            check.connect_toggled(move |check| {
                if let Some(ui) = weak.upgrade()
                    && !ui.updating.get()
                {
                    {
                        let mut saved = ui.selected.borrow_mut();
                        let selected = saved.as_mut().unwrap();
                        if check.is_active() {
                            selected.insert(email.clone());
                        } else {
                            selected.remove(&email);
                        }
                    }
                    ui.changed();
                }
            });
            self.content.append(&check);
            self.checks
                .borrow_mut()
                .push((account.email.clone(), check));
        }
        self.update();
    }

    pub fn selected_accounts(&self) -> Vec<Account> {
        let selected = self.selected.borrow();
        self.accounts
            .borrow()
            .iter()
            .filter(|account| {
                selected
                    .as_ref()
                    .is_some_and(|selected| selected.contains(&account.email))
            })
            .cloned()
            .collect()
    }

    pub fn connect_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }

    fn changed(&self) {
        self.update();
        for callback in self.callbacks.borrow().iter() {
            callback();
        }
    }

    fn update(&self) {
        self.updating.set(true);
        let selected = self.selected_accounts();
        self.button
            .set_sensitive(!self.accounts.borrow().is_empty());
        self.button
            .set_label(&format!("{} selected", selected.len()));
        let names = selected
            .iter()
            .map(|account| account.email.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.row.set_subtitle(&names);
        self.row
            .set_tooltip_text((!names.is_empty()).then_some(names.as_str()));
        let all = !selected.is_empty() && selected.len() == self.accounts.borrow().len();
        self.all.set_active(all);
        self.all.set_inconsistent(!selected.is_empty() && !all);
        for (email, check) in self.checks.borrow().iter() {
            check.set_active(selected.iter().any(|account| &account.email == email));
        }
        self.updating.set(false);
    }
}
