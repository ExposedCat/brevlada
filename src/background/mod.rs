mod portal;

use crate::application;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{cell::RefCell, rc::Rc};

pub struct Background {
    action: gio::SimpleAction,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    app: glib::WeakRef<adw::Application>,
    engine: std::rc::Weak<application::Engine>,
}

impl Background {
    pub fn new(app: &adw::Application, engine: &Rc<application::Engine>) -> Rc<Self> {
        let action = gio::SimpleAction::new_stateful("background", None, &false.to_variant());
        app.add_action(&action);
        let background = Rc::new(Self {
            action,
            hold: RefCell::default(),
            app: app.downgrade(),
            engine: Rc::downgrade(engine),
        });
        let weak = Rc::downgrade(&background);
        background.action.connect_change_state(move |_, value| {
            if let Some(background) = weak.upgrade()
                && let Some(enabled) = value.and_then(|value| value.get::<bool>())
            {
                background.change(enabled);
            }
        });
        if application::settings().is_ok_and(|settings| settings.background_service) {
            background.change(true);
        }
        background
    }

    fn change(self: &Rc<Self>, enabled: bool) {
        if !self.action.is_enabled() {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        self.action.set_enabled(false);
        let previous = self
            .action
            .state()
            .and_then(|state| state.get::<bool>())
            .unwrap_or(false);
        self.action.set_state(&enabled.to_variant());
        let background = self.clone();
        let pending_hold = app.hold();
        glib::spawn_future_local(async move {
            let sandboxed = std::path::Path::new("/.flatpak-info").exists();
            let result = if sandboxed && enabled {
                portal::request().await
            } else {
                Ok(())
            };
            let result = result.and_then(|()| application::save_background(enabled));
            match result {
                Ok(()) => {
                    background.action.set_state(&enabled.to_variant());
                    if enabled {
                        *background.hold.borrow_mut() = Some(app.hold());
                        if sandboxed && let Err(error) = portal::status().await {
                            eprintln!("Could not report background status: {error}");
                        }
                    } else {
                        background.hold.borrow_mut().take();
                    }
                }
                Err(error) => {
                    background.action.set_state(&previous.to_variant());
                    if let Err(error) = application::save_background(previous) {
                        eprintln!("Could not restore background setting: {error}");
                    }
                    eprintln!("Could not change background service: {error}");
                    let notification =
                        gio::Notification::new("Could not change background service");
                    notification.set_body(Some(&error.to_string()));
                    app.send_notification(Some("background-error"), &notification);
                }
            }
            background.action.set_enabled(true);
            if let Some(engine) = background.engine.upgrade() {
                engine.flush_notifications();
            }
            drop(pending_hold);
        });
    }

    pub fn stop(&self) {
        self.hold.borrow_mut().take();
    }
}
