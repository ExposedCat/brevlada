mod cards;
mod controls;
mod filters;
mod sections;
mod series;
mod subscriptions;

use super::{account_switcher::AccountSwitcher, button, column, horizontal, label, scroll};
use crate::{
    models::dashboard::{Entry, Section},
    theme,
};
use adw::prelude::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub use cards::Tile;

type Changed = Rc<RefCell<Vec<Box<dyn Fn()>>>>;

pub struct Dashboard {
    pub widget: gtk::Box,
    pub accounts: AccountSwitcher,
    pub range: std::rc::Rc<super::date_range::DateRange>,
    pub types: Rc<filters::Types>,
    pub refresh: gtk::Button,
    pub expand_sidebar: gtk::Button,
    pub unsubscribe_all: gtk::Button,
    pub unsubscribe_stop: gtk::Button,
    unsubscribe_status: gtk::Box,
    unsubscribe_progress: gtk::Label,
    unsubscribe_blocked: std::cell::Cell<bool>,
    stack: gtk::Stack,
    state: adw::StatusPage,
    content: gtk::Box,
    subscriptions: gtk::Box,
    sections: RefCell<HashMap<Section, sections::SectionView>>,
    expanded_limits: Rc<RefCell<HashMap<Section, usize>>>,
    callbacks: Changed,
}

impl Dashboard {
    pub fn new(settings: &gtk::gio::SimpleAction) -> Self {
        let widget = column("dashboard");
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        let accounts = AccountSwitcher::new();
        let range = super::date_range::DateRange::new();
        let types = filters::Types::new();
        let filters = horizontal("dashboard-filters", theme::SMALL_SPACING);
        filters.set_halign(gtk::Align::Center);
        filters.append(&accounts.widget);
        filters.append(&range.widget);
        filters.append(&types.widget);
        let header = adw::HeaderBar::builder()
            .title_widget(&filters)
            .centering_policy(adw::CenteringPolicy::Strict)
            .show_start_title_buttons(false)
            .css_classes(["dashboard-header"])
            .build();
        let expand_sidebar = button("sidebar-show-symbolic", "Expand sidebar");
        let refresh = button("view-refresh-symbolic", "Refresh dashboard");
        let menu = super::settings::menu_with_action(settings);
        header.pack_start(&expand_sidebar);
        header.pack_end(&menu);
        header.pack_end(&refresh);
        widget.append(&header);
        let unsubscribe_all = super::action_button("list-remove-symbolic", "Unsubscribe..");
        unsubscribe_all.remove_css_class("pill");
        unsubscribe_all.add_css_class("flat");
        let unsubscribe_status = horizontal("dashboard-unsubscribe-status", theme::SPACING);
        let unsubscribe_progress = label("", "");
        unsubscribe_progress.set_wrap(true);
        unsubscribe_progress.set_ellipsize(gtk::pango::EllipsizeMode::None);
        let unsubscribe_stop = super::action_button("process-stop-symbolic", "Stop");
        unsubscribe_status.append(&unsubscribe_progress);
        unsubscribe_status.append(&unsubscribe_stop);
        unsubscribe_status.set_visible(false);
        widget.append(&unsubscribe_status);
        let content = column("dashboard-sections");
        content.set_spacing(theme::DASHBOARD_SECTION_SPACING);
        content.set_valign(gtk::Align::Start);
        content.set_hexpand(true);
        let subscriptions = column("dashboard-subscriptions");
        subscriptions.set_spacing(theme::SMALL_SPACING);
        subscriptions.set_valign(gtk::Align::Start);
        subscriptions.set_visible(false);
        let layout = super::columns::Columns::new(
            &subscriptions,
            &content,
            theme::DASHBOARD_SUBSCRIPTIONS_WIDTH,
            theme::DASHBOARD_SECTION_SPACING,
        );
        layout.add_css_class("dashboard-content");
        let clamp = adw::Clamp::builder()
            .child(&layout)
            .maximum_size(theme::DASHBOARD_MAX_WIDTH)
            .tightening_threshold(theme::DASHBOARD_MAX_WIDTH)
            .build();
        let state = adw::StatusPage::builder()
            .icon_name("view-grid-symbolic")
            .title("Nothing in this range")
            .description("Try another date range or account.")
            .build();
        let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        stack.add_named(&scroll(&clamp), Some("cards"));
        stack.add_named(&state, Some("state"));
        stack.set_visible_child_name("state");
        widget.append(&stack);
        Self {
            widget,
            accounts,
            range,
            types,
            refresh,
            expand_sidebar,
            unsubscribe_all,
            unsubscribe_stop,
            unsubscribe_status,
            unsubscribe_progress,
            unsubscribe_blocked: std::cell::Cell::new(false),
            stack,
            state,
            content,
            subscriptions,
            sections: RefCell::default(),
            expanded_limits: Rc::default(),
            callbacks: Rc::default(),
        }
    }

