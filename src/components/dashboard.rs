mod cards;
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
    stack: gtk::Stack,
    state: adw::StatusPage,
    content: gtk::Box,
    subscriptions: gtk::Box,
    unsubscribe_buttons: RefCell<HashMap<String, gtk::Button>>,
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
            stack,
            state,
            content,
            subscriptions,
            unsubscribe_buttons: RefCell::default(),
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
        if let Some(focus) = self
            .widget
            .root()
            .and_downcast::<gtk::Window>()
            .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window))
            && (focus.is_ancestor(&self.content) || focus.is_ancestor(&self.subscriptions))
        {
            self.refresh.grab_focus();
        }
        self.unsubscribe_buttons.borrow_mut().clear();
        super::clear(&self.content);
        super::clear(&self.subscriptions);
        self.subscriptions.set_visible(false);
        if entries.is_empty() {
            self.status(
                "Nothing in this range",
                "Try another date range or account.",
            );
            return Vec::new();
        }
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
                continue;
            }
            let (group, section_tiles) = self.section(section, items, show_accounts, &pending);
            for tile in &section_tiles {
                if let Some(button) = &tile.unsubscribe {
                    self.unsubscribe_buttons
                        .borrow_mut()
                        .insert(tile.entry.id.clone(), button.clone());
                }
            }
            if section == Section::Subscriptions {
                self.subscriptions.append(&group);
                self.subscriptions.set_visible(true);
            } else {
                self.content.append(&group);
            }
            tiles.extend(section_tiles);
        }
        self.content
            .set_visible(self.content.first_child().is_some());
        if tiles.is_empty() {
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
        if let Some(button) = self.unsubscribe_buttons.borrow().get(id) {
            subscriptions::pending(button, pending);
        }
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
                id: "subscription".into(),
                source: source.clone(),
                content: Content::Subscription,
                series: Vec::new(),
            },
            Entry {
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
