use super::*;
use std::cell::Cell;

struct Controls {
    widget: gtk::Box,
    more: gtk::Button,
    shown: Rc<Cell<usize>>,
}

fn update_more(button: &gtk::Button, shown: usize, count: usize) {
    let remaining = count.saturating_sub(shown).min(theme::DASHBOARD_PAGE_SIZE);
    let label = button
        .child()
        .and_then(|content| content.last_child())
        .and_downcast::<gtk::Label>()
        .unwrap();
    label.set_text(&format!("Show {remaining} more"));
    button.set_visible(remaining > 0);
}

impl Dashboard {
    pub(super) fn section(
        &self,
        section: Section,
        items: Vec<&Entry>,
        show_accounts: bool,
        pending: &impl Fn(&Entry) -> bool,
    ) -> (gtk::Box, Vec<Tile>) {
        let group = column("dashboard-section");
        group.set_spacing(theme::SMALL_SPACING);
        let heading = horizontal("dashboard-section-heading", theme::SMALL_SPACING);
        heading.append(&label(section.title(), "title-3"));
        group.append(&heading);
        let count = items.len();
        let expanded_limit = self.expanded_limits.borrow().get(&section).copied();
        let subscriptions = section == Section::Subscriptions;
        let limit =
            expanded_limit.unwrap_or(theme::DASHBOARD_ROWS * if subscriptions { 1 } else { 2 });
        let tiles: Vec<_> = items
            .into_iter()
            .take(limit)
            .map(|entry| Tile::new(entry, show_accounts, pending(entry)))
            .collect();
        let controls =
            self.section_controls(section, count, limit.min(count), expanded_limit.is_some());
        if subscriptions {
            let cards = column("dashboard-subscription-cards");
            cards.set_spacing(theme::SPACING);
            for tile in &tiles {
                cards.append(&tile.widget);
            }
            group.append(&cards);
        } else {
            let widgets: Vec<_> = tiles
                .iter()
                .map(|tile| tile.widget.clone().upcast())
                .collect();
            let flow = super::super::calendar::Flow::grid(
                &widgets,
                if expanded_limit.is_some() {
                    0
                } else {
                    theme::DASHBOARD_ROWS
                },
            );
            let target = controls.more.downgrade();
            let shown = controls.shown.clone();
            flow.connect_visible_items_notify(move |flow| {
                shown.set(flow.visible_items() as usize);
                if let Some(button) = target.upgrade() {
                    update_more(&button, shown.get(), count);
                }
            });
            group.append(&flow);
        }
        group.append(&controls.widget);
        (group, tiles)
    }

    fn section_controls(
        &self,
        section: Section,
        count: usize,
        visible: usize,
        expanded: bool,
    ) -> Controls {
        let widget = column("dashboard-section-controls");
        widget.set_spacing(theme::SMALL_SPACING);
        let shown = Rc::new(Cell::new(visible));
        let more = super::super::action_button("pan-down-symbolic", "");
        more.remove_css_class("pill");
        more.add_css_class("flat");
        update_more(&more, visible, count);
        let expanded_limits = self.expanded_limits.clone();
        let visible = shown.clone();
        let callbacks = self.callbacks.clone();
        more.connect_clicked(move |_| {
            let next = visible.get()
                + count
                    .saturating_sub(visible.get())
                    .min(theme::DASHBOARD_PAGE_SIZE);
            expanded_limits.borrow_mut().insert(section, next);
            for callback in callbacks.borrow().iter() {
                callback();
            }
        });
        widget.append(&more);
        let less = super::super::action_button("pan-up-symbolic", "Show less");
        less.remove_css_class("pill");
        less.add_css_class("flat");
        less.set_visible(expanded);
        let expanded_limits = self.expanded_limits.clone();
        let callbacks = self.callbacks.clone();
        less.connect_clicked(move |_| {
            expanded_limits.borrow_mut().remove(&section);
            for callback in callbacks.borrow().iter() {
                callback();
            }
        });
        widget.append(&less);
        Controls {
            widget,
            more,
            shown,
        }
    }
}