    pub fn status(&self, title: &str, description: &str) {
        self.state.set_title(title);
        self.state.set_description(Some(description));
        self.stack.set_visible_child_name("state");
    }

    pub fn render(
        &self,
        entries: &[Entry],
        show_accounts: bool,
        pending: impl Fn(&Entry) -> bool,
    ) -> Vec<Tile> {
        let mut visible_sections = std::collections::HashSet::new();
        let mut has_subscriptions = false;
        let mut has_content = false;
        let mut previous_content: Option<gtk::Widget> = None;
        let mut tiles = Vec::new();
        let selected = self.types.selected();
        for section in std::iter::once(Section::Subscriptions)
            .chain(
                Section::ALL
                    .into_iter()
                    .filter(|section| *section != Section::Subscriptions),
            )
            .filter(|section| selected.contains(section))
        {
            let items: Vec<_> = entries
                .iter()
                .filter(|entry| entry.section() == section)
                .collect();
            if items.is_empty() {
                if let Some(view) = self.sections.borrow_mut().get_mut(&section) {
                    view.clear();
                }
                continue;
            }
            let (group, section_tiles) = self.section(section, items, show_accounts, &pending);
            let parent = if section == Section::Subscriptions {
                has_subscriptions = true;
                &self.subscriptions
            } else {
                has_content = true;
                &self.content
            };
            if group.parent().is_none() {
                parent.append(&group);
            }
            if section != Section::Subscriptions {
                if group.prev_sibling() != previous_content {
                    parent.reorder_child_after(&group, previous_content.as_ref());
                }
                previous_content = Some(group.clone().upcast());
            }
            visible_sections.insert(section);
            tiles.extend(section_tiles);
        }
        if self.unsubscribe_blocked.get()
            && let Some(view) = self.sections.borrow().get(&Section::Subscriptions)
        {
            for button in view.unsubscribe_buttons.values() {
                button.set_sensitive(false);
            }
        }
        for (section, view) in self.sections.borrow().iter() {
            view.widget.set_visible(visible_sections.contains(section));
        }
        self.subscriptions.set_visible(has_subscriptions);
        self.content.set_visible(has_content);
        if entries.is_empty() {
            self.status(
                "Nothing in this range",
                "Try another date range or account.",
            );
            return tiles;
        }
        if visible_sections.is_empty() {
            self.status(
                "No items of these types",
                "Choose other types or a different date range.",
            );
            return tiles;
        }
        self.stack.set_visible_child_name("cards");
        tiles
    }

    pub fn unsubscribe_pending(&self, id: &str, pending: bool) {
        if let Some(view) = self.sections.borrow_mut().get_mut(&Section::Subscriptions) {
            view.pending(id, pending);
            if let Some(button) = view.unsubscribe_buttons.get(id) {
                subscriptions::pending(button, pending);
                if self.unsubscribe_blocked.get() {
                    button.set_sensitive(false);
                }
            }
        }
    }

