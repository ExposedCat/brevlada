use crate::models::{
    Account,
    search::{Filters, SOURCE_LABELS},
};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

#[derive(Clone, Copy)]
pub enum Change {
    Accounts,
    Criteria,
}

type ChangeCallback = Box<dyn Fn(Change)>;

pub struct SearchFilters {
    pub widget: gtk::Box,
    accounts_button: gtk::MenuButton,
    accounts_box: gtk::Box,
    use_current: gtk::Button,
    all_accounts: gtk::Button,
    accounts: RefCell<Vec<String>>,
    current: RefCell<Option<String>>,
    selected: RefCell<Option<HashSet<String>>>,
    folders_button: gtk::MenuButton,
    current_folder: RefCell<Option<String>>,
    pinned_folder: RefCell<Option<String>>,
    use_current_folder: gtk::CheckButton,
    all_folders: gtk::CheckButton,
    sources_button: gtk::MenuButton,
    sources: Vec<gtk::CheckButton>,
    dates_button: gtk::MenuButton,
    from: gtk::Entry,
    to: gtk::Entry,
    date_error: gtk::Label,
    updating: Cell<bool>,
    callbacks: RefCell<Vec<ChangeCallback>>,
}

impl SearchFilters {
    pub fn new() -> Rc<Self> {
        let (accounts_button, accounts_box) = dropdown("Accounts (0)");
        let use_current = gtk::Button::with_label("Use current account");
        let all_accounts = gtk::Button::with_label("Select all accounts");
        accounts_button.set_tooltip_text(Some("Search selected accounts"));
        let (folders_button, folders_box) = dropdown("All Folders");
        let all_folders = gtk::CheckButton::with_label("All Folders");
        let use_current_folder = gtk::CheckButton::with_label("Current Folder");
        use_current_folder.set_group(Some(&all_folders));
        use_current_folder.set_sensitive(false);
        all_folders.set_active(true);
        folders_box.append(&all_folders);
        folders_box.append(&use_current_folder);
        let (sources_button, sources_box) = dropdown("All Sources");
        let sources: Vec<_> = SOURCE_LABELS
            .iter()
            .map(|label| {
                let check = gtk::CheckButton::with_label(label);
                check.set_active(true);
                sources_box.append(&check);
                check
            })
            .collect();
        let all_sources = gtk::Button::with_label("Enable all sources");
        sources_box.append(&all_sources);
        let (dates_button, dates_box) = dropdown("Any Date");
        dates_box.set_width_request(352);
        let (from, to) = date_inputs(&dates_box);
        let date_error = super::error_label();
        dates_box.append(&date_error);
        let clear_dates = gtk::Button::with_label("Clear dates");
        dates_box.append(&clear_dates);
        // Two rows keep all four filters readable in the sender pane.
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("message-search-filters");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.append(&accounts_button);
        top.append(&folders_button);
        bottom.append(&sources_button);
        bottom.append(&dates_button);
        widget.append(&top);
        widget.append(&bottom);
        let filters = Rc::new(Self {
            widget,
            accounts_button,
            accounts_box,
            use_current,
            all_accounts,
            accounts: RefCell::default(),
            current: RefCell::default(),
            selected: RefCell::default(),
            folders_button,
            current_folder: RefCell::default(),
            pinned_folder: RefCell::default(),
            use_current_folder,
            all_folders,
            sources_button,
            sources,
            dates_button,
            from,
            to,
            date_error,
            updating: Cell::new(false),
            callbacks: RefCell::default(),
        });
        let weak = Rc::downgrade(&filters);
        filters.all_folders.connect_toggled(move |_| {
            if let Some(filters) = weak.upgrade() {
                filters.pinned_folder.borrow_mut().take();
                filters.changed(Change::Criteria);
            }
        });
        for check in &filters.sources {
            let weak = Rc::downgrade(&filters);
            check.connect_toggled(move |_| {
                if let Some(filters) = weak.upgrade() {
                    filters.changed(Change::Criteria);
                }
            });
        }
        for entry in [&filters.from, &filters.to] {
            let weak = Rc::downgrade(&filters);
            entry.connect_changed(move |_| {
                if let Some(filters) = weak.upgrade() {
                    filters.changed(Change::Criteria);
                }
            });
        }
        let weak = Rc::downgrade(&filters);
        all_sources.connect_clicked(move |_| {
            if let Some(filters) = weak.upgrade() {
                filters.updating.set(true);
                for check in &filters.sources {
                    check.set_active(true);
                }
                filters.updating.set(false);
                filters.changed(Change::Criteria);
            }
        });
        let weak = Rc::downgrade(&filters);
        clear_dates.connect_clicked(move |_| {
            if let Some(filters) = weak.upgrade() {
                filters.updating.set(true);
                filters.from.set_text("");
                filters.to.set_text("");
                filters.updating.set(false);
                filters.changed(Change::Criteria);
            }
        });
        let weak = Rc::downgrade(&filters);
        filters.use_current.connect_clicked(move |_| {
            if let Some(filters) = weak.upgrade() {
                if filters.current.borrow().is_none() {
                    return;
                }
                *filters.selected.borrow_mut() = None;
                filters.rebuild_accounts();
                filters.changed(Change::Accounts);
            }
        });
        let weak = Rc::downgrade(&filters);
        filters.all_accounts.connect_clicked(move |_| {
            if let Some(filters) = weak.upgrade() {
                *filters.selected.borrow_mut() =
                    Some(filters.accounts.borrow().iter().cloned().collect());
                filters.rebuild_accounts();
                filters.changed(Change::Accounts);
            }
        });
        filters.rebuild_accounts();
        filters
    }

