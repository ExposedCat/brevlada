use super::*;
use std::cell::Cell;

pub(super) struct Controls {
    pub widget: gtk::Box,
    more: gtk::Button,
    less: gtk::Button,
    shown: Cell<usize>,
    count: Cell<usize>,
}

impl Controls {
    pub fn new(dashboard: &Dashboard, section: Section) -> Rc<Self> {
        let widget = column("dashboard-section-controls");
        widget.set_spacing(theme::SMALL_SPACING);
        let more = super::super::action_button("pan-down-symbolic", "");
        more.remove_css_class("pill");
        more.add_css_class("flat");
        if section == Section::Subscriptions {
            let actions = horizontal("dashboard-subscription-controls", theme::SMALL_SPACING);
            actions.append(&more);
            actions.append(&dashboard.unsubscribe_all);
            widget.append(&actions);
        } else {
            widget.append(&more);
        }
        let less = super::super::action_button("pan-up-symbolic", "Show less");
        less.remove_css_class("pill");
        less.add_css_class("flat");
        widget.append(&less);
        let controls = Rc::new(Self {
            widget,
            more,
            less,
            shown: Cell::new(0),
            count: Cell::new(0),
        });
        let weak = Rc::downgrade(&controls);
        let expanded_limits = dashboard.expanded_limits.clone();
        let callbacks = dashboard.callbacks.clone();
        controls.more.connect_clicked(move |_| {
            if let Some(controls) = weak.upgrade() {
                let next = controls.shown.get()
                    + controls
                        .count
                        .get()
                        .saturating_sub(controls.shown.get())
                        .min(theme::DASHBOARD_PAGE_SIZE);
                expanded_limits.borrow_mut().insert(section, next);
                for callback in callbacks.borrow().iter() {
                    callback();
                }
            }
        });
        let expanded_limits = dashboard.expanded_limits.clone();
        let callbacks = dashboard.callbacks.clone();
        controls.less.connect_clicked(move |_| {
            expanded_limits.borrow_mut().remove(&section);
            for callback in callbacks.borrow().iter() {
                callback();
            }
        });
        controls
    }

    pub fn update(&self, count: usize, shown: usize, expanded: bool) {
        self.count.set(count);
        self.less.set_visible(expanded);
        self.show(shown);
    }

    pub fn show(&self, shown: usize) {
        self.shown.set(shown);
        let remaining = self
            .count
            .get()
            .saturating_sub(shown)
            .min(theme::DASHBOARD_PAGE_SIZE);
        let label = self
            .more
            .child()
            .and_then(|content| content.last_child())
            .and_downcast::<gtk::Label>()
            .unwrap();
        label.set_text(&format!("Show {remaining} more"));
        self.more.set_visible(remaining > 0);
    }
}
