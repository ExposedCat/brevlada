use super::*;
use std::path::PathBuf;

impl State {
    pub(super) fn connect_account_settings(
        self: &Rc<Self>,
        settings: &ui::settings::SettingsWindow,
        path: PathBuf,
    ) {
        let weak = Rc::downgrade(self);
        let ui = settings.accounts.clone();
        let load_path = path.clone();
        settings.accounts_section.connect_activated(move |_| {
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
            ui.set_accounts(accounts, current.as_deref());
            state.load_account_settings(&ui, load_path.clone());
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.accounts);
        let load_path = path.clone();
        settings.accounts.account.connect_selected_notify(move |_| {
            if let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade())
                && !ui.updating.get()
            {
                state.load_account_settings(&ui, load_path.clone());
            }
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.accounts);
        let load_path = path.clone();
        settings.accounts.reload.connect_clicked(move |_| {
            if let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade()) {
                state.load_account_settings(&ui, load_path.clone());
            }
        });
        let weak = Rc::downgrade(self);
        let ui = Rc::downgrade(&settings.accounts);
        settings.accounts.apply.connect_clicked(move |_| {
            let (Some(state), Some(ui)) = (weak.upgrade(), ui.upgrade()) else { return };
            let Some(account) = ui.selected_account() else { return };
            let folder = ui.archive_folder();
            let share = ui.share.is_sensitive() && ui.share.is_active();
            let (reply, result) = async_channel::bounded(1);
            let request = ui.busy(true);
            if let Err(error) = state.sender.send(Command::SetAccountArchive { account, path: path.clone(), folder, share, reply }) {
                ui.failed(&error.to_string(), true);
                return;
            }
            let ui = Rc::downgrade(&ui);
            glib::spawn_future_local(async move {
                let result = result.recv().await;
                if let Some(ui) = ui.upgrade() && ui.accepts(request) {
                    match result {
                        Ok(Ok(config)) => ui.loaded(config, Some(share)),
                        Ok(Err(error)) => ui.failed(&error, true),
                        Err(_) => ui.failed("The mail service stopped before saving account settings. Reload to check the saved state.", true),
                    }
                }
            });
        });
    }

