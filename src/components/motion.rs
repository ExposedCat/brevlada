use adw::prelude::*;

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

type VisibilityHandler = Box<dyn Fn(&Sidebar)>;

struct SidebarState {
    pane: gtk::Paned,
    clip: gtk::ScrolledWindow,
    content: gtk::Box,
    visible: Cell<bool>,
    width: Cell<i32>,
    animating: Cell<bool>,
    animation: RefCell<Option<adw::TimedAnimation>>,
    handlers: RefCell<Vec<VisibilityHandler>>,
}

/// Logical visibility changes immediately; the clipped pane slides to its saved width.
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarState>);

pub struct WeakSidebar(Weak<SidebarState>);

impl WeakSidebar {
    pub fn upgrade(&self) -> Option<Sidebar> {
        self.0.upgrade().map(Sidebar)
    }
}

impl Sidebar {
    pub fn downgrade(&self) -> WeakSidebar {
        WeakSidebar(Rc::downgrade(&self.0))
    }
    pub fn new(content: &gtk::Box, end: &impl IsA<gtk::Widget>, width: i32, visible: bool) -> Self {
        let clip = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(content)
            .visible(visible)
            .build();
        let pane = super::pane(&clip, end, width);
        Self(Rc::new(SidebarState {
            pane,
            clip,
            content: content.clone(),
            visible: Cell::new(visible),
            width: Cell::new(width),
            animating: Cell::new(false),
            animation: RefCell::new(None),
            handlers: RefCell::new(Vec::new()),
        }))
    }

    pub fn pane(&self) -> &gtk::Paned {
        &self.0.pane
    }
    pub fn get_visible(&self) -> bool {
        self.0.visible.get()
    }

    pub fn width(&self) -> i32 {
        if self.get_visible() && !self.0.animating.get() {
            self.0.pane.position().max(1)
        } else {
            self.0.width.get()
        }
    }

    pub fn restore_width(&self, width: i32) {
        self.0.width.set(width.max(1));
        self.0.pane.set_position(width.max(1));
    }

    pub fn connect_visible_notify(&self, handler: impl Fn(&Self) + 'static) {
        self.0.handlers.borrow_mut().push(Box::new(handler));
    }

    pub fn set_visible(&self, visible: bool) {
        if visible == self.get_visible() {
            return;
        }
        let width = self.width();
        self.0.width.set(width);
        let from = if self.0.clip.get_visible() {
            self.0.pane.position()
        } else {
            0
        };
        if let Some(previous) = self.0.animation.borrow_mut().take() {
            previous.pause();
        }
        self.0.visible.set(visible);
        self.0.animating.set(true);
        // Hold the content at full width while the viewport clips it, avoiding text reflow.
        self.0.content.set_width_request(width);
        self.0.clip.set_hscrollbar_policy(gtk::PolicyType::External);
        self.0.pane.set_shrink_start_child(true);
        self.0.clip.set_visible(true);
        self.0.pane.set_position(from);
        let weak = Rc::downgrade(&self.0);
        let target = adw::CallbackAnimationTarget::new(move |value| {
            if let Some(state) = weak.upgrade() {
                state.pane.set_position(value.round() as i32);
            }
        });
        let animation = adw::TimedAnimation::new(
            &self.0.pane,
            f64::from(from),
            if visible { f64::from(width) } else { 0.0 },
            200,
            target,
        );
        animation.set_easing(adw::Easing::EaseOutCubic);
        let weak = Rc::downgrade(&self.0);
        animation.connect_done(move |_| {
            if let Some(state) = weak.upgrade() {
                state.clip.set_visible(state.visible.get());
                state.content.set_width_request(-1);
                state.clip.set_hscrollbar_policy(gtk::PolicyType::Never);
                state.pane.set_shrink_start_child(false);
                state.pane.set_position(state.width.get());
                state.animating.set(false);
            }
        });
        *self.0.animation.borrow_mut() = Some(animation.clone());
        animation.play();
        for handler in self.0.handlers.borrow().iter() {
            handler(self);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::time::{Duration, Instant};

    fn advance(ms: u64) {
        let context = gtk::glib::MainContext::default();
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated animated layout"]
    fn sidebar_slide_preserves_widths_when_interrupted() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let settings = gtk::Settings::default().unwrap();
        let enabled = settings.is_gtk_enable_animations();
        settings.set_gtk_enable_animations(true);
        let accounts = super::super::column("diagnostic");
        let senders = super::super::column("diagnostic");
        let messages = super::super::column("diagnostic");
        let viewer = super::super::column("diagnostic");
        let threads = Sidebar::new(&messages, &viewer, 350, true);
        let content = super::super::pane(&senders, threads.pane(), 300);
        let sidebar = Sidebar::new(&accounts, &content, 250, true);
        let window = gtk::Window::builder()
            .default_width(1400)
            .default_height(700)
            .child(sidebar.pane())
            .build();
        window.present();
        advance(300);
        let initial_viewer = viewer.width();
        for _ in 0..3 {
            sidebar.set_visible(false);
            advance(60);
            assert!(
                sidebar.pane().position() > 0 && sidebar.pane().position() < 250,
                "position={} mapped={} animating={} clip={} content={} animation={:?}",
                sidebar.pane().position(),
                sidebar.pane().is_mapped(),
                sidebar.0.animating.get(),
                sidebar.0.clip.width(),
                accounts.width(),
                sidebar
                    .0
                    .animation
                    .borrow()
                    .as_ref()
                    .map(|a| (a.state(), a.value()))
            );
            assert_eq!(sidebar.width(), 250);
            assert_eq!(senders.width(), 300);
            assert_eq!(messages.width(), 350);
            sidebar.set_visible(true);
            advance(260);
            assert_eq!(accounts.width(), 250);
            assert_eq!(viewer.width(), initial_viewer);
            sidebar.set_visible(false);
            advance(260);
            assert!(!sidebar.0.clip.get_visible());
            assert!(viewer.width() >= initial_viewer + 250);
            sidebar.set_visible(true);
            advance(260);
            threads.set_visible(false);
            advance(60);
            assert_eq!(threads.width(), 350);
            assert_eq!(senders.width(), 300);
            threads.set_visible(true);
            advance(260);
            assert_eq!(messages.width(), 350);
        }
        settings.set_gtk_enable_animations(false);
        sidebar.set_visible(false);
        assert!(!sidebar.0.animating.get());
        sidebar.set_visible(true);
        assert!(!sidebar.0.animating.get());
        assert_eq!(sidebar.width(), 250);
        settings.set_gtk_enable_animations(enabled);
        window.destroy();
    }
}
