use crate::theme;
use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Flow)]
    pub struct Flow {
        pub spacing: Cell<i32>,
        pub row_spacing: Cell<i32>,
        pub row_limit: Cell<usize>,
        #[property(get)]
        pub visible_items: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Flow {
        const NAME: &'static str = "BrevladaCalendarFlow";
        type Type = super::Flow;
        type ParentType = gtk::Widget;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Flow {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Flow {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let children = self.obj().children();
            let minimum = children
                .iter()
                .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
                .max()
                .unwrap_or(0);
            let preferred = minimum.max(theme::CARD_COLUMN_WIDTH);
            let natural = if children.len() > 1 {
                preferred * 2 + self.spacing.get()
            } else {
                preferred
            };
            if orientation == gtk::Orientation::Horizontal {
                return (minimum, natural, -1, -1);
            }
            let width = if for_size < 0 { natural } else { for_size };
            let (_, height) = self.obj().arrangement(&children, width);
            (height, height, -1, -1)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let children = self.obj().children();
            let (allocations, _) = self.obj().arrangement(&children, width);
            for (index, child) in children.iter().enumerate() {
                child.set_child_visible(index < allocations.len());
            }
            if self.visible_items.replace(allocations.len() as u32) != allocations.len() as u32 {
                self.obj().notify_visible_items();
            }
            for (child, allocation) in children.iter().zip(allocations) {
                child.size_allocate(&allocation, -1);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.obj().children() {
                if child.is_child_visible() {
                    self.obj().snapshot_child(&child, snapshot);
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct Flow(ObjectSubclass<imp::Flow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Flow {
    pub fn new(events: &[gtk::Widget]) -> Self {
        let flow: Self = glib::Object::builder()
            .property("hexpand", true)
            .property("vexpand", false)
            .property("valign", gtk::Align::Start)
            .build();
        flow.add_css_class("calendar-flow");
        flow.imp().spacing.set(theme::SMALL_SPACING);
        for event in events {
            event.set_parent(&flow);
        }
        flow
    }

    pub fn grid(events: &[gtk::Widget], rows: usize) -> Self {
        let flow = Self::new(events);
        flow.imp().spacing.set(theme::SPACING);
        flow.imp().row_spacing.set(theme::SPACING);
        flow.imp().row_limit.set(rows);
        flow
    }

    fn children(&self) -> Vec<gtk::Widget> {
        let mut children = Vec::new();
        let mut child = self.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if widget.get_visible() {
                children.push(widget);
            }
        }
        children
    }

    fn arrangement(&self, children: &[gtk::Widget], width: i32) -> (Vec<gtk::Allocation>, i32) {
        let minimum = children
            .iter()
            .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
            .max()
            .unwrap_or(0)
            .max(theme::CARD_COLUMN_WIDTH);
        let spacing = self.imp().spacing.get();
        let columns = if children.len() > 1 && width >= minimum * 2 + spacing {
            2
        } else {
            1
        };
        let column_width = (width - (columns as i32 - 1) * spacing) / columns as i32;
        let limit = self.imp().row_limit.get();
        let count = if limit == 0 {
            children.len()
        } else {
            children.len().min(limit * columns)
        };
        let mut allocations = Vec::new();
        let mut y = 0;
        for row in children[..count].chunks(columns) {
            let row_start = allocations.len();
            let mut row_height = 0;
            for (column, child) in row.iter().enumerate() {
                let x = column as i32 * (column_width + spacing);
                let child_width = if column + 1 == columns {
                    width - x
                } else {
                    column_width
                };
                let height = child.measure(gtk::Orientation::Vertical, child_width).1;
                let x = if self.direction() == gtk::TextDirection::Rtl {
                    width - x - child_width
                } else {
                    x
                };
                allocations.push(gtk::Allocation::new(x, y, child_width, height));
                row_height = row_height.max(height);
            }
            for allocation in &mut allocations[row_start..] {
                allocation.set_height(row_height);
            }
            y += row_height + self.imp().row_spacing.get();
        }
        (allocations, (y - self.imp().row_spacing.get()).max(0))
    }
}

#[cfg(test)]
mod tests;
