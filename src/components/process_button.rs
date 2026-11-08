mod queue;
mod ring;
mod samples;

use crate::theme;
use adw::prelude::*;
use queue::Item;
use std::{cell::Cell, rc::Rc};

type OpenLink = Rc<dyn Fn(&str)>;

struct Preview {
    name: &'static str,
    percent: u32,
    current: &'static str,
    completed: &'static str,
    items: Vec<Item>,
}

fn new(preview: Preview, open_link: OpenLink) -> gtk::MenuButton {
    let fraction = f64::from(preview.percent) / 100.0;
    let content = super::horizontal("process-button-content", theme::SPACING);
    let circle = ring::new(fraction);
    let name = super::label(preview.name, "process-button-name");
    let percent = super::label(&format!("{}%", preview.percent), "process-percentage");
    percent.set_hexpand(false);
    content.append(&circle);
    content.append(&name);
    content.append(&percent);
    let detail = super::column("process-popover-content");
    detail.set_spacing(theme::SPACING);
    detail.set_size_request(theme::PROCESS_POPOVER_WIDTH, -1);
    detail.append(&super::label(preview.name, "title-3"));
    let current = super::label(preview.current, "heading");
    current.set_wrap(true);
    current.set_ellipsize(gtk::pango::EllipsizeMode::None);
    detail.append(&current);
    let progress = gtk::ProgressBar::builder().fraction(fraction).build();
    detail.append(&progress);
    let completed = super::label(preview.completed, "dim-label");
    detail.append(&completed);
    let failed = preview
        .items
        .iter()
        .filter(|item| matches!(item.state, queue::State::Failed { .. }))
        .count();
    let queue_heading = super::label(
        &if failed == 0 {
            "Queue".into()
        } else {
            format!("Queue · {failed} failed")
        },
        "heading",
    );
    detail.append(&queue_heading);
    let queue = queue::new(&preview.items, open_link);
    detail.append(&queue);
    let actions = super::horizontal("process-popover-actions", theme::SPACING);
    actions.set_homogeneous(true);
    let pause = gtk::Button::with_label("Pause");
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("destructive-action");
    actions.append(&pause);
    actions.append(&cancel);
    detail.append(&actions);
    let popover = gtk::Popover::builder()
        .child(&detail)
        .position(gtk::PositionType::Right)
        .has_arrow(true)
        .build();
    popover.add_css_class("process-popover");
    let button = gtk::MenuButton::builder()
        .child(&content)
        .popover(&popover)
        .direction(gtk::ArrowType::Right)
        .hexpand(true)
        .tooltip_text(format!("{} details", preview.name))
        .css_classes(["flat", "process-button"])
        .build();
    let paused = Rc::new(Cell::new(false));
    pause.connect_clicked(move |button| {
        let is_paused = !paused.get();
        paused.set(is_paused);
        button.set_label(if is_paused { "Resume" } else { "Pause" });
        if is_paused {
            completed.set_text("Paused · progress is saved");
            circle.add_css_class("dim-label");
            name.add_css_class("dim-label");
        } else {
            completed.set_text(preview.completed);
            circle.remove_css_class("dim-label");
            name.remove_css_class("dim-label");
        }
    });
    let target = button.downgrade();
    let bubble = popover.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(popover) = bubble.upgrade() {
            popover.popdown();
        }
        if let Some(button) = target.upgrade() {
            button.set_visible(false);
        }
    });
    button
}

pub fn previews() -> gtk::Box {
    let widget = super::column("process-previews");
    widget.set_spacing(theme::SMALL_SPACING);
    let buttons: Vec<_> = samples::previews()
        .into_iter()
        .map(|preview| new(preview, Rc::new(super::links::open)))
        .collect();
    for button in &buttons {
        widget.append(button);
    }
    let targets: Vec<_> = buttons.iter().map(|button| button.downgrade()).collect();
    for button in &buttons {
        let target = widget.downgrade();
        let buttons = targets.clone();
        button.connect_visible_notify(move |_| {
            if let Some(widget) = target.upgrade() {
                widget.set_visible(
                    buttons
                        .iter()
                        .filter_map(|button| button.upgrade())
                        .any(|button| button.is_visible()),
                );
            }
        });
    }
    widget
}
