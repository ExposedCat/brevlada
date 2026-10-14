mod autostart;
mod configuration;
mod options;
mod portal;
mod startup;
pub use startup::install as install_startup;

use crate::application;
use adw::prelude::*;
use gtk::{gio, glib};
use options::Options;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct Background {
    action: gio::SimpleAction,
    autostart: gio::SimpleAction,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    login_session: Cell<bool>,
    app: glib::WeakRef<adw::Application>,
    engine: std::rc::Weak<application::Engine>,
}

impl Background {
    pub fn new(app: &adw::Application, engine: &Rc<application::Engine>) -> Rc<Self> {
        let action = gio::SimpleAction::new_stateful("background", None, &false.to_variant());
        let autostart = gio::SimpleAction::new_stateful("autostart", None, &false.to_variant());
        app.add_action(&action);
        app.add_action(&autostart);
        let background = Rc::new(Self {
            action,
            autostart,
            hold: RefCell::default(),
            login_session: Cell::new(false),
            app: app.downgrade(),
            engine: Rc::downgrade(engine),
        });
        for (action, startup) in [(&background.action, false), (&background.autostart, true)] {
            let weak = Rc::downgrade(&background);
            action.connect_change_state(move |_, value| {
                if let Some(background) = weak.upgrade()
                    && let Some(enabled) = value.and_then(|value| value.get::<bool>())
                {
                    let mut options = background.options();
                    if startup {
                        options.start_at_login = enabled;
                    } else {
                        options.keep_checking = enabled;
                    }
                    background.change(options);
                }
            });
        }
        match application::settings() {
            Ok(settings) if settings.background_service || settings.start_at_login => {
                background.change(Options {
                    keep_checking: settings.background_service,
                    start_at_login: settings.start_at_login,
                });
            }
            Err(error) => eprintln!("Could not restore background settings: {error}"),
            _ => {}
        }
        background
    }

    fn options(&self) -> Options {
        Options {
            keep_checking: self
                .action
                .state()
                .and_then(|state| state.get())
                .unwrap_or(false),
            start_at_login: self
                .autostart
                .state()
                .and_then(|state| state.get())
                .unwrap_or(false),
        }
    }

    fn set_options(&self, options: Options) {
        self.action.set_state(&options.keep_checking.to_variant());
        self.autostart
            .set_state(&options.start_at_login.to_variant());
    }

    fn update_hold(&self) {
        if self.options().stays_alive(self.login_session.get()) {
            if self.hold.borrow().is_none()
                && let Some(app) = self.app.upgrade()
            {
                *self.hold.borrow_mut() = Some(app.hold());
            }
        } else {
            self.hold.borrow_mut().take();
        }
    }

    pub fn start_headless(&self) {
        if self
            .app
            .upgrade()
            .is_some_and(|app| app.windows().is_empty())
        {
            self.login_session.set(true);
            if self.action.is_enabled() {
                self.update_hold();
            }
        }
    }

    pub fn window_opened(&self) {
        self.login_session.set(false);
        if self.action.is_enabled() {
            self.update_hold();
        }
    }

    pub fn stop(&self) {
        self.hold.borrow_mut().take();
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "requires a graphical session; no mail workers or autostart changes"]
    fn opening_window_ends_login_only_hold_and_closing_preference_is_independent() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("com.chipichapa.Brevlada.BackgroundLifetimeDiagnostics")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let background = Background {
            action: gio::SimpleAction::new_stateful("background", None, &false.to_variant()),
            autostart: gio::SimpleAction::new_stateful("autostart", None, &true.to_variant()),
            hold: RefCell::default(),
            login_session: Cell::new(false),
            app: app.downgrade(),
            engine: std::rc::Weak::new(),
        };
        background.action.set_enabled(false);
        background.start_headless();
        assert!(background.hold.borrow().is_none());
        background.action.set_enabled(true);
        background.update_hold();
        assert!(background.hold.borrow().is_some());
        assert!(app.windows().is_empty());
        let window = adw::ApplicationWindow::new(&app);
        background.window_opened();
        assert!(background.hold.borrow().is_none());
        background.action.set_state(&true.to_variant());
        background.update_hold();
        assert!(background.hold.borrow().is_some());
        window.destroy();
        assert!(background.hold.borrow().is_some());
        background.autostart.set_state(&false.to_variant());
        background.update_hold();
        assert!(background.hold.borrow().is_some());
        background.action.set_state(&false.to_variant());
        background.update_hold();
        assert!(background.hold.borrow().is_none());
        background.stop();
    }
}
