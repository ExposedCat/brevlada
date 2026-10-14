use super::*;

impl Background {
    pub(super) fn change(self: &Rc<Self>, options: Options) {
        if !self.action.is_enabled() {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let previous = self.options();
        self.action.set_enabled(false);
        self.autostart.set_enabled(false);
        self.set_options(options);
        let background = self.clone();
        let pending_hold = app.hold();
        glib::spawn_future_local(async move {
            let sandboxed = std::path::Path::new("/.flatpak-info").exists();
            let configure =
                options.needs_background() || options.start_at_login != previous.start_at_login;
            let result = if sandboxed && configure {
                portal::request(options).await
            } else if !sandboxed {
                autostart::set(options.start_at_login)
            } else {
                Ok(())
            };
            let configured = result.is_ok();
            let result = result.and_then(|()| {
                application::save_background(options.keep_checking, options.start_at_login)
            });
            if let Err(error) = result {
                background.set_options(previous);
                if sandboxed
                    && configure
                    && (configured || options.start_at_login != previous.start_at_login)
                {
                    if let Err(error) = portal::request(previous).await {
                        eprintln!("Could not restore login startup: {error}");
                    }
                } else if !sandboxed && let Err(error) = autostart::set(previous.start_at_login) {
                    eprintln!("Could not restore login startup: {error}");
                }
                if let Err(error) =
                    application::save_background(previous.keep_checking, previous.start_at_login)
                {
                    eprintln!("Could not restore background settings: {error}");
                }
                eprintln!("Could not change background settings: {error}");
                let notification = gio::Notification::new("Could not change background settings");
                notification.set_body(Some(&error.to_string()));
                app.send_notification(Some("background-error"), &notification);
            }
            background.update_hold();
            if sandboxed
                && background.options().needs_background()
                && let Err(error) = portal::status().await
            {
                eprintln!("Could not report background status: {error}");
            }
            background.action.set_enabled(true);
            background.autostart.set_enabled(true);
            if let Some(engine) = background.engine.upgrade() {
                engine.flush_notifications();
            }
            drop(pending_hold);
        });
    }
}
