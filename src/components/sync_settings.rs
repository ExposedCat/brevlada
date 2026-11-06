use super::account_selection::AccountSelection;
use crate::models::{Account, sync::Folders};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

type FolderChanged = Box<dyn Fn(&str, bool)>;
type FolderRow = (String, adw::ActionRow, gtk::CheckButton);

pub struct SyncSettings {
    pub page: gtk::ScrolledWindow,
    pub accounts: Rc<AccountSelection>,
    pub reload: gtk::Button,
    group: adw::PreferencesGroup,
    spinner: gtk::Spinner,
    rows: RefCell<Vec<FolderRow>>,
    configs: RefCell<HashMap<String, Folders>>,
    callback: RefCell<Option<FolderChanged>>,
    pub updating: Cell<bool>,
    request: Cell<u64>,
}

impl SyncSettings {
    pub fn new() -> Rc<Self> {
        let content = super::column("sync-settings-content");
        content.set_spacing(crate::theme::SETTINGS_MARGIN);
        for margin in ["margin-top", "margin-bottom", "margin-start", "margin-end"] {
            content.set_property(margin, crate::theme::SETTINGS_MARGIN);
        }
        let page = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let selection = adw::PreferencesGroup::new();
        let accounts = AccountSelection::new();
        let reload = super::button("view-refresh-symbolic", "Reload folders");
        accounts.row.add_suffix(&reload);
        selection.add(&accounts.row);
        content.append(&selection);
        let group = adw::PreferencesGroup::builder()
            .title("Folders to sync")
            .build();
        let spinner = gtk::Spinner::builder().valign(gtk::Align::Center).build();
        group.set_header_suffix(Some(&spinner));
        content.append(&group);
        Rc::new(Self {
            page,
            accounts,
            reload,
            group,
            spinner,
            rows: RefCell::default(),
            configs: RefCell::default(),
            callback: RefCell::default(),
            updating: Cell::new(false),
            request: Cell::new(0),
        })
    }

    pub fn connect_folder_changed(&self, callback: impl Fn(&str, bool) + 'static) {
        *self.callback.borrow_mut() = Some(Box::new(callback));
    }

    pub fn busy(&self, loading: bool) -> u64 {
        self.request.set(self.request.get().wrapping_add(1));
        self.updating.set(true);
        self.group.set_sensitive(false);
        self.reload.set_sensitive(false);
        self.spinner.start();
        if loading {
            self.clear_rows();
            self.group.set_description(Some("Loading folders…"));
        }
        self.request.get()
    }

    pub fn accepts(&self, request: u64) -> bool {
        self.request.get() == request
    }

    pub fn loaded(self: &Rc<Self>, configs: HashMap<String, Folders>) {
        *self.configs.borrow_mut() = configs;
        self.render();
    }

    pub fn changes(&self, folder: &str, enabled: bool) -> Vec<(Account, Vec<String>)> {
        let configs = self.configs.borrow();
        self.accounts
            .selected_accounts()
            .into_iter()
            .filter_map(|account| {
                let config = configs.get(&account.email)?;
                if config.selected.iter().any(|name| name == folder) == enabled {
                    return None;
                }
                let mut selected = config.selected.clone();
                selected.retain(|name| name != folder);
                if enabled {
                    selected.push(folder.to_owned());
                }
                selected.sort();
                Some((account, selected))
            })
            .collect()
    }

    pub fn saved(self: &Rc<Self>, saved: Vec<(String, Vec<String>)>) {
        for (account, selected) in saved {
            if let Some(config) = self.configs.borrow_mut().get_mut(&account) {
                config.selected = selected;
            }
        }
        self.render();
    }

    pub fn failed(&self) {
        self.spinner.stop();
        self.reload
            .set_sensitive(!self.accounts.selected_accounts().is_empty());
        self.group
            .set_description(Some("Could not load folders. Reload to try again."));
        self.updating.set(false);
    }

    pub fn empty(&self) {
        self.request.set(self.request.get().wrapping_add(1));
        self.spinner.stop();
        self.clear_rows();
        self.configs.borrow_mut().clear();
        self.group
            .set_description(Some("Select accounts to choose their sync folders."));
        self.reload.set_sensitive(false);
        self.updating.set(false);
    }

    fn clear_rows(&self) {
        for (_, row, _) in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
    }

    fn render(self: &Rc<Self>) {
        self.updating.set(true);
        self.clear_rows();
        let accounts = self.accounts.selected_accounts();
        let configs = self.configs.borrow();
        let selected: Vec<_> = accounts
            .iter()
            .filter_map(|account| configs.get(&account.email))
            .collect();
        let mut folders = selected
            .first()
            .map_or_else(Vec::new, |config| config.folders.clone());
        if selected.len() == 1 {
            folders.extend(selected[0].selected.iter().cloned());
        } else {
            folders.retain(|folder| {
                selected
                    .iter()
                    .all(|config| config.folders.contains(folder))
            });
        }
        folders.sort_by(|left, right| (left != "INBOX", left).cmp(&(right != "INBOX", right)));
        folders.dedup();
        self.group.set_title(if accounts.len() > 1 {
            "Common folders"
        } else {
            "Folders to sync"
        });
        self.group.set_description(Some(if folders.is_empty() {
            if accounts.len() > 1 {
                "These accounts have no folders in common."
            } else {
                "No folders available for this account."
            }
        } else {
            "Download full messages automatically from selected folders."
        }));
        for folder in folders {
            let enabled = selected
                .iter()
                .filter(|config| config.selected.contains(&folder))
                .count();
            let mixed = enabled > 0 && enabled < selected.len();
            let check = gtk::CheckButton::builder()
                .active(enabled == selected.len())
                .inconsistent(mixed)
                .valign(gtk::Align::Center)
                .build();
            let row = adw::ActionRow::builder()
                .title(&folder)
                .use_markup(false)
                .activatable_widget(&check)
                .build();
            row.add_suffix(&check);
            if mixed {
                row.set_subtitle("Enabled for some selected accounts");
            } else if selected
                .iter()
                .any(|config| !config.folders.contains(&folder))
            {
                row.set_subtitle("Unavailable. Turn off to remove this saved choice.");
            }
            let weak = Rc::downgrade(self);
            let name = folder.clone();
            check.connect_toggled(move |check| {
                if let Some(ui) = weak.upgrade()
                    && !ui.updating.get()
                    && let Some(callback) = ui.callback.borrow().as_ref()
                {
                    callback(&name, check.is_active());
                }
            });
            self.group.add(&row);
            self.rows.borrow_mut().push((folder, row, check));
        }
        self.spinner.stop();
        self.group.set_sensitive(true);
        self.reload.set_sensitive(true);
        self.updating.set(false);
    }
}
