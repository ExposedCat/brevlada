mod items;

use super::*;
use items::dismissed_path;
use models::dashboard::Entry;

pub(super) struct Dashboard {
    pub view: ui::dashboard::Dashboard,
    accounts: RefCell<Vec<String>>,
    selected: RefCell<String>,
    request: Cell<u64>,
    open_request: Cell<u64>,
    pending: Cell<bool>,
    scheduled: Cell<bool>,
    dirty: Cell<bool>,
    items: RefCell<Vec<Entry>>,
    dismissed: RefCell<HashSet<String>>,
}

impl Dashboard {
    pub fn new(view: ui::dashboard::Dashboard) -> Self {
        let dismissed = std::fs::read(dismissed_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            view,
            accounts: RefCell::default(),
            selected: RefCell::default(),
            request: Cell::new(0),
            open_request: Cell::new(0),
            pending: Cell::new(false),
            scheduled: Cell::new(false),
            dirty: Cell::new(false),
            items: RefCell::default(),
            dismissed: RefCell::new(dismissed),
        }
    }
}

impl State {
    pub(super) fn dashboard_visible(&self) -> bool {
        self.content_stack.visible_child_name().as_deref() == Some("dashboard")
    }

    pub(super) fn connect_dashboard(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.dashboard.view.connect_changed(move || {
            if let Some(state) = weak.upgrade() {
                state.render_dashboard();
            }
        });
        let weak = Rc::downgrade(self);
        self.dashboard.view.types.connect_changed(move || {
            if let Some(state) = weak.upgrade() {
                state.render_dashboard();
            }
        });
        let weak = Rc::downgrade(self);
        self.dashboard_button.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.content_stack.set_visible_child_name("dashboard");
                state.dashboard.open_request.set(0);
                state.navigation_selection.activate(&state.dashboard_button);
                state.refresh_dashboard();
            }
        });
        let weak = Rc::downgrade(self);
        self.dashboard.view.accounts.connect_selected(move |email| {
            if let Some(state) = weak.upgrade() {
                *state.dashboard.selected.borrow_mut() = email.into();
                state.dashboard.open_request.set(0);
                state.dashboard.view.accounts.set_selected(
                    email,
                    if email.is_empty() {
                        "All accounts"
                    } else {
                        email
                    },
                );
                state.dashboard.items.borrow_mut().clear();
                state.refresh_dashboard();
            }
        });
        let weak = Rc::downgrade(self);
        self.dashboard.view.range.connect_changed(move || {
            if let Some(state) = weak.upgrade() {
                state.dashboard.open_request.set(0);
                state.dashboard.items.borrow_mut().clear();
                state.refresh_dashboard();
            }
        });
        let weak = Rc::downgrade(self);
        self.dashboard.view.refresh.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.refresh_dashboard();
            }
        });
        let target = self.account_sidebar.downgrade();
        self.dashboard
            .view
            .expand_sidebar
            .connect_clicked(move |_| {
                if let Some(sidebar) = target.upgrade() {
                    sidebar.set_visible(true);
                }
            });
        self.dashboard
            .view
            .expand_sidebar
            .set_visible(!self.account_sidebar.get_visible());
        let expand = self.dashboard.view.expand_sidebar.downgrade();
        self.account_sidebar.connect_visible_notify(move |sidebar| {
            if let Some(expand) = expand.upgrade() {
                expand.set_visible(!sidebar.get_visible());
            }
        });
    }

    pub(super) fn dashboard_accounts(self: &Rc<Self>, accounts: &[Account]) {
        let emails: Vec<_> = accounts
            .iter()
            .map(|account| account.email.clone())
            .collect();
        if !emails.contains(&self.dashboard.selected.borrow()) {
            self.dashboard.selected.borrow_mut().clear();
        }
        self.dashboard.view.accounts.set_choices(
            std::iter::once((String::new(), "All accounts".into()))
                .chain(emails.iter().map(|email| (email.clone(), email.clone()))),
        );
        let selected = self.dashboard.selected.borrow();
        self.dashboard.view.accounts.set_selected(
            &selected,
            if selected.is_empty() {
                "All accounts"
            } else {
                &selected
            },
        );
        self.dashboard
            .view
            .accounts
            .widget
            .set_sensitive(!emails.is_empty());
        *self.dashboard.accounts.borrow_mut() = emails;
        drop(selected);
        if self.dashboard_visible() {
            self.refresh_dashboard();
        }
    }

    pub(super) fn refresh_dashboard(self: &Rc<Self>) {
        let selected = self.dashboard.selected.borrow();
        let accounts = if selected.is_empty() {
            self.dashboard.accounts.borrow().clone()
        } else {
            vec![selected.clone()]
        };
        drop(selected);
        let request = self.sender.next_request();
        self.dashboard.request.set(request);
        self.dashboard.dirty.set(false);
        if accounts.is_empty() {
            self.dashboard.view.status(
                "No accounts",
                "Add an email account in GNOME Online Accounts to see your dashboard.",
            );
            self.dashboard.view.refresh.set_sensitive(false);
            self.dashboard.pending.set(false);
            return;
        }
        self.dashboard.pending.set(true);
        self.dashboard.view.refresh.set_sensitive(false);
        if self.dashboard.items.borrow().is_empty() {
            self.dashboard.view.status(
                "Loading dashboard…",
                "Gathering deliveries, plans, and recent purchases.",
            );
        }
        let range = self.dashboard.view.range.range();
        if let Err(error) = self.sender.send(Command::Dashboard {
            accounts,
            request,
            range,
        }) {
            self.dashboard_loaded(request, Err(error.to_string()));
        }
    }

    pub(super) fn dashboard_loaded(
        self: &Rc<Self>,
        request: u64,
        result: Result<Vec<Entry>, String>,
    ) {
        if request != self.dashboard.request.get() {
            return;
        }
        self.dashboard.pending.set(false);
        self.dashboard.view.refresh.set_sensitive(true);
        match result {
            Ok(items) => {
                *self.dashboard.items.borrow_mut() = items;
                if self.dashboard_visible() {
                    self.render_dashboard();
                }
            }
            Err(error) => self
                .dashboard
                .view
                .status("Could not load dashboard", &error),
        }
        if self.dashboard.dirty.replace(false) && self.dashboard_visible() {
            self.dashboard_cache_changed("");
        }
    }

    pub(super) fn dashboard_cache_changed(self: &Rc<Self>, email: &str) {
        if !self.dashboard_visible()
            || (!email.is_empty()
                && !self.dashboard.selected.borrow().is_empty()
                && self.dashboard.selected.borrow().as_str() != email)
        {
            return;
        }
        if self.dashboard.pending.get() {
            self.dashboard.dirty.set(true);
            return;
        }
        if self.dashboard.scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            if let Some(state) = weak.upgrade() {
                state.dashboard.scheduled.set(false);
                if state.dashboard_visible() {
                    state.refresh_dashboard();
                }
            }
        });
    }
}