    pub fn connect_changed(&self, callback: impl Fn(Change) + 'static) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }

    pub fn selected_accounts(&self) -> HashSet<String> {
        self.selected
            .borrow()
            .clone()
            .unwrap_or_else(|| self.default_accounts(self.current.borrow().as_deref()))
    }

    pub fn accounts_for(&self, current: Option<&str>) -> HashSet<String> {
        self.selected
            .borrow()
            .clone()
            .unwrap_or_else(|| self.default_accounts(current))
    }

    fn default_accounts(&self, current: Option<&str>) -> HashSet<String> {
        current.map_or_else(
            || self.accounts.borrow().iter().cloned().collect(),
            |account| HashSet::from([account.to_owned()]),
        )
    }

    pub fn reset_scope(self: &Rc<Self>) {
        self.updating.set(true);
        *self.selected.borrow_mut() = None;
        self.pinned_folder.borrow_mut().take();
        if self.current_folder.borrow().is_some() {
            self.use_current_folder.set_active(true);
        } else {
            self.all_folders.set_active(true);
        }
        self.rebuild_accounts();
    }

    pub fn keep_selection(&self) {
        // Opening a result changes the current mailbox, but search stays scoped
        // to the accounts that were selected when the result was shown.
        if self.selected.borrow().is_none() {
            let selected = self.selected_accounts();
            *self.selected.borrow_mut() = Some(selected);
        }
        if self.use_current_folder.is_active() && self.pinned_folder.borrow().is_none() {
            *self.pinned_folder.borrow_mut() = self.current_folder.borrow().clone();
        }
    }

    /// None searches all cached folders; Some searches this exact folder in each account.
    pub fn folder(&self) -> Option<String> {
        if self.all_folders.is_active() {
            None
        } else {
            self.pinned_folder
                .borrow()
                .clone()
                .or_else(|| self.current_folder.borrow().clone())
        }
    }

    pub fn set_current_folder(&self, folder: Option<&str>, preserve_search: bool) {
        self.updating.set(true);
        *self.current_folder.borrow_mut() = folder
            .filter(|folder| !folder.is_empty())
            .map(str::to_owned);
        if !preserve_search {
            self.pinned_folder.borrow_mut().take();
        }
        let available = self.current_folder.borrow().is_some();
        self.use_current_folder.set_sensitive(available);
        if !available {
            self.all_folders.set_active(true);
        }
        self.updating.set(false);
        self.update_labels();
    }

    pub fn account_label(&self) -> String {
        let selected = self.selected_accounts();
        if selected.is_empty() {
            String::new()
        } else if selected.len() == 1 {
            selected.into_iter().next().unwrap()
        } else if selected.len() == self.accounts.borrow().len() {
            "All accounts".into()
        } else {
            format!("{} accounts", selected.len())
        }
    }

    pub fn filters(&self) -> Result<Filters, &'static str> {
        let (from, to) = Filters::dates(&self.from.text(), &self.to.text())?;
        Ok(Filters {
            sources: std::array::from_fn(|index| self.sources[index].is_active()),
            from,
            to,
        })
    }

