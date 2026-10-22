use crate::models::{Account, settings::AccountFolders};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct Accounts {
    pub page: adw::PreferencesPage,
    pub account: adw::ComboRow,
    pub archive: adw::ComboRow,
    pub share: adw::SwitchRow,
    pub apply: gtk::Button,
    pub reload: gtk::Button,
    details: adw::PreferencesGroup,
    status: gtk::Label,
    accounts: RefCell<Vec<Account>>,
    config: RefCell<Option<AccountFolders>>,
    choices: RefCell<Vec<Option<String>>>,
    pub updating: Cell<bool>,
    request: Cell<u64>,
}

impl Accounts {
    pub fn new() -> Rc<Self> {
        let page = adw::PreferencesPage::builder().title("Accounts").build();
        let selection = adw::PreferencesGroup::new();
        let account = adw::ComboRow::builder()
            .title("Account")
            .enable_search(true)
            .build();
        let reload = super::button("view-refresh-symbolic", "Reload account settings");
        account.add_suffix(&reload);
        selection.add(&account);
        page.add(&selection);
        let details = adw::PreferencesGroup::builder()
            .title("Folders")
            .description(
                "Choose where Brevlada archives messages. Existing messages stay where they are.",
            )
            .sensitive(false)
            .build();
        let archive = adw::ComboRow::builder()
            .title("Archive folder")
            .enable_search(true)
            .use_subtitle(true)
            .build();
        details.add(&archive);
        let share = adw::SwitchRow::builder()
            .title("Share with other mail apps")
            .build();
        details.add(&share);
        page.add(&details);
        let status = super::label("Choose an account to configure its folders.", "dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::None);
        status.set_wrap(true);
        status.set_margin_top(crate::theme::SPACING);
        let actions = adw::PreferencesGroup::new();
        actions.add(&status);
        let apply = gtk::Button::builder()
            .label("Apply")
            .halign(gtk::Align::End)
            .margin_top(crate::theme::SPACING)
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();
        actions.add(&apply);
        page.add(&actions);
        let ui = Rc::new(Self {
            page,
            account,
            archive,
            share,
            apply,
            reload,
            details,
            status,
            accounts: RefCell::default(),
            config: RefCell::default(),
            choices: RefCell::default(),
            updating: Cell::new(false),
            request: Cell::new(0),
        });
        let weak = Rc::downgrade(&ui);
        ui.archive.connect_selected_notify(move |_| {
            if let Some(ui) = weak.upgrade()
                && !ui.updating.get()
            {
                ui.changed();
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.share.connect_active_notify(move |_| {
            if let Some(ui) = weak.upgrade()
                && !ui.updating.get()
            {
                ui.changed();
            }
        });
        ui
    }

    pub fn set_accounts(&self, accounts: Vec<Account>, current: Option<&str>) {
        let selected = self.selected_account().map(|account| account.email);
        let selected = selected.as_deref().or(current);
        self.updating.set(true);
        self.account.set_model(Some(&gtk::StringList::new(
            &accounts
                .iter()
                .map(|account| account.email.as_str())
                .collect::<Vec<_>>(),
        )));
        let position = accounts
            .iter()
            .position(|account| Some(account.email.as_str()) == selected)
            .unwrap_or(0);
        self.account.set_selected(if accounts.is_empty() {
            gtk::INVALID_LIST_POSITION
        } else {
            position as u32
        });
        *self.accounts.borrow_mut() = accounts;
        self.updating.set(false);
    }

    pub fn selected_account(&self) -> Option<Account> {
        self.accounts
            .borrow()
            .get(self.account.selected() as usize)
            .cloned()
    }

    pub fn archive_folder(&self) -> Option<String> {
        self.choices
            .borrow()
            .get(self.archive.selected() as usize)
            .cloned()
            .flatten()
    }

    pub fn busy(&self, saving: bool) -> u64 {
        self.request.set(self.request.get().wrapping_add(1));
        self.updating.set(true);
        self.account.set_sensitive(false);
        self.reload.set_sensitive(false);
        self.details.set_sensitive(false);
        self.apply.set_sensitive(false);
        self.status.remove_css_class("error");
        self.status.set_text(if saving {
            "Saving account settings…"
        } else {
            "Loading account settings…"
        });
        self.request.get()
    }

    pub fn accepts(&self, request: u64) -> bool {
        self.request.get() == request
    }

    pub fn loaded(&self, config: AccountFolders, saved: Option<bool>) {
        self.updating.set(true);
        let automatic = config
            .server_archive
            .as_ref()
            .map_or("Automatic (no archive folder found)".into(), |folder| {
                format!("Automatic ({folder})")
            });
        let mut names = vec![automatic];
        let mut choices = vec![None];
        for folder in &config.folders {
            names.push(folder.clone());
            choices.push(Some(folder.clone()));
        }
        if let Some(folder) = &config.archive_folder
            && !config.folders.contains(folder)
        {
            names.push(format!("{folder} (unavailable)"));
            choices.push(Some(folder.clone()));
        }
        let selected = choices
            .iter()
            .position(|folder| folder == &config.archive_folder)
            .unwrap_or(0);
        self.archive.set_model(Some(&gtk::StringList::new(
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )));
        *self.choices.borrow_mut() = choices;
        self.archive.set_selected(selected as u32);
        self.share.set_active(false);
        self.share.set_subtitle(if config.can_share {
            "Ask the server to make this the archive folder for compatible mail apps. The server may decline."
        } else {
            "This server does not support sharing folder changes. Your choice is saved in Brevlada only."
        });
        *self.config.borrow_mut() = Some(config);
        self.account.set_sensitive(true);
        self.reload.set_sensitive(true);
        self.details.set_sensitive(true);
        self.updating.set(false);
        self.changed();
        if let Some(shared) = saved {
            self.status.set_text(if shared { "Saved in Brevlada and on the server. Compatible mail apps can discover the archive folder." } else { "Saved in Brevlada." });
        }
    }

    pub fn failed(&self, error: &str, saving: bool) {
        self.account.set_sensitive(true);
        self.reload.set_sensitive(self.selected_account().is_some());
        self.details.set_sensitive(saving);
        self.updating.set(false);
        self.status.add_css_class("error");
        self.status.set_text(error);
        if saving {
            self.changed_controls();
        }
    }

    pub fn empty(&self) {
        self.request.set(self.request.get().wrapping_add(1));
        self.details.set_sensitive(false);
        self.account.set_sensitive(false);
        self.reload.set_sensitive(false);
        self.apply.set_sensitive(false);
        self.status.set_text(
            "No email accounts available. Add an account in GNOME Online Accounts first.",
        );
    }

    fn changed_controls(&self) {
        let config = self.config.borrow();
        let Some(config) = config.as_ref() else {
            return;
        };
        let folder = self.archive_folder();
        self.archive
            .set_tooltip_text(folder.as_deref().or(config.server_archive.as_deref()));
        let valid = folder
            .as_ref()
            .is_none_or(|folder| config.folders.contains(folder));
        self.share
            .set_sensitive(config.can_share && folder.is_some() && valid);
        self.apply.set_sensitive(
            valid
                && (folder != config.archive_folder
                    || (self.share.is_sensitive() && self.share.is_active())),
        );
    }

    fn changed(&self) {
        self.changed_controls();
        self.status.remove_css_class("error");
        let config = self.config.borrow();
        let Some(config) = config.as_ref() else {
            return;
        };
        let folder = self.archive_folder();
        self.status.set_text(
            if folder
                .as_ref()
                .is_some_and(|folder| !config.folders.contains(folder))
            {
                "The saved archive folder is unavailable. Choose another folder or Automatic."
            } else if self.share.is_sensitive() && self.share.is_active() {
                "Apply will update the server's archive folder for this account."
            } else if folder.is_none() {
                "Automatic follows the archive folder reported by the server."
            } else {
                "This archive folder choice applies to Brevlada."
            },
        );
    }
}
