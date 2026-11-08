mod controls;
mod outcomes;
mod row;

use super::avatars::Avatars;
use crate::{
    models::{dashboard::Entry, senders},
    theme,
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};

pub use controls::Controls;
type Requested = RefCell<Option<Box<dyn Fn(Vec<Entry>, bool)>>>;
type OpenLink = Rc<dyn Fn(&str)>;

#[derive(Clone)]
pub enum Status {
    Pending,
    Done,
    Manual(String),
    Failed(String),
}

pub struct Subscriptions {
    pub widget: gtk::Box,
    pub back: gtk::Button,
    pub stop: gtk::Button,
    pub list: gtk::ListView,
    pub bottom: Controls,
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    progress: gtk::Label,
    status_row: gtk::Box,
    statuses: RefCell<HashMap<String, Status>>,
    rows: RefCell<Vec<Weak<row::Row>>>,
    avatars: Rc<Avatars>,
    requested: Requested,
    retry_urls: RefCell<HashMap<String, String>>,
    open_link: RefCell<OpenLink>,
    blocked: Cell<bool>,
    updating: Cell<bool>,
}

impl Subscriptions {
    pub fn new(avatars: Rc<Avatars>) -> Rc<Self> {
        let widget = super::column("subscriptions-screen");
        let back = super::button("go-previous-symbolic", "Back to dashboard");
        let title = adw::WindowTitle::new("Subscriptions", "");
        let header = adw::HeaderBar::builder()
            .title_widget(&title)
            .show_start_title_buttons(false)
            .build();
        header.pack_start(&back);
        widget.append(&header);
        let bottom = Controls::new();
        let status = super::horizontal("subscriptions-status", theme::SPACING);
        let progress = super::label("", "dim-label");
        progress.set_wrap(true);
        progress.set_ellipsize(gtk::pango::EllipsizeMode::None);
        let stop = super::action_button("process-stop-symbolic", "Stop");
        stop.set_visible(false);
        status.append(&progress);
        status.append(&stop);
        status.set_visible(false);
        widget.append(&status);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let list = gtk::ListView::builder()
            .model(&selection)
            .single_click_activate(true)
            .css_classes(["message-list-view", "subscriptions-list"])
            .build();
        let scroll = super::scroll(&list);
        scroll.add_css_class("subscriptions-content");
        scroll.add_css_class("message-list-surface");
        scroll.set_overflow(gtk::Overflow::Hidden);
        widget.append(&scroll);
        widget.append(&bottom.widget);
        let screen = Rc::new(Self {
            widget,
            back,
            stop,
            list,
            bottom,
            store,
            selection,
            progress,
            status_row: status,
            statuses: RefCell::default(),
            rows: RefCell::default(),
            avatars,
            requested: RefCell::default(),
            retry_urls: RefCell::default(),
            open_link: RefCell::new(Rc::new(super::links::open)),
            blocked: Cell::new(false),
            updating: Cell::new(false),
        });
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(&screen);
        factory.connect_setup(move |_, object| {
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            if let Some(screen) = weak.upgrade() {
                row::Row::attach(item, &screen);
            }
        });
        screen.list.set_factory(Some(&factory));
        let weak = Rc::downgrade(&screen);
        screen.selection.connect_selection_changed(move |_, _, _| {
            if let Some(screen) = weak.upgrade() {
                screen.update_controls();
            }
        });
        let weak = Rc::downgrade(&screen);
        screen.list.connect_activate(move |_, position| {
            if let Some(screen) = weak.upgrade()
                && !screen.blocked.get()
                && let Some(entry) = screen.entry(position)
                && !screen.unavailable(&entry.id)
            {
                if screen.selection.is_selected(position) {
                    screen.selection.unselect_item(position);
                } else {
                    screen.selection.select_item(position, false);
                }
            }
        });
        screen.bottom.connect(&screen);
        screen.update_controls();
        screen
    }

    pub fn load(&self, mut entries: Vec<Entry>) {
        let ids: HashSet<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
        self.statuses
            .borrow_mut()
            .retain(|id, _| ids.contains(id.as_str()));
        self.retry_urls
            .borrow_mut()
            .retain(|id, _| ids.contains(id.as_str()));
        entries.sort_by(|left, right| self.compare(left, right));
        let objects: Vec<_> = entries.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.store.splice(0, self.store.n_items(), &objects);
        self.selection.select_all();
        self.progress.set_text("");
        self.status_row.set_visible(false);
        self.update_controls();
    }

