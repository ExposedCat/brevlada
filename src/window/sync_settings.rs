use super::*;

impl State {
    pub(super) fn connect_sync_settings(self: &Rc<Self>, settings: &ui::settings::SettingsWindow) {
        let weak = Rc::downgrade(self);
        let ui = settings.sync.clone();
        let window = settings.toasts.downgrade();
        settings.sync_section.connect_activated(move |_| {
            let Some(state) = weak.upgrade() else { return };
            let mut accounts: Vec<_> = state
                .folder_boxes
                .borrow()
                .values()
                .map(|(account, _)| account.clone())
                .collect();
            accounts.sort_by(|left, right| left.email.cmp(&right.email));
            let current = state
                .account
                .borrow()
                .as_ref()
                .map(|account| account.email.clone());
            ui.accounts.set_accounts(accounts, current.as_deref());
            state.load_sync_settings(&ui, window.clone());
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.sync);
        let window = settings.toasts.downgrade();
        settings.sync.accounts.connect_changed(move || {
            if let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade()) {
                state.load_sync_settings(&ui, window.clone());
            }
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.sync);
        let window = settings.toasts.downgrade();
        settings.sync.reload.connect_clicked(move |_| {
            if let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade()) {
                state.load_sync_settings(&ui, window.clone());
            }
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.sync);
        let window = settings.toasts.downgrade();
        settings
            .sync
            .connect_folder_changed(move |folder, enabled| {
                if let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade()) {
                    state.save_sync_folder(&ui, window.clone(), folder, enabled);
                }
            });
    }

    fn load_sync_settings(
        &self,
        ui: &Rc<ui::sync_settings::SyncSettings>,
        window: glib::WeakRef<adw::ToastOverlay>,
    ) {
        let accounts = ui.accounts.selected_accounts();
        if accounts.is_empty() {
            ui.empty();
            return;
        }
        let request = ui.busy(true);
        let mut pending = Vec::new();
        for account in accounts {
            let (reply, result) = async_channel::bounded(1);
            let email = account.email.clone();
            if let Err(error) = self.sender.send(Command::SyncFolders { account, reply }) {
                ui.failed();
                if let Some(window) = window.upgrade() {
                    window.add_toast(adw::Toast::new(&error.to_string()));
                }
                return;
            }
            pending.push((email, result));
        }
        let ui = Rc::downgrade(ui);
        glib::spawn_future_local(async move {
            let mut configs = HashMap::new();
            let mut errors = Vec::new();
            for (email, result) in pending {
                match result.recv().await {
                    Ok(Ok(config)) => {
                        configs.insert(email, config);
                    }
                    Ok(Err(error)) => errors.push(format!("{email}: {error}")),
                    Err(_) => errors.push(format!(
                        "{email}: The mail service stopped while loading folders."
                    )),
                }
            }
            if let Some(ui) = ui.upgrade()
                && ui.accepts(request)
            {
                if errors.is_empty() {
                    ui.loaded(configs);
                } else {
                    ui.failed();
                    if let Some(window) = window.upgrade() {
                        window.add_toast(adw::Toast::new(&errors.join("\n")));
                    }
                }
            }
        });
    }

