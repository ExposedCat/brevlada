use super::{column, horizontal};
use crate::{
    models::{dashboard::Range, search::Filters},
    theme,
};
use adw::prelude::*;
use std::{cell::RefCell, rc::Rc};

pub struct DateRange {
    pub widget: gtk::MenuButton,
    range: RefCell<Range>,
    presets: Vec<(Range, gtk::ToggleButton)>,
    custom: gtk::Revealer,
    from: gtk::Entry,
    to: gtk::Entry,
    error: gtk::Label,
    apply: gtk::Button,
    callbacks: RefCell<Vec<Box<dyn Fn()>>>,
}

impl DateRange {
    pub fn new() -> Rc<Self> {
        let content = column("date-range-popover");
        content.set_spacing(theme::SPACING);
        let popover = gtk::Popover::builder().child(&content).build();
        let widget = gtk::MenuButton::builder()
            .label(Range::default().label())
            .tooltip_text("Item date range")
            .popover(&popover)
            .css_classes(["flat"])
            .build();
        let choices = horizontal("linked", 0);
        let mut presets: Vec<(Range, gtk::ToggleButton)> = Vec::new();
        for range in Range::PRESETS {
            let button = gtk::ToggleButton::with_label(range.label());
            if let Some((_, first)) = presets.first() {
                button.set_group(Some(first));
            }
            button.set_active(range == Range::default());
            choices.append(&button);
            presets.push((range, button));
        }
        content.append(&choices);
        let custom_button = gtk::Button::with_label("Custom date range…");
        content.append(&custom_button);
        let custom = gtk::Revealer::new();
        let dates = column("date-range-custom");
        dates.set_spacing(theme::SMALL_SPACING);
        custom.set_child(Some(&dates));
        content.append(&custom);
        let (from, to) = super::search_filters::date_inputs(&dates);
        let today = chrono::Local::now().date_naive();
        from.set_text(&(today - chrono::Duration::days(30)).to_string());
        to.set_text(&today.to_string());
        let error = super::error_label();
        dates.append(&error);
        let apply = gtk::Button::with_label("Apply");
        apply.add_css_class("suggested-action");
        dates.append(&apply);
        let ui = Rc::new(Self {
            widget,
            range: RefCell::new(Range::default()),
            presets,
            custom,
            from,
            to,
            error,
            apply,
            callbacks: RefCell::default(),
        });
        for (range, button) in &ui.presets {
            let weak = Rc::downgrade(&ui);
            let range = *range;
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.select(range);
                }
            });
        }
        let weak = Rc::downgrade(&ui);
        custom_button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if !matches!(ui.range(), Range::Custom { .. }) {
                    let now = chrono::Utc::now().timestamp();
                    let past = ui.range().past(now);
                    let future = ui.range().future(now);
                    let date = |timestamp| {
                        chrono::DateTime::from_timestamp(timestamp, 0)
                            .unwrap()
                            .with_timezone(&chrono::Local)
                            .date_naive()
                            .to_string()
                    };
                    ui.from.set_text(&date(past.from));
                    ui.to.set_text(&date(future.until - 1));
                }
                ui.custom.set_reveal_child(true);
                ui.validate();
            }
        });
        for entry in [&ui.from, &ui.to] {
            let weak = Rc::downgrade(&ui);
            entry.connect_changed(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.validate();
                }
            });
        }
        let weak = Rc::downgrade(&ui);
        ui.apply.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade()
                && let Ok((Some(from), Some(to))) = Filters::dates(&ui.from.text(), &ui.to.text())
            {
                ui.select(Range::Custom { from, to });
            }
        });
        ui
    }

    pub fn range(&self) -> Range {
        *self.range.borrow()
    }

    pub fn connect_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }

    fn select(&self, range: Range) {
        let changed = self.range.replace(range) != range;
        self.widget.set_label(range.label());
        self.widget.set_tooltip_text(Some(&match range {
            Range::Custom { from, to } => format!("Item dates: {from} – {to}"),
            _ => "Item date range".into(),
        }));
        for (preset, button) in &self.presets {
            button.set_active(*preset == range);
        }
        self.custom
            .set_reveal_child(matches!(range, Range::Custom { .. }));
        self.widget.popdown();
        if changed {
            for callback in self.callbacks.borrow().iter() {
                callback();
            }
        }
    }

    fn validate(&self) {
        let error = match Filters::dates(&self.from.text(), &self.to.text()) {
            Ok((Some(_), Some(_))) => None,
            Ok(_) => Some("Choose both dates"),
            Err(error) => Some(error),
        };
        self.error.set_label(error.unwrap_or(""));
        self.error.set_visible(error.is_some());
        self.apply.set_sensitive(error.is_none());
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a private graphical session; presents only a date filter fixture"]
    fn presets_and_custom_dates_validate_and_publish_the_selected_range() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let ui = DateRange::new();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let observed = changes.clone();
        let weak = Rc::downgrade(&ui);
        ui.connect_changed(move || observed.borrow_mut().push(weak.upgrade().unwrap().range()));
        for (range, button) in &ui.presets {
            button.emit_clicked();
            assert_eq!(ui.range(), *range);
            assert_eq!(ui.widget.label().as_deref(), Some(range.label()));
        }
        assert_eq!(changes.borrow().len(), 5);
        let window = gtk::Window::builder()
            .child(&ui.widget)
            .default_width(400)
            .build();
        window.present();
        let settle = || {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
            while std::time::Instant::now() < until {
                gtk::glib::MainContext::default().iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        settle();
        ui.widget.popup();
        let popover = ui.widget.popover().unwrap();
        let content = popover.child().unwrap();
        content
            .first_child()
            .unwrap()
            .next_sibling()
            .and_downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert!(ui.custom.reveals_child());
        ui.from.set_text("2026-11-06");
        ui.to.set_text("2026-11-04");
        assert!(!ui.apply.is_sensitive());
        assert!(ui.error.is_visible());
        ui.from.set_text("");
        assert!(!ui.apply.is_sensitive());
        ui.from.set_text("2026-11-01");
        assert!(ui.apply.is_sensitive());
        let picker = ui
            .from
            .parent()
            .unwrap()
            .last_child()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        picker.set_active(true);
        let calendar = ui
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
            .select_day(&gtk::glib::DateTime::from_iso8601("2026-11-02T12:00:00Z", None).unwrap());
        assert_eq!(ui.from.text(), "2026-11-02");
        settle();
        if let Ok(path) = std::env::var("BREVLADA_RANGE_SCREENSHOT") {
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&popover)).snapshot(
                &snapshot,
                popover.width() as f64,
                popover.height() as f64,
            );
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(path)
                .unwrap();
        }
        ui.apply.emit_clicked();
        let expected = Range::Custom {
            from: chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            to: chrono::NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
        };
        assert_eq!(ui.range(), expected);
        assert_eq!(changes.borrow().last(), Some(&expected));
        assert_eq!(ui.widget.label().as_deref(), Some("Custom"));
        window.close();
    }
}