    fn load_account_settings(&self, ui: &Rc<ui::account_settings::Accounts>, path: PathBuf) {
        let Some(account) = ui.selected_account() else {
            ui.empty();
            return;
        };
        let (reply, result) = async_channel::bounded(1);
        let request = ui.busy(false);
        if let Err(error) = self.sender.send(Command::AccountSettings {
            account,
            path,
            reply,
        }) {
            ui.failed(&error.to_string(), false);
            return;
        }
        let ui = Rc::downgrade(ui);
        glib::spawn_future_local(async move {
            let result = result.recv().await;
            if let Some(ui) = ui.upgrade()
                && ui.accepts(request)
            {
                match result {
                    Ok(Ok(config)) => ui.loaded(config, None),
                    Ok(Err(error)) => ui.failed(&error, false),
                    Err(_) => ui.failed(
                        "The mail service stopped while loading account settings.",
                        false,
                    ),
                }
            }
        });
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use models::settings::AccountFolders;

    fn wait(predicate: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !predicate() && std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(predicate());
    }

    fn config(shared: bool) -> AccountFolders {
        AccountFolders {
            folders: vec!["Archive".into(), "[Gmail]/All Mail".into()],
            archive_folder: None,
            server_archive: Some("[Gmail]/All Mail".into()),
            can_share: shared,
        }
    }

    #[test]
    #[ignore = "Requires a private graphical session; fixture accounts and recording worker"]
    fn accounts_settings_select_save_reload_and_explain_server_support() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaAccountSettingsDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let parent = shell.window.clone();
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        let first = Account {
            email: "alice@example.com".into(),
            path: String::new(),
            name: "Alice".into(),
            host: "fixture".into(),
            username: "alice".into(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let second = Account {
            email: "work@example.com".into(),
            ..first.clone()
        };
        state.event(Event::Accounts(vec![second.clone(), first.clone()]));
        *state.account.borrow_mut() = Some(first.clone());
        let settings = ui::settings::SettingsWindow::new(&parent, false);
        let directory = std::env::temp_dir().join(format!(
            "brevlada-account-ui-{}",
            glib::uuid_string_random()
        ));
        let path = directory.join("settings.json");
        state.connect_account_settings(&settings, path.clone());
        let ui = &settings.accounts;
        settings
            .accounts_section
            .emit_by_name::<()>("activated", &[]);
        let Command::AccountSettings { account, reply, .. } = commands.try_recv().unwrap() else {
            panic!("Expected account settings request");
        };
        assert_eq!(account.email, first.email);
        reply.send_blocking(Ok(config(false))).unwrap();
        wait(|| ui.archive.is_sensitive());
        assert_eq!(ui.archive.selected(), 0);
        assert!(!ui.share.is_sensitive());
        assert!(ui.share.subtitle().unwrap().contains("Brevlada only"));
        ui.archive.set_selected(1);
        assert!(ui.apply.is_sensitive());
        ui.apply.emit_clicked();
        let Command::SetAccountArchive {
            account,
            path: saved_path,
            folder,
            share,
            reply,
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected archive settings save");
        };
        assert_eq!(account.email, first.email);
        assert_eq!(saved_path, path);
        assert_eq!(folder.as_deref(), Some("Archive"));
        assert!(!share);
        let mut saved = config(false);
        saved.archive_folder = folder;
        reply.send_blocking(Ok(saved.clone())).unwrap();
        wait(|| ui.archive.is_sensitive());
        assert!(!ui.apply.is_sensitive());
        ui.account.set_selected(1);
        let Command::AccountSettings { account, reply, .. } = commands.try_recv().unwrap() else {
            panic!("Expected second account settings");
        };
        assert_eq!(account.email, second.email);
        reply.send_blocking(Ok(config(true))).unwrap();
        wait(|| ui.archive.is_sensitive());
        assert_eq!(ui.archive.selected(), 0);
        assert!(!ui.apply.is_sensitive());
        ui.archive.set_selected(1);
        assert!(ui.share.is_sensitive());
        ui.share.set_active(true);
        ui.apply.emit_clicked();
        let Command::SetAccountArchive {
            account,
            share,
            reply,
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected shared archive save");
        };
        assert_eq!(account.email, second.email);
        assert!(share);
        reply
            .send_blocking(Err("Server declined the archive folder change".into()))
            .unwrap();
        wait(|| ui.apply.is_sensitive());
        ui.reload.emit_clicked();
        let Command::AccountSettings { reply, .. } = commands.try_recv().unwrap() else {
            panic!("Expected reload");
        };
        reply.send_blocking(Ok(config(true))).unwrap();
        wait(|| ui.archive.is_sensitive());
        assert_eq!(ui.archive.selected(), 0);
        assert!(!ui.share.is_active());
        ui.account.set_selected(0);
        let Command::AccountSettings { reply, .. } = commands.try_recv().unwrap() else {
            panic!("Expected first account reload");
        };
        reply.send_blocking(Ok(saved)).unwrap();
        wait(|| ui.archive.is_sensitive());
        assert_eq!(ui.archive.selected(), 1);
        ui.archive.set_selected(0);
        assert!(ui.apply.is_sensitive());
        ui.apply.emit_clicked();
        let Command::SetAccountArchive {
            folder,
            share,
            reply,
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected automatic folder save");
        };
        assert!(folder.is_none() && !share);
        reply.send_blocking(Ok(config(false))).unwrap();
        wait(|| ui.archive.is_sensitive());
        settings.window.present();
        wait(|| settings.window.is_mapped());
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(path) = std::env::var_os("BREVLADA_ACCOUNT_SETTINGS_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(&settings.window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(
                &snapshot,
                settings.window.width() as f64,
                settings.window.height() as f64,
            );
            let texture = settings
                .window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None);
            texture.save_to_png(path).unwrap();
        }
        let old = ui.busy(false);
        let new = ui.busy(false);
        assert!(!ui.accepts(old) && ui.accepts(new));
        settings.window.destroy();
        parent.destroy();
    }
}