    fn save_sync_folder(
        &self,
        ui: &Rc<ui::sync_settings::SyncSettings>,
        window: glib::WeakRef<adw::ToastOverlay>,
        folder: &str,
        enabled: bool,
    ) {
        let changes = ui.changes(folder, enabled);
        let request = ui.busy(false);
        let mut pending = Vec::new();
        let mut errors = Vec::new();
        for (account, folders) in changes {
            let (reply, result) = async_channel::bounded(1);
            let email = account.email.clone();
            if let Err(error) = self.sender.send(Command::SetSyncFolders {
                account,
                folders,
                reply,
            }) {
                errors.push(format!("{email}: {error}"));
            } else {
                pending.push((email, result));
            }
        }
        let ui = Rc::downgrade(ui);
        glib::spawn_future_local(async move {
            let mut saved = Vec::new();
            for (email, result) in pending {
                match result.recv().await {
                    Ok(Ok(folders)) => saved.push((email, folders)),
                    Ok(Err(error)) => errors.push(format!("{email}: {error}")),
                    Err(_) => errors.push(format!(
                        "{email}: The mail service stopped before saving folders."
                    )),
                }
            }
            if let Some(ui) = ui.upgrade()
                && ui.accepts(request)
            {
                ui.saved(saved);
            }
            if !errors.is_empty()
                && let Some(window) = window.upgrade()
            {
                window.add_toast(adw::Toast::new(&errors.join("\n")));
            }
        });
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::sync::Folders;
    use std::sync::mpsc::Receiver;

    fn wait(predicate: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !predicate() && std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(predicate());
    }

    fn check(widget: &gtk::Widget, label: Option<&str>) -> Option<gtk::CheckButton> {
        if let Some(check) = widget.downcast_ref::<gtk::CheckButton>()
            && label.is_none_or(|label| check.label().as_deref() == Some(label))
        {
            return Some(check.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(check) = check(&widget, label) {
                return Some(check);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn row(widget: &gtk::Widget, title: &str) -> Option<adw::ActionRow> {
        if let Some(row) = widget.downcast_ref::<adw::ActionRow>()
            && row.title() == title
        {
            return Some(row.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(row) = row(&widget, title) {
                return Some(row);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn toggle(ui: &ui::sync_settings::SyncSettings, name: &str) -> gtk::CheckButton {
        check(row(ui.page.upcast_ref(), name).unwrap().upcast_ref(), None).unwrap()
    }

    fn config(work: bool, selected: &[&str]) -> Folders {
        Folders {
            folders: [
                "INBOX",
                "Archive",
                "Sent",
                "Drafts",
                if work { "Work" } else { "Personal" },
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            selected: selected.iter().map(|folder| (*folder).into()).collect(),
        }
    }

    fn respond(commands: &Receiver<Command>, first: &[&str], second: &[&str]) {
        for _ in 0..2 {
            let Command::SyncFolders { account, reply } = commands.try_recv().unwrap() else {
                panic!("Expected folder load")
            };
            let work = account.email == "work@example.com";
            reply
                .send_blocking(Ok(config(work, if work { second } else { first })))
                .unwrap();
        }
    }

    fn layout() {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    #[ignore = "Requires a private graphical session; fixture accounts and recording worker"]
    fn common_folders_autosave_without_overwriting_other_folders_and_fill_the_window() {
        let (worker, commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaCommonSyncDiagnostic",
            worker,
        );
        let parent = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        let first = state.account.borrow().clone().unwrap();
        state.event(Event::Accounts(vec![
            first.clone(),
            Account {
                email: "work@example.com".into(),
                ..first.clone()
            },
        ]));
        let settings = ui::settings::SettingsWindow::new(&parent, false);
        state.connect_sync_settings(&settings);
        let ui = &settings.sync;
        while commands.try_recv().is_ok() {}
        settings.sync_section.emit_by_name::<()>("activated", &[]);
        let Command::SyncFolders { account, reply } = commands.try_recv().unwrap() else {
            panic!("Expected initial folder load")
        };
        assert_eq!(account.email, first.email);
        reply
            .send_blocking(Ok(config(false, &["INBOX", "Personal"])))
            .unwrap();
        wait(|| !ui.updating.get());
        assert!(toggle(ui, "INBOX").is_active());
        toggle(ui, "Archive").set_active(true);
        let Command::SetSyncFolders {
            account,
            folders,
            reply,
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected automatic save")
        };
        assert_eq!(account.email, first.email);
        assert_eq!(folders, ["Archive", "INBOX", "Personal"]);
        reply.send_blocking(Ok(folders)).unwrap();
        wait(|| !ui.updating.get());
        check(ui.accounts.row.upcast_ref(), Some("work@example.com"))
            .unwrap()
            .set_active(true);
        respond(
            &commands,
            &["Archive", "INBOX", "Personal"],
            &["Archive", "Work"],
        );
        wait(|| !ui.updating.get());
        assert_eq!(ui.accounts.selected_accounts().len(), 2);
        assert!(row(ui.page.upcast_ref(), "Personal").is_none());
        assert!(row(ui.page.upcast_ref(), "Work").is_none());
        assert!(toggle(ui, "INBOX").is_inconsistent());
        assert!(toggle(ui, "Archive").is_active());
        settings.window.set_default_size(1200, 850);
        settings.window.present();
        wait(|| settings.window.is_mapped());
        layout();
        assert!(row(ui.page.upcast_ref(), "INBOX").unwrap().width() > 800);
        if let Some(path) = std::env::var_os("BREVLADA_SYNC_SETTINGS_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(&settings.window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(
                &snapshot,
                settings.window.width() as f64,
                settings.window.height() as f64,
            );
            settings
                .window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(path)
                .unwrap();
        }
        settings.window.set_default_size(560, 600);
        layout();
        assert!(row(ui.page.upcast_ref(), "INBOX").unwrap().width() < 560);
        toggle(ui, "INBOX").set_active(true);
        let Command::SetSyncFolders {
            account,
            folders,
            reply,
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected shared-folder save")
        };
        assert_eq!(account.email, "work@example.com");
        assert_eq!(folders, ["Archive", "INBOX", "Work"]);
        assert!(commands.try_recv().is_err());
        reply.send_blocking(Ok(folders)).unwrap();
        wait(|| !ui.updating.get());
        assert!(!toggle(ui, "INBOX").is_inconsistent());
        toggle(ui, "Archive").set_active(false);
        for _ in 0..2 {
            let Command::SetSyncFolders {
                account,
                folders,
                reply,
            } = commands.try_recv().unwrap()
            else {
                panic!("Expected multi-account save")
            };
            if account.email == first.email {
                assert_eq!(folders, ["INBOX", "Personal"]);
                reply.send_blocking(Ok(folders)).unwrap();
            } else {
                assert_eq!(folders, ["INBOX", "Work"]);
                reply.send_blocking(Err("Disk error".into())).unwrap();
            }
        }
        wait(|| !ui.updating.get());
        assert!(toggle(ui, "Archive").is_inconsistent());
        ui.reload.emit_clicked();
        let old: Vec<_> = (0..2).map(|_| commands.try_recv().unwrap()).collect();
        check(ui.accounts.row.upcast_ref(), Some(first.email.as_str()))
            .unwrap()
            .set_active(false);
        let Command::SyncFolders { reply, .. } = commands.try_recv().unwrap() else {
            panic!("Expected changed account selection")
        };
        reply
            .send_blocking(Ok(config(true, &["Archive", "INBOX", "Work"])))
            .unwrap();
        wait(|| !ui.updating.get());
        for command in old {
            let Command::SyncFolders { reply, .. } = command else {
                unreachable!()
            };
            reply.send_blocking(Ok(config(false, &[]))).unwrap();
        }
        layout();
        assert!(toggle(ui, "Work").is_active());
        check(ui.accounts.row.upcast_ref(), Some("work@example.com"))
            .unwrap()
            .set_active(false);
        assert!(row(ui.page.upcast_ref(), "INBOX").is_none());
        assert!(commands.try_recv().is_err());
        check(ui.accounts.row.upcast_ref(), Some("All accounts"))
            .unwrap()
            .set_active(true);
        for name in ["Only personal", "Only work"] {
            let Command::SyncFolders { reply, .. } = commands.try_recv().unwrap() else {
                panic!("Expected folders for selected accounts");
            };
            reply
                .send_blocking(Ok(Folders {
                    folders: vec![name.into()],
                    selected: Vec::new(),
                }))
                .unwrap();
        }
        wait(|| !ui.updating.get());
        assert!(row(ui.page.upcast_ref(), "Only personal").is_none());
        assert!(row(ui.page.upcast_ref(), "Only work").is_none());
        ui.reload.emit_clicked();
        for _ in 0..2 {
            let Command::SyncFolders { reply, .. } = commands.try_recv().unwrap() else {
                panic!("Expected reload");
            };
            reply.send_blocking(Err("Offline".into())).unwrap();
        }
        wait(|| !ui.updating.get());
        assert!(ui.reload.is_sensitive());
        assert!(row(ui.page.upcast_ref(), "INBOX").is_none());
        ui.reload.emit_clicked();
        respond(
            &commands,
            &["INBOX", "Personal"],
            &["Archive", "INBOX", "Work"],
        );
        wait(|| !ui.updating.get());
        settings.window.destroy();
        parent.destroy();
    }
}
