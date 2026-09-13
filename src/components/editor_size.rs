use crate::theme;
use adw::prelude::*;
use std::{cell::Cell, rc::Rc};

pub struct Size {
    pub stack: gtk::Stack,
    height: Cell<i32>,
    dragging: Cell<bool>,
    measured: [Cell<Option<i32>>; 2],
}

impl Size {
    pub fn new() -> Rc<Self> {
        let size = Rc::new(Self {
            stack: gtk::Stack::builder()
                .vhomogeneous(false)
                .vexpand(false)
                .height_request(theme::COMPOSE_HEIGHT)
                .build(),
            height: Cell::new(theme::COMPOSE_HEIGHT),
            dragging: Cell::new(false),
            measured: [Cell::new(None), Cell::new(None)],
        });
        let weak = Rc::downgrade(&size);
        size.stack.add_tick_callback(move |_, _| {
            if let Some(size) = weak.upgrade() {
                size.set(size.height.get());
                gtk::glib::ControlFlow::Continue
            } else {
                gtk::glib::ControlFlow::Break
            }
        });
        size
    }

    pub fn measure(&self, mode: usize, content_height: i32, edited: bool) {
        if let Some(previous) = self.measured[mode].replace(Some(content_height))
            && edited
            && !self.dragging.get()
        {
            self.set(automatic_height(
                self.height.get(),
                previous,
                content_height,
            ));
        }
    }

    fn maximum(&self) -> i32 {
        self.stack
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
            .map(|scroll| scroll.height())
            .filter(|height| *height > 0)
            .unwrap_or(theme::WINDOW_HEIGHT)
            .max(theme::COMPOSE_HEIGHT)
    }

    fn set(&self, height: i32) {
        let height = height.clamp(theme::COMPOSE_HEIGHT, self.maximum());
        if self.height.replace(height) != height {
            self.stack.set_height_request(height);
        }
    }

    pub fn track_text(self: &Rc<Self>, body: &gtk::TextView) {
        let edited = Rc::new(Cell::new(false));
        let pending = edited.clone();
        body.buffer()
            .connect_end_user_action(move |_| pending.set(true));
        let weak = Rc::downgrade(self);
        body.add_tick_callback(move |body, _| {
            if let Some(size) = weak.upgrade() {
                if body.is_mapped() {
                    let end = body.iter_location(&body.buffer().end_iter());
                    size.measure(
                        0,
                        end.y() + end.height() + body.top_margin() + body.bottom_margin(),
                        edited.replace(false),
                    );
                }
                gtk::glib::ControlFlow::Continue
            } else {
                gtk::glib::ControlFlow::Break
            }
        });
    }

    pub fn handle(self: &Rc<Self>) -> gtk::Box {
        let handle = super::horizontal("compose-resize-handle", 0);
        handle.set_focusable(true);
        handle.set_cursor_from_name(Some("ns-resize"));
        handle.set_tooltip_text(Some(
            "Drag to resize; use Up/Down when focused. New lines grow the field. Double-click to reset height.",
        ));
        let grip = super::horizontal("compose-resize-grip", 0);
        grip.set_halign(gtk::Align::Center);
        grip.set_valign(gtk::Align::Center);
        grip.set_hexpand(true);
        handle.append(&grip);
        let drag = gtk::GestureDrag::new();
        let origin = Rc::new(Cell::new((theme::COMPOSE_HEIGHT, 0.0)));
        let size = self.clone();
        let start = origin.clone();
        drag.connect_drag_begin(move |gesture, _, _| {
            if let Some((_, y)) = gesture.current_event().and_then(|event| event.position()) {
                start.set((size.height.get(), y));
                size.dragging.set(true);
            }
        });
        let size = self.clone();
        drag.connect_drag_update(move |gesture, _, _| {
            if size.dragging.get()
                && let Some((_, y)) = gesture.current_event().and_then(|event| event.position())
            {
                let (height, start_y) = origin.get();
                size.set(height + (y - start_y).round() as i32);
            }
        });
        let size = self.clone();
        drag.connect_drag_end(move |_, _, _| size.dragging.set(false));
        let size = self.clone();
        drag.connect_cancel(move |_, _| size.dragging.set(false));
        handle.add_controller(drag);
        let click = gtk::GestureClick::new();
        let size = self.clone();
        click.connect_pressed(move |_, count, _, _| {
            if count == 2 {
                size.set(theme::COMPOSE_HEIGHT);
            }
        });
        handle.add_controller(click);
        let keys = gtk::EventControllerKey::new();
        let size = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let delta = match key {
                gtk::gdk::Key::Up => -theme::COMPOSE_RESIZE_STEP,
                gtk::gdk::Key::Down => theme::COMPOSE_RESIZE_STEP,
                _ => return gtk::glib::Propagation::Proceed,
            };
            size.set(size.height.get() + delta);
            gtk::glib::Propagation::Stop
        });
        handle.add_controller(keys);
        handle
    }
}

