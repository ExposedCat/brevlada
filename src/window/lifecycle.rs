use super::*;

struct CloseAttempt {
    pending: Rc<Cell<bool>>,
    window: glib::WeakRef<adw::ApplicationWindow>,
}

impl Drop for CloseAttempt {
    fn drop(&mut self) {
        self.pending.set(false);
        if let Some(window) = self.window.upgrade() {
            window.set_sensitive(true);
        }
    }
}

pub(super) fn flush_before_close(
    state: &State,
    window: &adw::ApplicationWindow,
    pending: &Rc<Cell<bool>>,
    ready: &Rc<Cell<bool>>,
) -> bool {
    let mut composers: Vec<_> = state
        .open_composers
        .borrow()
        .iter()
        .map(|open| open.compose.clone())
        .chain(
            state
                .draft_editors
                .borrow()
                .values()
                .map(|editor| editor.compose.clone()),
        )
        .filter(|compose| !compose.autosave.paused() && compose.autosave.needs_flush())
        .collect();
    composers.dedup_by(|left, right| Rc::ptr_eq(left, right));
    if composers.is_empty() {
        return false;
    }
    pending.set(true);
    window.set_sensitive(false);
    let attempt = CloseAttempt {
        pending: pending.clone(),
        window: window.downgrade(),
    };
    let ready = ready.clone();
    let window = window.downgrade();
    flush(
        composers,
        Box::new(move || {
            drop(attempt);
            ready.set(true);
            glib::idle_add_local_once(move || {
                if let Some(window) = window.upgrade() {
                    window.close();
                }
            });
        }),
    );
    true
}

fn flush(mut composers: Vec<Rc<ui::compose::Compose>>, done: Box<dyn FnOnce()>) {
    let Some(compose) = composers.pop() else {
        done();
        return;
    };
    let autosave = compose.autosave.clone();
    compose.autosave.defer(true, move || {
        autosave.resume();
        flush(composers, done);
    });
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "requires a graphical session"]
    fn reopened_windows_reject_previous_window_results() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.ExposedCat.Brevlada.LifecycleDiagnostics")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let (worker, _commands) = worker::Worker::recording();
        let worker = Rc::new(worker);
        let state = |account: &str| {
            let state = State::new(
                ui::shell::Shell::new(&app),
                worker.clone(),
                ui::expansion::Expansion::default(),
                ui::avatars::Avatars::new(|_| {}),
            );
            state.select(
                Account {
                    email: account.into(),
                    path: String::new(),
                    name: String::new(),
                    host: String::new(),
                    username: String::new(),
                    port: 993,
                    ssl: true,
                    tls: false,
                    oauth2: false,
                    smtp: None,
                },
                "INBOX".into(),
            );
            state
        };
        let previous = state("previous@example.com");
        let generation = previous.generation.get();
        let selection = previous.selection.get();
        let composer = previous.next_composer_request();
        app.windows()[0].destroy();
        drop(previous);
        let reopened = state("current@example.com");
        assert_ne!(reopened.selection.get(), selection);
        assert_ne!(reopened.next_composer_request(), composer);
        reopened.event(Event::Messages(
            generation,
            vec![Message {
                uid: 7,
                subject: "Previous window".into(),
                ..Message::default()
            }],
            false,
        ));
        assert!(reopened.messages.borrow().is_empty());
        assert!(reopened.loading.get());
        app.windows()[0].destroy();
    }
}
