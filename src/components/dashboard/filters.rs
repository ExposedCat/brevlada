use super::*;
use crate::models::dashboard::Section;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

pub struct Types {
    pub widget: gtk::MenuButton,
    checks: Vec<(Section, gtk::CheckButton)>,
    callbacks: RefCell<Vec<Box<dyn Fn()>>>,
}

impl Types {
    pub fn new() -> Rc<Self> {
        let content = column("dashboard-type-options");
        content.set_spacing(theme::SMALL_SPACING);
        let popover = gtk::Popover::builder().child(&content).build();
        let widget = gtk::MenuButton::builder()
            .label("All types")
            .popover(&popover)
            .css_classes(["flat"])
            .build();
        let mut checks = Vec::new();
        for section in Section::ALL {
            let check = gtk::CheckButton::with_label(section.title());
            check.set_active(true);
            content.append(&check);
            checks.push((section, check));
        }
        let ui = Rc::new(Self {
            widget,
            checks,
            callbacks: RefCell::default(),
        });
        for (_, check) in &ui.checks {
            let weak = Rc::downgrade(&ui);
            check.connect_toggled(move |_| {
                if let Some(ui) = weak.upgrade() {
                    let selected = ui.selected();
                    let title = if selected.len() == Section::ALL.len() {
                        "All types".into()
                    } else if selected.len() == 1 {
                        selected.iter().next().unwrap().title().into()
                    } else {
                        format!("{} types", selected.len())
                    };
                    ui.widget.set_label(&title);
                    for callback in ui.callbacks.borrow().iter() {
                        callback();
                    }
                }
            });
        }
        ui
    }

    pub fn selected(&self) -> HashSet<Section> {
        self.checks
            .iter()
            .filter_map(|(section, check)| check.is_active().then_some(*section))
            .collect()
    }

    pub fn connect_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }
}
