mod cards;
mod filters;
mod series;

use super::{account_switcher::AccountSwitcher, button, column, horizontal, label, scroll};
use crate::{
    models::dashboard::{Entry, Section},
    theme,
};
use adw::prelude::*;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

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
    expanded: Rc<RefCell<HashSet<Section>>>,
    callbacks: Changed,
}

impl Dashboard {
    pub fn new() -> Self {
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
        header.pack_start(&expand_sidebar);
        header.pack_end(&refresh);
        widget.append(&header);
        let content = column("dashboard-content");
        content.set_spacing(theme::DASHBOARD_SECTION_SPACING);
        content.set_valign(gtk::Align::Start);
        let clamp = adw::Clamp::builder()
            .child(&content)
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
            expanded: Rc::default(),
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
        super::clear(&self.content);
        if entries.is_empty() {
            self.status(
                "Nothing in this range",
                "Try another date range or account.",
            );
            return Vec::new();
        }
        let mut tiles = Vec::new();
        let selected = self.types.selected();
        for section in Section::ALL
            .into_iter()
            .filter(|section| selected.contains(section))
        {
            let items: Vec<_> = entries
                .iter()
                .filter(|entry| entry.section() == section)
                .collect();
            if items.is_empty() {
                continue;
            }
            let group = column("dashboard-section");
            group.set_spacing(theme::SMALL_SPACING);
            let heading = horizontal("dashboard-section-heading", theme::SMALL_SPACING);
            heading.append(&label(section.title(), "title-3"));
            group.append(&heading);
            let mut widgets = Vec::new();
            let count = items.len();
            let expanded = self.expanded.borrow().contains(&section);
            for entry in items.into_iter().take(if expanded {
                count
            } else {
                theme::DASHBOARD_ROWS * 2
            }) {
                let tile = Tile::new(entry, show_accounts, pending(entry));
                widgets.push(tile.widget.clone().upcast());
                tiles.push(tile);
            }
            let flow = super::calendar::Flow::grid(
                &widgets,
                if expanded { 0 } else { theme::DASHBOARD_ROWS },
            );
            group.append(&flow);
            let more = super::action_button(
                if expanded {
                    "pan-up-symbolic"
                } else {
                    "pan-down-symbolic"
                },
                &if expanded {
                    "Show less".into()
                } else {
                    format!("Show all {count}")
                },
            );
            more.remove_css_class("pill");
            more.add_css_class("flat");
            more.set_visible(expanded || count > theme::DASHBOARD_ROWS);
            let target = more.downgrade();
            flow.connect_visible_items_notify(move |flow| {
                if let Some(button) = target.upgrade() {
                    button.set_visible(expanded || count > flow.visible_items() as usize);
                }
            });
            let expanded_sections = self.expanded.clone();
            let callbacks = self.callbacks.clone();
            more.connect_clicked(move |_| {
                if expanded {
                    expanded_sections.borrow_mut().remove(&section);
                } else {
                    expanded_sections.borrow_mut().insert(section);
                }
                for callback in callbacks.borrow().iter() {
                    callback();
                }
            });
            group.append(&more);
            self.content.append(&group);
        }
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
        let view = Rc::new(Dashboard::new());
        view.accounts.set_selected("", "All accounts");
        let entries: Rc<Vec<_>> = Rc::new(
            (0..10)
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
        let more = view
            .content
            .first_child()
            .unwrap()
            .last_child()
            .and_downcast::<gtk::Button>()
            .unwrap();
        assert!(more.is_visible());
        more.emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 10);
        view.content
            .first_child()
            .unwrap()
            .last_child()
            .and_downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        settle();
        assert_eq!(grid().visible_items(), 6);
        window.set_default_size(420, 900);
        settle();
        assert_eq!(grid().visible_items(), 3);
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
        window.close();
    }
}