    pub fn update(&self, entries: Vec<Entry>) {
        let selected = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        let mut incoming: HashMap<_, _> = entries
            .iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect();
        for position in (0..self.store.n_items()).rev() {
            let object = self
                .store
                .item(position)
                .and_downcast::<glib::BoxedAnyObject>()
                .unwrap();
            let id = object.borrow::<Entry>().id.clone();
            if let Some(entry) = incoming.remove(&id) {
                if *object.borrow::<Entry>() != *entry {
                    object.replace(entry.clone());
                }
            } else if !self.statuses.borrow().contains_key(&id) {
                self.store.remove(position);
            }
        }
        for entry in &entries {
            if incoming.contains_key(&entry.id) {
                self.store.append(&glib::BoxedAnyObject::new(entry.clone()));
            }
        }
        self.sort(selected);
        self.refresh_rows();
        self.update_controls();
    }

    fn sort(&self, selected: HashSet<String>) {
        let previous: Vec<_> = (0..self.store.n_items())
            .map(|index| {
                self.store
                    .item(index)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .unwrap()
            })
            .collect();
        let mut ordered = previous.clone();
        ordered
            .sort_by(|left, right| self.compare(&left.borrow::<Entry>(), &right.borrow::<Entry>()));
        let prefix = previous
            .iter()
            .zip(&ordered)
            .take_while(|(left, right)| left == right)
            .count();
        if prefix < previous.len() {
            let suffix = previous[prefix..]
                .iter()
                .rev()
                .zip(ordered[prefix..].iter().rev())
                .take_while(|(left, right)| left == right)
                .count();
            let end = previous.len() - suffix;
            self.store
                .splice(prefix as u32, (end - prefix) as u32, &ordered[prefix..end]);
        }
        let bits = gtk::Bitset::new_empty();
        for (index, object) in ordered.iter().enumerate() {
            if selected.contains(&object.borrow::<Entry>().id) {
                bits.add(index as u32);
            }
        }
        self.selection
            .set_selection(&bits, &gtk::Bitset::new_range(0, self.store.n_items()));
    }

    pub fn connect_unsubscribe(&self, callback: impl Fn(Vec<Entry>, bool) + 'static) {
        *self.requested.borrow_mut() = Some(Box::new(callback));
    }

    pub fn progress(&self, text: &str, active: bool, stopping: bool) {
        self.blocked.set(active);
        self.progress.set_text(text);
        self.status_row.set_visible(!text.is_empty());
        self.stop.set_visible(active);
        self.stop.set_sensitive(!stopping);
        self.refresh_rows();
        self.update_controls();
    }

    fn entry(&self, position: u32) -> Option<Entry> {
        Some(
            self.store
                .item(position)?
                .downcast::<glib::BoxedAnyObject>()
                .ok()?
                .borrow::<Entry>()
                .clone(),
        )
    }

    pub fn selected_entries(&self) -> Vec<Entry> {
        (0..self.store.n_items())
            .filter(|&index| self.selection.is_selected(index))
            .filter_map(|index| self.entry(index))
            .filter(|entry| !self.unavailable(&entry.id))
            .collect()
    }

    fn request(&self, entries: Vec<Entry>, retry: bool) {
        if !entries.is_empty()
            && let Some(callback) = self.requested.borrow().as_ref()
        {
            callback(entries, retry);
        }
    }

    fn update_controls(&self) {
        self.updating.set(true);
        let selected = self.selection.selection().size() as usize;
        let total = (0..self.store.n_items())
            .filter(|&index| {
                self.entry(index)
                    .is_some_and(|entry| !self.unavailable(&entry.id))
            })
            .count();
        self.bottom.update(selected, total, self.blocked.get());
        self.updating.set(false);
    }

    fn refresh_rows(&self) {
        self.rows.borrow_mut().retain(|row| {
            if let Some(row) = row.upgrade() {
                row.refresh();
                true
            } else {
                false
            }
        });
    }
}

fn compare_entries(left: &Entry, right: &Entry) -> std::cmp::Ordering {
    right
        .recent_emails
        .cmp(&left.recent_emails)
        .then_with(|| left.id.cmp(&right.id))
}