    pub fn set_accounts(self: &Rc<Self>, accounts: &[Account]) {
        let selected_before = self.selected_accounts();
        let mut known = self.accounts.borrow_mut();
        known.retain(|email| accounts.iter().any(|account| account.email == *email));
        for account in accounts {
            if !known.contains(&account.email) {
                known.push(account.email.clone());
            }
        }
        if let Some(selected) = self.selected.borrow_mut().as_mut() {
            selected.retain(|email| known.contains(email));
        }
        if self
            .current
            .borrow()
            .as_ref()
            .is_some_and(|current| !known.contains(current))
        {
            *self.current.borrow_mut() = None;
        }
        drop(known);
        self.rebuild_accounts();
        if self.selected_accounts() != selected_before {
            self.changed(Change::Accounts);
        }
    }

    pub fn set_current(self: &Rc<Self>, email: Option<&str>) {
        *self.current.borrow_mut() = email.map(str::to_owned);
        self.rebuild_accounts();
    }

    fn rebuild_accounts(self: &Rc<Self>) {
        self.updating.set(true);
        super::clear(&self.accounts_box);
        self.accounts_box.append(&self.use_current);
        let selected = self.selected_accounts();
        for email in self.accounts.borrow().iter() {
            let check = gtk::CheckButton::with_label(email);
            check.set_active(selected.contains(email));
            self.accounts_box.append(&check);
            let weak = Rc::downgrade(self);
            let email = email.clone();
            check.connect_toggled(move |check| {
                if let Some(filters) = weak.upgrade() {
                    if filters.updating.get() {
                        return;
                    }
                    let mut selected = filters.selected_accounts();
                    if check.is_active() {
                        selected.insert(email.clone());
                    } else {
                        selected.remove(&email);
                    }
                    *filters.selected.borrow_mut() = Some(selected);
                    filters.changed(Change::Accounts);
                }
            });
        }
        self.accounts_box.append(&self.all_accounts);
        self.updating.set(false);
        self.update_labels();
    }

    fn update_labels(&self) {
        self.folders_button
            .set_label(if self.all_folders.is_active() {
                "All Folders"
            } else {
                "Current Folder"
            });
        self.folders_button.set_tooltip_text(
            self.folder()
                .as_deref()
                .or(Some("Search all cached folders")),
        );
        let selected = self.selected_accounts();
        let accounts = self.accounts.borrow();
        let all = !accounts.is_empty() && accounts.iter().all(|account| selected.contains(account));
        let current = self
            .current
            .borrow()
            .as_ref()
            .is_some_and(|current| selected.len() == 1 && selected.contains(current));
        self.accounts_button.set_label(&if current {
            "Current Account".into()
        } else if all {
            "All accounts".into()
        } else {
            format!("Accounts ({})", selected.len())
        });
        self.all_accounts
            .set_sensitive(!accounts.is_empty() && !all);
        self.use_current
            .set_sensitive(self.current.borrow().is_some() && !current);
        let count = self
            .sources
            .iter()
            .filter(|check| check.is_active())
            .count();
        self.sources_button
            .set_label(&if count == self.sources.len() {
                "All Sources".into()
            } else {
                format!("Sources ({count})")
            });
        let valid = self.filters();
        self.date_error.set_visible(valid.is_err());
        self.date_error
            .set_label(valid.as_ref().err().copied().unwrap_or(""));
        self.dates_button.set_label(if valid.is_err() {
            "Invalid dates"
        } else if self.from.text().trim().is_empty() && self.to.text().trim().is_empty() {
            "Any Date"
        } else {
            "Date range"
        });
        let description = format!(
            "From: {} · To: {}",
            if self.from.text().is_empty() {
                "any".into()
            } else {
                self.from.text()
            },
            if self.to.text().is_empty() {
                "any".into()
            } else {
                self.to.text()
            }
        );
        self.dates_button.set_tooltip_text(Some(&description));
    }

    fn changed(&self, change: Change) {
        if self.updating.get() {
            return;
        }
        self.update_labels();
        for callback in self.callbacks.borrow().iter() {
            callback(change);
        }
    }
}