fn automatic_height(current: i32, previous: i32, content: i32) -> i32 {
    let target = (content + theme::COMPOSE_HEADROOM).max(theme::COMPOSE_HEIGHT);
    if content > previous && content + theme::COMPOSE_RESIZE_STEP > current
        || content < previous && current - target >= theme::COMPOSE_SHRINK_SLACK
    {
        target
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_sizing_keeps_headroom_without_chasing_every_line() {
        assert_eq!(automatic_height(200, 40, 64), 200);
        assert_eq!(automatic_height(200, 160, 184), 232);
        assert_eq!(automatic_height(232, 184, 208), 232);
        assert_eq!(automatic_height(500, 200, 224), 500);
        assert_eq!(automatic_height(500, 224, 200), 248);
        assert_eq!(automatic_height(248, 200, 176), 248);
        assert_eq!(automatic_height(400, 176, 40), 200);
    }

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated editor without mail workers"]
    fn grows_text_and_resizes_both_modes() {
        gtk::init().unwrap();
        let size = Size::new();
        let text = gtk::TextView::new();
        size.track_text(&text);
        let scroll = gtk::ScrolledWindow::builder().child(&text).build();
        size.stack.add_named(&scroll, Some("text"));
        size.stack
            .add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("html"));
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.append(&size.stack);
        let handle = size.handle();
        container.append(&handle);
        let viewport = gtk::ScrolledWindow::builder().child(&container).build();
        let window = gtk::Window::builder()
            .child(&viewport)
            .default_height(800)
            .default_width(400)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        let settle = || {
            context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
                100,
            )))
        };
        settle();
        assert_eq!(size.height.get(), theme::COMPOSE_HEIGHT);
        text.buffer().begin_user_action();
        text.buffer().insert_at_cursor(&"A line\n".repeat(100));
        text.buffer().end_user_action();
        settle();
        assert_eq!(size.height.get(), size.maximum());
        size.stack.set_visible_child_name("html");
        settle();
        assert_eq!(size.stack.height_request(), size.maximum());
        let controllers = handle.observe_controllers();
        let keys = (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)?
                    .downcast::<gtk::EventControllerKey>()
                    .ok()
            })
            .next()
            .unwrap();
        for _ in 0..8 {
            assert!(keys.emit_by_name::<bool>(
                "key-pressed",
                &[&gtk::gdk::Key::Up, &0u32, &gtk::gdk::ModifierType::empty()]
            ));
        }
        let resized = size.maximum() - 8 * theme::COMPOSE_RESIZE_STEP;
        assert_eq!(size.height.get(), resized);
        size.stack.set_visible_child_name("text");
        settle();
        assert_eq!(size.stack.height_request(), resized);
        text.buffer().begin_user_action();
        text.buffer().insert_at_cursor("One more line\n");
        text.buffer().end_user_action();
        settle();
        assert!(size.height.get() > resized);
        let height = size.height.get();
        size.measure(1, 40, false);
        size.measure(1, 64, true);
        assert_eq!(size.height.get(), height);
        size.dragging.set(true);
        size.measure(1, 88, true);
        assert_eq!(size.height.get(), height);
        size.dragging.set(false);
        size.measure(1, 20, true);
        assert_eq!(size.height.get(), theme::COMPOSE_HEIGHT);
        window.destroy();
    }
}