    pub fn unsubscribe_progress(&self, text: &str, active: bool, stopping: bool) {
        self.unsubscribe_blocked.set(active);
        if let Some(view) = self.sections.borrow().get(&Section::Subscriptions) {
            for button in view.unsubscribe_buttons.values() {
                let pending = button
                    .child()
                    .and_then(|child| child.last_child())
                    .and_downcast::<gtk::Label>()
                    .is_some_and(|label| label.text() == "Unsubscribing…");
                button.set_sensitive(!active && !pending);
            }
        }
        self.unsubscribe_all.set_sensitive(!active);
        self.unsubscribe_stop.set_visible(active);
        self.unsubscribe_stop.set_sensitive(!stopping);
        self.unsubscribe_progress.set_text(text);
        self.unsubscribe_status.set_visible(!text.is_empty());
    }

    pub fn connect_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }
}

pub fn navigation_button() -> gtk::Button {
    let button = super::sidebar::navigation_button("view-grid-symbolic", "Dashboard");
    button.add_css_class("dashboard-navigation");
    button.set_halign(gtk::Align::Fill);
    button
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use crate::models::{
        Message,
        calendar::Event,
        dashboard::{Content, Source},
    };
    use std::time::Duration;

    #[test]
    #[ignore = "Requires a private graphical session; presents fixture subscriptions and dashboard sections"]
    fn subscriptions_sit_left_of_the_first_section_and_stack_in_narrow_windows() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let css = gtk::CssProvider::new();
        css.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let view = Rc::new(Dashboard::new(&gtk::gio::SimpleAction::new(
            "settings", None,
        )));
        view.accounts.set_selected("", "All accounts");
        let source = Source {
            account: "fixture@example.com".into(),
            folder: "Newsletters".into(),
            message: Message {
                sender: "GNOME News <news@gnome.org>".into(),
                unsubscribe: Some(crate::models::Unsubscribe {
                    url: "https://example.com/unsubscribe".into(),
                    one_click: Some(true),
                }),
                ..Default::default()
            },
        };
        let entries = vec![
            Entry {
                recent_emails: 0,
                id: "subscription".into(),
                source: source.clone(),
                content: Content::Subscription,
                series: Vec::new(),
            },
            Entry {
                recent_emails: 0,
                id: "event".into(),
                source,
                content: Content::Event(Event {
                    name: "Design workshop".into(),
                    start: "2026-11-08T12:00:00Z".into(),
                    ..Default::default()
                }),
                series: Vec::new(),
            },
        ];
        let tiles = view.render(&entries, true, |_| false);
        assert_eq!(tiles.len(), 2);
        let button = tiles[0].unsubscribe.as_ref().unwrap();
        let button_text = |button: &gtk::Button| {
            button
                .child()
                .unwrap()
                .last_child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .text()
                .to_string()
        };
        assert_eq!(button_text(button), "Unsubscribe");
        assert!(button.has_css_class("pill"));
        assert_eq!(
            button
                .child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("list-remove-symbolic")
        );
        assert!(button.is_sensitive());
        let window = gtk::Window::builder()
            .title("Brevlada dashboard subscriptions diagnostic")
            .child(&view.widget)
            .default_width(1200)
            .default_height(800)
            .build();
        window.present();
        let settle = || {
            gtk::glib::MainContext::default()
                .block_on(gtk::glib::timeout_future(Duration::from_millis(200)))
        };
        settle();
        let group = view.subscriptions.first_child().unwrap();
        let controls = group.last_child().unwrap();
        let bulk_parent = view.unsubscribe_all.parent().unwrap();
        let card = group
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .first_child()
            .unwrap();
        let detached = Rc::new(std::cell::Cell::new(0));
        let changes = detached.clone();
        view.unsubscribe_all
            .connect_parent_notify(move |_| changes.set(changes.get() + 1));
        assert!(view.render(&entries, true, |_| false).is_empty());
        let mut count_update = entries.clone();
        count_update[0].recent_emails += 1;
        assert!(view.render(&count_update, true, |_| false).is_empty());
        let mut refreshed = entries.clone();
        refreshed.push(Entry {
            recent_emails: 0,
            id: "new-subscription".into(),
            ..entries[0].clone()
        });
        let added = view.render(&refreshed, true, |_| false);
        assert_eq!(added.len(), 1);
        assert_eq!(view.unsubscribe_all.parent().as_ref(), Some(&bulk_parent));
        assert_eq!(group.last_child().as_ref(), Some(&controls));
        assert_eq!(
            group
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .first_child()
                .as_ref(),
            Some(&card)
        );
        view.render(&entries, true, |_| false);
        assert_eq!(detached.get(), 0);
        assert_eq!(button_text(&view.unsubscribe_all), "Unsubscribe..");
        let subscriptions = view.subscriptions.compute_bounds(&window).unwrap();
        let sections = view.content.compute_bounds(&window).unwrap();
        assert!(subscriptions.x() + subscriptions.width() < sections.x());
        assert_eq!(subscriptions.y(), sections.y());
        if std::env::var_os("BREVLADA_DIAGNOSTIC_HOLD").is_some() {
            gtk::glib::MainContext::default()
                .block_on(gtk::glib::timeout_future(Duration::from_secs(8)));
        }
        window.set_default_size(420, 800);
        settle();
        let subscriptions = view.subscriptions.compute_bounds(&window).unwrap();
        let sections = view.content.compute_bounds(&window).unwrap();
        assert_eq!(subscriptions.x(), sections.x());
        assert!(subscriptions.y() + subscriptions.height() < sections.y());
        let tiles = view.render(&entries, true, |entry| {
            matches!(entry.content, Content::Subscription)
        });
        let button = tiles[0].unsubscribe.as_ref().unwrap();
        assert_eq!(button_text(button), "Unsubscribing…");
        assert!(!button.is_sensitive());
        view.render(&entries[1..], false, |_| false);
        settle();
        assert!(!view.subscriptions.get_visible());
        assert!(view.content.get_visible());
        assert_eq!(view.stack.visible_child_name().as_deref(), Some("cards"));
        view.render(&entries[..1], false, |_| false);
        settle();
        assert!(view.subscriptions.get_visible());
        assert!(!view.content.get_visible());
        assert_eq!(view.stack.visible_child_name().as_deref(), Some("cards"));
        let subscriptions: Rc<Vec<_>> = Rc::new(
            (0..27)
                .map(|index| Entry {
                    recent_emails: 0,
                    id: format!("subscription-{index}"),
                    ..entries[0].clone()
                })
                .collect(),
        );
        let weak = Rc::downgrade(&view);
        let items = subscriptions.clone();
        view.connect_changed(move || {
            if let Some(view) = weak.upgrade() {
                view.render(&items, true, |_| false);
            }
        });
        view.render(&subscriptions, true, |_| false);
        settle();
        let count = || {
            view.subscriptions
                .first_child()
                .unwrap()
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .observe_children()
                .n_items()
        };
        let more = || {
            view.subscriptions
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .first_child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Button>()
                .unwrap()
        };
        let less = || {
            view.subscriptions
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .last_child()
                .and_downcast::<gtk::Button>()
                .unwrap()
        };
        assert_eq!(count(), theme::DASHBOARD_ROWS as u32);
        assert!(more().get_visible());
        assert!(!less().get_visible());
        assert_eq!(button_text(&more()), "Show 10 more");
        more().emit_clicked();
        settle();
        assert_eq!(count(), 13);
        assert!(less().get_visible());
        assert_eq!(button_text(&more()), "Show 10 more");
        more().emit_clicked();
        settle();
        assert_eq!(count(), 23);
        assert_eq!(button_text(&more()), "Show 4 more");
        more().emit_clicked();
        settle();
        assert_eq!(count(), 27);
        assert!(!more().get_visible());
        assert!(less().get_visible());
        assert_eq!(button_text(&less()), "Show less");
        less().emit_clicked();
        settle();
        assert_eq!(count(), theme::DASHBOARD_ROWS as u32);
        assert!(!less().get_visible());
        assert_eq!(button_text(&more()), "Show 10 more");
        view.render(&[], false, |_| false);
        assert_eq!(view.stack.visible_child_name().as_deref(), Some("state"));
        window.close();
    }

    #[test]
    #[ignore = "Requires a private graphical session; presents fixture dashboard sections"]
    fn sections_limit_rows_expand_and_keep_equal_grid_spacing() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let css = gtk::CssProvider::new();
        css.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let view = Rc::new(Dashboard::new(&gtk::gio::SimpleAction::new(
            "settings", None,
        )));
        view.accounts.set_selected("", "All accounts");
        let entries: Rc<Vec<_>> = Rc::new(
            (0..29)
                .map(|index| Entry {
                    recent_emails: 0,
                    id: index.to_string(),
                    series: Vec::new(),
                    source: Source {
                        account: "fixture@example.com".into(),
                        folder: "INBOX".into(),
                        message: Message {
                            sender: "Events <events@example.com>".into(),
                            ..Default::default()
                        },
                    },
                    content: Content::Event(Event {
                        name: format!("Event {index}"),
                        start: "2026-11-06T12:00:00Z".into(),
                        ..Default::default()
                    }),
                })
                .collect(),
        );
        let weak = Rc::downgrade(&view);
        let items = entries.clone();
        view.connect_changed(move || {
            if let Some(view) = weak.upgrade() {
                view.render(&items, true, |_| false);
            }
        });
        view.render(&entries, true, |_| false);
        let window = gtk::Window::builder()
            .child(&view.widget)
            .default_width(1000)
            .default_height(900)
            .build();
        window.present();
        let settle = || {
            gtk::glib::MainContext::default()
                .block_on(gtk::glib::timeout_future(Duration::from_millis(200)))
        };
        settle();
        let grid = || {
            view.content
                .first_child()
                .unwrap()
                .first_child()
                .unwrap()
                .next_sibling()
                .and_downcast::<super::super::calendar::Flow>()
                .unwrap()
        };
        assert_eq!(grid().visible_items(), 6);
        let first = grid().first_child().unwrap();
        let second = first.next_sibling().unwrap();
        let third = second.next_sibling().unwrap();
        let a = first.compute_bounds(&grid()).unwrap();
        let b = second.compute_bounds(&grid()).unwrap();
        let c = third.compute_bounds(&grid()).unwrap();
        assert_eq!(b.x() - a.x() - a.width(), theme::SPACING as f32);
        assert_eq!(c.y() - a.y() - a.height(), theme::SPACING as f32);
        let more = || {
            view.content
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Button>()
                .unwrap()
        };
        let less = || {
            view.content
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .last_child()
                .and_downcast::<gtk::Button>()
                .unwrap()
        };
        let text = |button: &gtk::Button| {
            button
                .child()
                .unwrap()
                .last_child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .text()
                .to_string()
        };
        assert!(more().get_visible());
        assert!(!less().get_visible());
        assert_eq!(text(&more()), "Show 10 more");
        more().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 16);
        assert!(less().get_visible());
        more().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 26);
        assert_eq!(text(&more()), "Show 3 more");
        more().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 29);
        assert!(!more().get_visible());
        assert!(less().get_visible());
        less().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 6);
        window.set_default_size(420, 900);
        settle();
        assert_eq!(grid().visible_items(), 3);
        assert_eq!(text(&more()), "Show 10 more");
        assert!(
            grid()
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .compute_bounds(&grid())
                .unwrap()
                .y()
                > 0.0
        );
        more().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 13);
        assert_eq!(text(&more()), "Show 10 more");
        less().emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 3);
        window.close();
    }
}