fn dropdown(label: &str) -> (gtk::MenuButton, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for margin in ["margin-top", "margin-bottom", "margin-start", "margin-end"] {
        content.set_property(margin, 12i32);
    }
    let popover = gtk::Popover::builder()
        .child(&content)
        .autohide(true)
        .build();
    let button = gtk::MenuButton::builder()
        .label(label)
        .popover(&popover)
        .hexpand(true)
        .build();
    (button, content)
}

fn date_input(parent: &gtk::Box, label: &str) -> (gtk::Entry, gtk::ToggleButton) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some(label));
    title.set_width_chars(4);
    title.set_xalign(0.0);
    row.append(&title);
    let entry = gtk::Entry::builder()
        .placeholder_text("YYYY-MM-DD")
        .width_chars(12)
        .hexpand(true)
        .build();
    entry.set_tooltip_text(Some("Leave empty for any date"));
    row.append(&entry);
    let pick = gtk::ToggleButton::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text(format!("Select {label} date"))
        .build();
    row.append(&pick);
    parent.append(&row);
    (entry, pick)
}

fn date_inputs(parent: &gtk::Box) -> (gtk::Entry, gtk::Entry) {
    let (from, from_pick) = date_input(parent, "From");
    let (to, to_pick) = date_input(parent, "To");
    // A single popup owns both the entries and calendar. Nested autohide
    // popovers can lose the parent popup's grab when the calendar closes.
    let calendar = gtk::Calendar::new();
    calendar.add_css_class("search-calendar");
    calendar.set_overflow(gtk::Overflow::Hidden);
    let reveal = gtk::Revealer::builder().child(&calendar).build();
    parent.append(&reveal);
    let target: Rc<RefCell<Option<gtk::glib::WeakRef<gtk::Entry>>>> = Rc::default();
    let syncing = Rc::new(Cell::new(false));
    for (entry, pick, other) in [(&from, &from_pick, &to_pick), (&to, &to_pick, &from_pick)] {
        let entry = entry.downgrade();
        let other = other.downgrade();
        let calendar = calendar.downgrade();
        let reveal = reveal.downgrade();
        let target = target.clone();
        let syncing = syncing.clone();
        pick.connect_toggled(move |pick| {
            if syncing.get() {
                return;
            }
            syncing.set(true);
            if pick.is_active() {
                if let Some(other) = other.upgrade() {
                    other.set_active(false);
                }
                *target.borrow_mut() = Some(entry.clone());
                if let (Some(entry), Some(calendar)) = (entry.upgrade(), calendar.upgrade())
                    && let Ok(Some(date)) = Filters::dates(&entry.text(), "").map(|(date, _)| date)
                    && let Ok(date) =
                        gtk::glib::DateTime::from_iso8601(&format!("{date}T12:00:00Z"), None)
                {
                    calendar.select_day(&date);
                }
            } else {
                *target.borrow_mut() = None;
            }
            if let Some(reveal) = reveal.upgrade() {
                reveal.set_reveal_child(pick.is_active());
            }
            if pick.is_active()
                && let Some(calendar) = calendar.upgrade()
            {
                calendar.grab_focus();
            }
            syncing.set(false);
        });
    }
    let from_toggle = from_pick.downgrade();
    let to_toggle = to_pick.downgrade();
    let selected_target = target.clone();
    let selected_syncing = syncing.clone();
    calendar.connect_day_selected(move |calendar| {
        if selected_syncing.get() {
            return;
        }
        let entry = selected_target
            .borrow()
            .as_ref()
            .and_then(|entry| entry.upgrade());
        if let Some(entry) = entry {
            let date = calendar.date();
            entry.set_text(&format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day_of_month()
            ));
            entry.grab_focus();
            for pick in [&from_toggle, &to_toggle] {
                if let Some(pick) = pick.upgrade() {
                    pick.set_active(false);
                }
            }
        }
    });
    let popover = parent
        .ancestor(gtk::Popover::static_type())
        .and_downcast::<gtk::Popover>()
        .unwrap();
    let from_pick = from_pick.downgrade();
    let to_pick = to_pick.downgrade();
    popover.connect_closed(move |_| {
        for pick in [&from_pick, &to_pick] {
            if let Some(pick) = pick.upgrade() {
                pick.set_active(false);
            }
        }
    });
    (from, to)
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session to inspect date popup sizing and dismissal"]
    fn date_calendar_stays_in_one_popup_and_restores_focus() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let provider = gtk::CssProvider::new();
        provider.load_from_string(crate::theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let filters = SearchFilters::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.append(&filters.widget);
        let outside = gtk::Button::with_label("Outside date popup");
        outside.set_vexpand(true);
        content.append(&outside);
        let window = gtk::Window::builder()
            .title("Brevlada date popup diagnostic")
            .default_width(700)
            .default_height(700)
            .child(&content)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        let settle = || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(350);
            while std::time::Instant::now() < deadline {
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        settle();
        let popup = filters.dates_button.popover().unwrap();
        let picker = filters
            .from
            .parent()
            .unwrap()
            .last_child()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        let reveal = filters
            .to
            .parent()
            .unwrap()
            .next_sibling()
            .and_downcast::<gtk::Revealer>()
            .unwrap();
        let calendar = reveal.child().and_downcast::<gtk::Calendar>().unwrap();
        for selected in [false, true] {
            filters.dates_button.popup();
            settle();
            picker.set_active(true);
            settle();
            assert_eq!(
                calendar.ancestor(gtk::Popover::static_type()),
                Some(popup.clone().upcast())
            );
            assert!(popup.is_autohide());
            assert!(popup.get_visible());
            assert!(reveal.reveals_child());
            let calendar_bounds = calendar.compute_bounds(&popup).unwrap();
            assert!(calendar_bounds.x() >= 0.0);
            assert!(calendar_bounds.x() + calendar_bounds.width() <= popup.width() as f32);
            assert!(
                filters.from.width() >= 150,
                "Date text should fit: {}",
                filters.from.width()
            );
            if selected {
                calendar.select_day(
                    &gtk::glib::DateTime::from_iso8601("2026-10-01T12:00:00Z", None).unwrap(),
                );
                settle();
                assert_eq!(filters.from.text(), "2026-10-01");
                assert!(!reveal.reveals_child());
                let focus = gtk::prelude::RootExt::focus(&window).unwrap();
                assert!(focus == filters.from || focus.is_ancestor(&filters.from));
            } else if let Ok(path) = std::env::var("BREVLADA_DATE_SCREENSHOT") {
                let paintable = gtk::WidgetPaintable::new(Some(&popup));
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(&snapshot, popup.width() as f64, popup.height() as f64);
                popup
                    .renderer()
                    .unwrap()
                    .render_texture(snapshot.to_node().unwrap(), None)
                    .save_to_png(path)
                    .unwrap();
            }
            if std::env::var_os("BREVLADA_DATE_CLICK_TEST").is_some() {
                eprintln!("DATE_POPUP_READY selected={selected}");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                while popup.get_visible() && std::time::Instant::now() < deadline {
                    context.iteration(false);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(
                    !popup.get_visible(),
                    "Outside click must dismiss the main popup"
                );
            } else {
                popup.popdown();
                settle();
            }
            assert!(!picker.is_active());
            assert!(!reveal.reveals_child());
        }
        window.destroy();
    }

    #[test]
    #[ignore = "Requires a graphical session for account, source and calendar controls"]
    fn controls_follow_current_account_and_keep_custom_selections() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let filters = SearchFilters::new();
        let account = |email: &str| Account {
            path: String::new(),
            email: email.into(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        filters.set_accounts(&[account("a@example.com"), account("b@example.com")]);
        assert!(!filters.use_current.is_sensitive());
        assert_eq!(filters.selected_accounts().len(), 2);
        assert!(!filters.all_accounts.is_sensitive());
        assert_eq!(
            filters.accounts_button.label().as_deref(),
            Some("All accounts")
        );
        assert_eq!(filters.accounts_for(None).len(), 2);
        assert_eq!(filters.account_label(), "All accounts");
        assert_eq!(
            filters.sources_button.label().as_deref(),
            Some("All Sources")
        );
        assert_eq!(filters.dates_button.label().as_deref(), Some("Any Date"));
        assert_eq!(filters.folder(), None);
        assert!(!filters.use_current_folder.is_sensitive());
        filters.set_current_folder(Some("Archive"), false);
        assert!(filters.use_current_folder.is_sensitive());
        filters.use_current_folder.set_active(true);
        assert_eq!(filters.folder().as_deref(), Some("Archive"));
        assert_eq!(
            filters.folders_button.label().as_deref(),
            Some("Current Folder")
        );
        filters.keep_selection();
        filters.set_current_folder(Some("INBOX"), true);
        assert_eq!(filters.folder().as_deref(), Some("Archive"));
        filters.all_folders.set_active(true);
        assert_eq!(filters.folder(), None);
        filters.use_current_folder.set_active(true);
        assert_eq!(filters.folder().as_deref(), Some("INBOX"));
        filters.set_current_folder(None, false);
        assert!(!filters.use_current_folder.is_sensitive());
        assert!(filters.all_folders.is_active());
        assert_eq!(filters.folder(), None);
        assert!(!filters.use_current.is_sensitive());
        *filters.selected.borrow_mut() = None;
        filters.set_current(Some("a@example.com"));
        assert_eq!(
            filters.selected_accounts(),
            HashSet::from(["a@example.com".into()])
        );
        assert_eq!(filters.filters().unwrap(), Filters::default());
        assert!(!filters.use_current.is_sensitive());
        assert_eq!(
            filters.accounts_button.label().as_deref(),
            Some("Current Account")
        );
        let second = filters
            .accounts_box
            .last_child()
            .unwrap()
            .prev_sibling()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        second.set_active(true);
        assert_eq!(filters.selected_accounts().len(), 2);
        assert!(filters.use_current.is_sensitive());
        assert_eq!(
            filters.accounts_button.label().as_deref(),
            Some("All accounts")
        );
        filters.set_current(Some("b@example.com"));
        assert_eq!(filters.selected_accounts().len(), 2);
        filters
            .accounts_box
            .first_child()
            .and_downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(
            filters.selected_accounts(),
            HashSet::from(["b@example.com".into()])
        );
        assert!(!filters.use_current.is_sensitive());
        assert_eq!(
            filters.accounts_button.label().as_deref(),
            Some("Current Account")
        );
        filters.set_accounts(&[
            account("c@example.com"),
            account("b@example.com"),
            account("a@example.com"),
        ]);
        assert_eq!(
            *filters.accounts.borrow(),
            ["a@example.com", "b@example.com", "c@example.com"]
        );
        assert_eq!(
            filters.selected_accounts(),
            HashSet::from(["b@example.com".into()])
        );
        filters.set_current_folder(Some("Archive"), false);
        filters.reset_scope();
        assert_eq!(filters.folder().as_deref(), Some("Archive"));
        filters.keep_selection();
        filters.set_current_folder(Some("INBOX"), true);
        filters.all_accounts.emit_clicked();
        filters.reset_scope();
        assert_eq!(
            filters.selected_accounts(),
            HashSet::from(["b@example.com".into()])
        );
        assert_eq!(filters.folder().as_deref(), Some("INBOX"));
        filters.all_folders.set_active(true);
        filters.reset_scope();
        assert_eq!(filters.folder().as_deref(), Some("INBOX"));
        filters.set_accounts(&[account("b@example.com")]);
        assert_eq!(
            filters.accounts_button.label().as_deref(),
            Some("Current Account")
        );
        filters.sources[5].set_active(false);
        assert!(!filters.filters().unwrap().sources[5]);
        filters.from.set_text("2026-10-03");
        filters.to.set_text("2026-10-02");
        assert!(filters.filters().is_err());
        assert!(filters.date_error.get_visible());
        filters.from.set_text("2026-10-02");
        assert!(filters.filters().is_ok());
        assert!(!filters.date_error.get_visible());
        let row = filters.from.parent().unwrap();
        let picker = row
            .last_child()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        picker.set_active(true);
        let calendar = filters
            .to
            .parent()
            .unwrap()
            .next_sibling()
            .and_downcast::<gtk::Revealer>()
            .unwrap()
            .child()
            .and_downcast::<gtk::Calendar>()
            .unwrap();
        calendar
            .select_day(&gtk::glib::DateTime::from_iso8601("2026-10-01T12:00:00Z", None).unwrap());
        assert_eq!(filters.from.text(), "2026-10-01");
        let dates_box = filters.to.parent().unwrap().parent().unwrap();
        dates_box
            .last_child()
            .and_downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(filters.filters().unwrap().from, None);
        assert_eq!(filters.filters().unwrap().to, None);
    }
}
