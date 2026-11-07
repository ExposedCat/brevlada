use crate::theme;
use gtk::{glib, prelude::*, subclass::prelude::*};
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Columns {
        pub sidebar_width: Cell<i32>,
        pub spacing: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Columns {
        const NAME: &'static str = "BrevladaColumns";
        type Type = super::Columns;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Columns {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Columns {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let children = self.obj().children();
            if orientation == gtk::Orientation::Horizontal {
                let sizes: Vec<_> = children
                    .iter()
                    .map(|child| child.measure(orientation, -1))
                    .collect();
                let minimum = sizes.iter().map(|size| size.0).max().unwrap_or(0);
                let natural = match sizes.as_slice() {
                    [sidebar, content] => {
                        sidebar.0.max(self.sidebar_width.get()) + self.spacing.get() + content.1
                    }
                    [child] => child.1,
                    _ => 0,
                };
                return (minimum, natural.max(minimum), -1, -1);
            }
            let height = self
                .obj()
                .arrangement(for_size)
                .iter()
                .map(|(_, allocation)| allocation.y() + allocation.height())
                .max()
                .unwrap_or(0);
            (height, height, -1, -1)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            for (child, allocation) in self.obj().arrangement(width) {
                child.size_allocate(&allocation, -1);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            for child in self.obj().children() {
                self.obj().snapshot_child(&child, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct Columns(ObjectSubclass<imp::Columns>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Columns {
    pub fn new(
        sidebar: &impl IsA<gtk::Widget>,
        content: &impl IsA<gtk::Widget>,
        sidebar_width: i32,
        spacing: i32,
    ) -> Self {
        let widget: Self = glib::Object::builder()
            .property("hexpand", true)
            .property("valign", gtk::Align::Start)
            .build();
        widget.imp().sidebar_width.set(sidebar_width);
        widget.imp().spacing.set(spacing);
        sidebar.set_parent(&widget);
        content.set_parent(&widget);
        widget
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

    fn arrangement(&self, width: i32) -> Vec<(gtk::Widget, gtk::Allocation)> {
        let children = self.children();
        let spacing = self.imp().spacing.get();
        let sidebar_width = children
            .first()
            .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
            .unwrap_or(0)
            .max(self.imp().sidebar_width.get());
        let content_width = children
            .get(1)
            .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
            .unwrap_or(0)
            .max(theme::CARD_COLUMN_WIDTH);
        let horizontal = children.len() == 2 && width >= sidebar_width + spacing + content_width;
        let mut y = 0;
        children
            .into_iter()
            .enumerate()
            .map(|(index, child)| {
                let (x, child_width) = if horizontal {
                    if index == 0 {
                        (0, sidebar_width)
                    } else {
                        (sidebar_width + spacing, width - sidebar_width - spacing)
                    }
                } else {
                    (0, width)
                };
                let height = child.measure(gtk::Orientation::Vertical, child_width).1;
                let x = if self.direction() == gtk::TextDirection::Rtl {
                    width - x - child_width
                } else {
                    x
                };
                let allocation = gtk::Allocation::new(x, y, child_width, height);
                if !horizontal {
                    y += height + spacing;
                }
                (child, allocation)
            })
            .collect()
    }
}
