use crate::{
    backend::worker::{self, Command, Event},
    models::settings::Settings,
    theme,
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::{cell::RefCell, collections::HashSet, rc::Rc, time::Duration};

type Listener = Box<dyn Fn(Event)>;

pub struct Engine {
    pub worker: Rc<worker::Worker>,
    listener: RefCell<Option<Listener>>,
    timer: RefCell<Option<glib::SourceId>>,
    notified: RefCell<HashSet<(String, String, u32, u32)>>,
    accounts: RefCell<HashSet<String>>,
    app: glib::WeakRef<adw::Application>,
}

impl Engine {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let (worker, events) = worker::start(glib::user_data_dir().join("brevlada/emails.db"));
        let engine = Rc::new(Self {
            worker: Rc::new(worker),
            listener: RefCell::default(),
            timer: RefCell::default(),
            notified: RefCell::default(),
            accounts: RefCell::default(),
            app: app.downgrade(),
        });
        let weak = Rc::downgrade(&engine);
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                let Some(engine) = weak.upgrade() else { break };
                engine.event(event);
            }
        });
        let weak = Rc::downgrade(&engine);
        *engine.timer.borrow_mut() = Some(glib::timeout_add_local(
            Duration::from_secs(theme::SYNC_SECONDS),
            move || {
                let Some(engine) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let _ = engine.worker.send(Command::Discover);
                engine.worker.sync();
                glib::ControlFlow::Continue
            },
        ));
        let _ = engine.worker.send(Command::Discover);
        engine
    }

    pub fn attach(&self, listener: impl Fn(Event) + 'static) {
        *self.listener.borrow_mut() = Some(Box::new(listener));
    }

    pub fn detach(&self) {
        self.listener.borrow_mut().take();
        self.worker.select(self.worker.next_request());
    }

    pub fn stop(&self) {
        self.detach();
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
        self.worker.stop();
    }

    pub fn flush_notifications(&self) {
        let _ = self.worker.send(Command::FlushNotifications);
    }

    fn event(&self, event: Event) {
        match &event {
            Event::Accounts(accounts) => {
                self.worker.retain_accounts(accounts);
                *self.accounts.borrow_mut() = accounts
                    .iter()
                    .map(|account| account.email.clone())
                    .collect();
                for account in accounts {
                    self.worker.register(account.clone(), false);
                }
            }
            Event::SidebarReady if self.listener.borrow().is_none() => self.worker.sync(),
            _ => {}
        }
        if let Event::NewMail(account, folder, validity, messages) = event {
            if let Some(app) = self.app.upgrade()
                && !app.is_action_enabled("background")
            {
                return;
            }
            let uids: Vec<_> = messages.iter().map(|message| message.uid).collect();
            let fresh: Vec<_> = messages
                .iter()
                .filter(|message| {
                    self.notified.borrow_mut().insert((
                        account.clone(),
                        folder.clone(),
                        validity,
                        message.uid,
                    ))
                })
                .collect();
            if let Some(app) = self.app.upgrade()
                && !fresh.is_empty()
                && self.accounts.borrow().contains(&account)
                && app
                    .action_state("background")
                    .and_then(|state| state.get::<bool>())
                    .unwrap_or(false)
            {
                let notification = gio::Notification::new(&if fresh.len() == 1 {
                    fresh[0].sender.clone()
                } else {
                    format!("{} new messages", fresh.len())
                });
                notification.set_body(Some(&if fresh.len() == 1 {
                    fresh[0].subject.clone()
                } else {
                    account.clone()
                }));
                notification.set_icon(&gio::ThemedIcon::new(
                    "io.github.ExposedCat.Brevlada-symbolic",
                ));
                notification.set_default_action_and_target_value(
                    "app.open-message",
                    Some(&(account.clone(), validity, fresh[0].uid).to_variant()),
                );
                app.send_notification(Some(&format!("new-mail-{account}")), &notification);
            }
            let _ = self.worker.send(Command::NotificationsHandled {
                account,
                folder,
                validity,
                uids,
            });
        } else if let Some(listener) = self.listener.borrow().as_ref() {
            listener(event);
        }
    }
}

pub fn settings() -> anyhow::Result<Settings> {
    Settings::load(&glib::user_config_dir().join("brevlada/settings.json"))
}

pub fn save_background(enabled: bool) -> anyhow::Result<()> {
    let path = glib::user_config_dir().join("brevlada/settings.json");
    let mut settings = Settings::load(&path)?;
    settings.background_service = enabled;
    settings.save(&path)
}
