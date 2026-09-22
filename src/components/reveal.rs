use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    pub struct Reveal {
        pub child: RefCell<Option<gtk::Widget>>,
        pub progress: Cell<f64>,
        pub animation: RefCell<Option<adw::TimedAnimation>>,
    }

    impl Default for Reveal {
        fn default() -> Self {
            Self {
                child: RefCell::new(None),
                progress: Cell::new(1.0),
                animation: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Reveal {
        const NAME: &'static str = "BrevladaReveal";
        type Type = super::Reveal;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for Reveal {
        fn dispose(&self) {
            if let Some(animation) = self.animation.borrow_mut().take() {
                animation.pause();
            }
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Reveal {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            if let Some(child) = self.child.borrow().as_ref() {
                snapshot.push_clip(&gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    widget.width() as f32,
                    widget.height() as f32 * self.progress.get() as f32,
                ));
                widget.snapshot_child(child, snapshot);
                snapshot.pop();
            }
        }
    }
}

glib::wrapper! {
    pub struct Reveal(ObjectSubclass<imp::Reveal>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Reveal {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let widget: Self = glib::Object::new();
        widget.set_hexpand(child.compute_expand(gtk::Orientation::Horizontal));
        widget.set_vexpand(child.compute_expand(gtk::Orientation::Vertical));
        child.set_parent(&widget);
        *widget.imp().child.borrow_mut() = Some(child.as_ref().clone());
        let weak = widget.downgrade();
        let target = adw::CallbackAnimationTarget::new(move |progress| {
            if let Some(widget) = weak.upgrade() {
                widget.imp().progress.set(progress);
                widget.queue_draw();
            }
        });
        let animation = adw::TimedAnimation::new(&widget, 0.0, 1.0, 160, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        *widget.imp().animation.borrow_mut() = Some(animation);
        widget
    }

    /// Clip the rendered content from top to bottom without changing its layout or opacity.
    pub fn play(&self) {
        if let Some(animation) = self.imp().animation.borrow().as_ref() {
            animation.reset();
            animation.play();
        }
    }
}
