mod actions;
mod autosave;
mod calendar;
mod conversation;
mod drafts;
mod events;
mod lists;
mod message_navigation;
mod parcel_history;
mod previews;
mod read_state;
mod rendering;
#[cfg(test)]
mod response_tests;
mod search;
mod search_sender;
mod sender_actions;
mod sender_pane;
mod settings;
mod sorting;
mod tickets;
use crate::{
    backend::worker::{self, BodyRequest, Command, Event},
    components as ui,
    models::{self, Account, Message},
    theme,
};
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

struct State {
    sender: worker::Worker,
    sender_actions: RefCell<models::pending_actions::PendingActions>,
    account: RefCell<Option<Account>>,
    folder: RefCell<String>,
    generation: Cell<u64>,
    selection: Cell<u64>,
    pending: RefCell<HashSet<u32>>,
    preview_pending: RefCell<HashSet<u32>>,
    loading: Cell<bool>,
    rendering: Cell<bool>,
    messages: RefCell<Vec<Message>>,
    visible_limit: Cell<usize>,
    has_more_senders: Cell<bool>,
    folder_cache: RefCell<HashMap<(String, String), Vec<Message>>>,
    active_sender_row: ui::active_row::ActiveRow,
    active_thread_row: ui::active_row::ActiveRow,
    sender_pane: RefCell<models::sender_pane::SenderPane>,
    selected: RefCell<Vec<u32>>,
    next_message: Cell<Option<(u32, u32)>>,
    search_message: RefCell<Option<(u32, String)>>,
    search_sender: RefCell<Option<search_sender::SenderMatches>>,
    deferred_read_sort: RefCell<HashSet<u32>>,
    cards: RefCell<HashMap<u32, ui::viewer::Card>>,
    sent_cards: RefCell<HashMap<String, ui::viewer::Card>>,
    related_sent: RefCell<Vec<models::SentMessage>>,
    parcel_history: RefCell<HashMap<(String, models::parcel::Identifier), Vec<Message>>>,
    calendar_history: RefCell<HashMap<String, Vec<Message>>>,
    calendar_pending: RefCell<HashSet<(String, String, Option<String>)>>,
    draft_editors: RefCell<HashMap<drafts::Key, Rc<drafts::Editor>>>,
    draft_removed: RefCell<HashSet<drafts::Key>>,
    draft_local: RefCell<HashMap<drafts::Key, drafts::Local>>,
    composer_pending: RefCell<HashMap<u64, drafts::Pending>>,
    composer_request: Cell<u64>,
    sent_pending: RefCell<HashSet<String>>,
    open_group: RefCell<Vec<Message>>,
    trusted_senders: RefCell<HashMap<String, HashSet<String>>>,
    settings: RefCell<models::settings::Settings>,
    downloaded_media: RefCell<HashSet<(String, String, u32, String)>>,
    sidebar: gtk::Box,
    account_sidebar: ui::motion::Sidebar,
    account_title: gtk::Label,
    restore_accounts_on_back: Cell<bool>,
    thread_sidebar: ui::motion::Sidebar,
    thread_list: gtk::ListView,
    thread_scroll: gtk::ScrolledWindow,
    thread_stack: gtk::Stack,
    sync_status: ui::sync_status::SyncStatus,
    folder_boxes: RefCell<HashMap<String, (Account, gtk::Box)>>,
    folder_names: RefCell<HashMap<String, Vec<String>>>,
    unread: RefCell<HashMap<String, ui::sidebar::Unread>>,
    list: gtk::ListView,
    list_scroll: gtk::ScrolledWindow,
    thread_load_more: gtk::Button,
    viewer_scroll: gtk::ScrolledWindow,
    viewer: gtk::Box,
    viewer_reveal: ui::reveal::Reveal,
    conversation_limit: Cell<usize>,
    conversation_more: gtk::Button,
    list_stack: gtk::Stack,
    toast: adw::ToastOverlay,
    navigation_selection: ui::sidebar::Selection,
    expansion: ui::expansion::Expansion,
    avatars: Rc<ui::avatars::Avatars>,
    compose_button: gtk::Button,
    composers: gtk::Box,
    open_composers: RefCell<Vec<drafts::OpenComposer>>,
    refresh: gtk::Button,
    sync: gtk::Button,
    back: gtk::Button,
    sender_unread_first: ui::sort_menu::SortMenu,
    thread_unread_first: ui::sort_menu::SortMenu,
    search: gtk::SearchEntry,
    search_filters: Rc<ui::search_filters::SearchFilters>,
    search_locations: RefCell<HashMap<String, crate::backend::search::Location>>,
    search_cache_revision: Cell<u64>,
    search_progress: [gtk::Spinner; 2],
    search_running: [Cell<bool>; 2],
    search_show_progress: [Cell<bool>; 2],
    search_background_refresh: [Rc<Cell<bool>>; 2],
    search_refresh_again: [Cell<bool>; 2],
    search_multiaccount: Cell<bool>,
    search_preserve_results: [Cell<bool>; 2],
    search_published_queries: RefCell<[String; 2]>,
    search_account_order: RefCell<Option<Vec<String>>>,
    thread_search: gtk::SearchEntry,
    search_worker: crate::backend::search::Search,
    search_snapshots: RefCell<[Option<std::sync::Arc<Vec<Message>>>; 2]>,
    search_refresh_pending: [Rc<Cell<bool>>; 2],
}

fn load_trusted_senders() -> HashMap<String, HashSet<String>> {
    let path = glib::user_config_dir().join("brevlada/trusted-senders.json");
    match std::fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
            eprintln!("Could not restore trusted senders: {error}");
            HashMap::new()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => {
            eprintln!("Could not restore trusted senders: {error}");
            HashMap::new()
        }
    }
}

fn load_downloaded_media() -> HashSet<(String, String, u32, String)> {
    let path = glib::user_config_dir().join("brevlada/downloaded-media.json");
    match std::fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
            eprintln!("Could not restore downloaded media: {error}");
            HashSet::new()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashSet::new(),
        Err(error) => {
            eprintln!("Could not restore downloaded media: {error}");
            HashSet::new()
        }
    }
}

pub fn create(app: &adw::Application) {
    let shell = ui::shell::Shell::new(app);
    let window = shell.window.clone();
    let settings = shell.settings.clone();
    let (sender, events) = worker::start(glib::user_data_dir().join("brevlada/emails.db"));
    let queue = sender.avatars();
    let avatars = ui::avatars::Avatars::new(move |email| queue.push(email.to_owned()));
    let state = State::new(shell, sender, ui::expansion::Expansion::load(), avatars);
    state.connect_settings(
        &window,
        &settings,
        glib::user_config_dir().join("brevlada/settings.json"),
    );
    state.remember_sorting(glib::user_config_dir().join("brevlada/sorting.json"));
    for threads in [false, true] {
        let lookup = Rc::downgrade(&state);
        let activate = Rc::downgrade(&state);
        let trust_lookup = Rc::downgrade(&state);
        let trust_toggle = Rc::downgrade(&state);
        let menu_lookup = Rc::downgrade(&state);
        ui::sender_menu::attach_view(
            if threads {
                &state.thread_list
            } else {
                &state.list
            },
            if threads {
                &state.thread_scroll
            } else {
                &state.list_scroll
            },
            move |index| {
                let state = lookup.upgrade()?;
                let row = ui::virtual_list::item(
                    if threads {
                        &state.thread_list
                    } else {
                        &state.list
                    },
                    index.try_into().ok()?,
                )?;
                let group = &row.messages;
                let bulk = !threads;
                let target = if bulk {
                    models::action_target::ActionTarget::Sender(models::senders::key(
                        group.first()?,
                    ))
                } else {
                    models::action_target::ActionTarget::Messages(
                        group
                            .iter()
                            .map(|message| (message.uid, message.message_id.clone()))
                            .collect(),
                    )
                };
                let location = if threads {
                    None
                } else {
                    state.search_locations.borrow().get(&row.key).cloned()
                };
                Some(((target, location, row.messages), bulk))
            },
            move |(sender, location, _), action| {
                if let Some(state) = activate.upgrade()
                    && state.select_search_location(location.as_ref())
                {
                    state.sender_action(sender, action);
                }
            },
            move |(target, location, _)| {
                let Some(state) = trust_lookup.upgrade() else {
                    return false;
                };
                match target {
                    models::action_target::ActionTarget::Sender(sender) => {
                        if let Some(location) = location {
                            state
                                .trusted_senders
                                .borrow()
                                .get(&location.account)
                                .is_some_and(|senders| senders.contains(sender))
                        } else {
                            state.is_sender_trusted(sender)
                        }
                    }
                    _ => false,
                }
            },
            move |(target, location, _)| {
                if let Some(state) = trust_toggle.upgrade()
                    && let models::action_target::ActionTarget::Sender(sender) = target
                    && state.select_search_location(location.as_ref())
                {
                    state.toggle_trust(&sender);
                }
            },
            move |(target, location, messages)| {
                menu_lookup.upgrade().map_or_else(Vec::new, |state| {
                    if let Some(location) = location {
                        let current = state
                            .account
                            .borrow()
                            .as_ref()
                            .is_some_and(|account| account.email == location.account)
                            && *state.folder.borrow() == location.folder;
                        if !current {
                            return sender_actions::menu_actions_for_messages(
                                &location.folder,
                                messages,
                            );
                        }
                    }
                    state.menu_actions(target)
                })
            },
        );
    }
    let weak = Rc::downgrade(&state);
    state.back.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.close_sender_pane();
        }
    });
    let weak = Rc::downgrade(&state);
    state.refresh.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.load();
        }
    });
    let weak = Rc::downgrade(&state);
    state.sync.connect_clicked(move |button| {
        if let Some(state) = weak.upgrade() {
            button.set_sensitive(false);
            state.sender.sync();
        }
    });
    let weak = Rc::downgrade(&state);
    state.thread_load_more.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.load_sender_page();
        }
    });
    let weak = Rc::downgrade(&state);
    state
        .list_scroll
        .vadjustment()
        .connect_value_changed(move |_| {
            if let Some(state) = weak.upgrade() {
                state.maybe_load_more();
            }
        });
    let weak = Rc::downgrade(&state);
    state.list_scroll.vadjustment().connect_changed(move |_| {
        if let Some(state) = weak.upgrade() {
            state.maybe_load_more();
        }
    });
    let weak = Rc::downgrade(&state);
    state
        .thread_scroll
        .vadjustment()
        .connect_value_changed(move |_| {
            if let Some(state) = weak.upgrade() {
                state.maybe_load_sender_page();
            }
        });
    let weak = Rc::downgrade(&state);
    let timer = glib::timeout_add_local(Duration::from_secs(theme::SYNC_SECONDS), move || {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !state.loading.get() {
            state.load();
        }
        state.sender.sync();
        glib::ControlFlow::Continue
    });
    let owner = RefCell::new(Some(state.clone()));
    let weak = Rc::downgrade(&state);
    let status_timer = glib::timeout_add_local(Duration::from_secs(1), move || {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(status) = state.sender.sync_status() {
            state.sync.set_sensitive(
                status.active == 0 && status.queued == 0 && !state.folder_boxes.borrow().is_empty(),
            );
            state.sync_status.update(&status);
        }
        glib::ControlFlow::Continue
    });
    let status_timer = RefCell::new(Some(status_timer));
    let timer = RefCell::new(Some(timer));
    window.connect_close_request(move |_| {
        owner.borrow_mut().take();
        if let Some(timer) = timer.borrow_mut().take() {
            timer.remove();
        }
        if let Some(timer) = status_timer.borrow_mut().take() {
            timer.remove();
        }
        glib::Propagation::Proceed
    });
    let weak = Rc::downgrade(&state);
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            let Some(state) = weak.upgrade() else {
                break;
            };
            state.event(event);
        }
    });
    state.send(Command::Discover);
    window.present();
}

impl State {
    fn mark_media_downloaded(&self, key: (String, String, u32, String)) {
        if !self.downloaded_media.borrow_mut().insert(key) {
            return;
        }
        let path = glib::user_config_dir().join("brevlada/downloaded-media.json");
        let save = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            glib::file_set_contents(
                &path,
                &serde_json::to_vec(&*self.downloaded_media.borrow())?,
            )?;
            Ok(())
        })();
        if let Err(error) = save {
            self.toast.add_toast(adw::Toast::new(&format!(
                "Could not save downloaded media: {error}"
            )));
        }
    }

    fn media_key(&self, message: &Message) -> (String, String, u32, String) {
        (
            self.account
                .borrow()
                .as_ref()
                .map_or(String::new(), |account| account.email.clone()),
            self.folder.borrow().clone(),
            message.uid,
            message.message_id.clone(),
        )
    }

    fn is_trusted(&self, sender: &str) -> bool {
        self.settings.borrow().trust_all_senders || self.is_sender_trusted(sender)
    }

    fn is_sender_trusted(&self, sender: &str) -> bool {
        self.account.borrow().as_ref().is_some_and(|account| {
            self.trusted_senders
                .borrow()
                .get(&account.email)
                .is_some_and(|senders| senders.contains(sender))
        })
    }

    fn toggle_trust(&self, sender: &str) {
        let Some(account) = self
            .account
            .borrow()
            .as_ref()
            .map(|account| account.email.clone())
        else {
            return;
        };
        let own = models::senders::address(&account) == sender;
        let trusted = {
            let mut accounts = self.trusted_senders.borrow_mut();
            let senders = accounts.entry(account).or_default();
            if !senders.insert(sender.to_owned()) {
                senders.remove(sender);
                false
            } else {
                true
            }
        };
        let path = glib::user_config_dir().join("brevlada/trusted-senders.json");
        let save = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            glib::file_set_contents(&path, &serde_json::to_vec(&*self.trusted_senders.borrow())?)?;
            Ok(())
        })();
        if let Err(error) = save {
            self.toast.add_toast(adw::Toast::new(&format!(
                "Could not save trusted senders: {error}"
            )));
        }
        for message in self
            .messages
            .borrow()
            .iter()
            .filter(|message| models::senders::key(message) == sender)
        {
            if let Some(card) = self.cards.borrow().get(&message.uid) {
                card.set_trusted(trusted || own || self.settings.borrow().trust_all_senders);
            }
        }
    }

    fn new(
        shell: ui::shell::Shell,
        sender: worker::Worker,
        expansion: ui::expansion::Expansion,
        avatars: Rc<ui::avatars::Avatars>,
    ) -> Rc<Self> {
        let ui::shell::Shell {
            window,
            sidebar,
            account_sidebar,
            account_title,
            thread_sidebar,
            thread_list,
            thread_scroll,
            thread_stack,
            sync_status,
            list,
            list_scroll,
            thread_load_more,
            viewer_scroll,
            list_stack,
            viewer,
            viewer_reveal,
            compose_button,
            composers,
            refresh,
            sync,
            back,
            sender_unread_first,
            thread_unread_first,
            search,
            search_filters,
            search_progress,
            thread_search,
            toast,
            settings: _,
        } = shell;
        let (search_worker, search_results) = crate::backend::search::Search::start();
        let conversation_more = gtk::Button::with_label("Load older messages");
        conversation_more.set_halign(gtk::Align::Center);
        conversation_more.set_margin_top(theme::SMALL_SPACING);
        conversation_more.set_margin_bottom(theme::SPACING);
        let state = Rc::new(State {
            sender,
            account: RefCell::new(None),
            folder: RefCell::new(String::new()),
            generation: Cell::new(0),
            selection: Cell::new(0),
            pending: RefCell::new(HashSet::new()),
            preview_pending: RefCell::new(HashSet::new()),
            sender_actions: RefCell::default(),
            loading: Cell::new(false),
            rendering: Cell::new(false),
            messages: RefCell::new(Vec::new()),
            visible_limit: Cell::new(theme::MESSAGE_LIMIT),
            has_more_senders: Cell::new(false),
            folder_cache: RefCell::default(),
            active_sender_row: ui::active_row::ActiveRow::default(),
            active_thread_row: ui::active_row::ActiveRow::default(),
            restore_accounts_on_back: Cell::new(false),
            sender_pane: RefCell::default(),
            selected: RefCell::new(Vec::new()),
            next_message: Cell::new(None),
            search_message: RefCell::default(),
            search_sender: RefCell::default(),
            deferred_read_sort: RefCell::default(),
            cards: RefCell::new(HashMap::new()),
            sent_cards: RefCell::new(HashMap::new()),
            related_sent: RefCell::new(Vec::new()),
            parcel_history: RefCell::default(),
            calendar_history: RefCell::default(),
            calendar_pending: RefCell::default(),
            draft_editors: RefCell::default(),
            draft_removed: RefCell::default(),
            draft_local: RefCell::default(),
            composer_pending: RefCell::default(),
            composer_request: Cell::new(0),
            sent_pending: RefCell::new(HashSet::new()),
            open_group: RefCell::new(Vec::new()),
            trusted_senders: RefCell::new(load_trusted_senders()),
            settings: RefCell::default(),
            downloaded_media: RefCell::new(load_downloaded_media()),
            sidebar,
            account_sidebar,
            account_title,
            thread_sidebar,
            thread_list,
            thread_scroll,
            thread_stack,
            sync_status,
            folder_boxes: RefCell::new(HashMap::new()),
            folder_names: RefCell::new(HashMap::new()),
            unread: RefCell::new(HashMap::new()),
            list,
            list_scroll,
            thread_load_more,
            viewer_scroll,
            viewer,
            viewer_reveal,
            conversation_limit: Cell::new(25),
            conversation_more,
            list_stack,
            toast,
            navigation_selection: ui::sidebar::Selection::default(),
            expansion,
            avatars,
            compose_button,
            composers,
            open_composers: RefCell::default(),
            refresh,
            sync,
            back,
            sender_unread_first,
            thread_unread_first,
            search,
            search_filters,
            search_progress,
            search_running: Default::default(),
            search_show_progress: Default::default(),
            search_background_refresh: Default::default(),
            search_refresh_again: Default::default(),
            search_multiaccount: Cell::new(false),
            search_preserve_results: Default::default(),
            search_published_queries: Default::default(),
            thread_search,
            search_worker,
            search_locations: RefCell::default(),
            search_cache_revision: Cell::new(0),
            search_account_order: RefCell::new(None),
            search_snapshots: RefCell::new([None, None]),
            search_refresh_pending: [Rc::default(), Rc::default()],
        });
        let weak = Rc::downgrade(&state);
        state.conversation_more.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state
                    .conversation_limit
                    .set(state.conversation_limit.get() + 25);
                ui::scroll_position::preserve_offset(&state.viewer_scroll, || {
                    state.render_conversation()
                });
            }
        });
        state.connect_search(search_results);
        for toggle in [&state.sender_unread_first, &state.thread_unread_first] {
            let weak = Rc::downgrade(&state);
            toggle.connect_changed(move || {
                if let Some(state) = weak.upgrade() {
                    *state.search_account_order.borrow_mut() = None;
                    state.render_list();
                }
            });
        }
        state.connect_lists();
        let weak = Rc::downgrade(&state);
        ui::mail_shortcuts::attach_open_message(
            &window,
            &state.thread_list,
            &state.composers,
            move |action| {
                let Some(state) = weak.upgrade() else {
                    return false;
                };
                let messages: Vec<_> = state
                    .open_group
                    .borrow()
                    .iter()
                    .map(|message| (message.uid, message.message_id.clone()))
                    .collect();
                if messages.is_empty() {
                    return false;
                }
                state.sender_action(
                    models::action_target::ActionTarget::Messages(messages),
                    action,
                );
                true
            },
        );
        let weak = Rc::downgrade(&state);
        state.account_sidebar.connect_visible_notify(move |_| {
            if let Some(state) = weak.upgrade() {
                // A later manual visibility change overrides the automatic collapse.
                state.restore_accounts_on_back.set(false);
                state.refresh_account_title();
            }
        });
        let weak = Rc::downgrade(&state);
        state.compose_button.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                let receiver = state
                    .sender_pane
                    .borrow()
                    .sender()
                    .unwrap_or_default()
                    .to_owned();
                state.new_composer(&receiver, None);
            }
        });
        state
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn sender_ticket(state: &Rc<State>) -> u64 {
        if !state.sender_pane.borrow().loading() {
            state.sender_pane.borrow_mut().request();
        }
        state.sender_pane.borrow().ticket()
    }

    fn unread_test_state(id: &str) -> Rc<State> {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id(id)
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let state = State::new(
            ui::shell::Shell::new(&app),
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        state
    }

    #[test]
    #[ignore = "Requires a graphical session for the composer and draft cards"]
    fn sending_preserves_failed_edits_and_labels_injected_drafts() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaDraftDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            ui::shell::Shell::new(&app),
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        let received = Message {
            uid: 1,
            message_id: "incoming".into(),
            subject: "Topic".into(),
            sender: "other@example.com".into(),
            body_loaded: true,
            ..Default::default()
        };
        let compose = state
            .new_composer(
                "",
                Some((&received, "INBOX", models::response::Action::Reply)),
            )
            .unwrap();
        let header = compose
            .widget
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap();
        let error = compose
            .widget
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert!(error.has_css_class("error"));
        assert!(!error.get_visible());
        let subject = header
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        compose.load_draft(&models::Draft {
            to: "other@example.com".into(),
            subject: String::new(),
            text: "Reply body".into(),
            in_reply_to: Some("incoming".into()),
            references: vec!["incoming".into()],
            ..models::Draft::from(&received)
        });
        let send = header
            .last_child()
            .unwrap()
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(!send.is_sensitive());
        assert_eq!(send.tooltip_text().as_deref(), Some("Send"));
        assert!(commands.try_recv().is_err());
        subject.set_text("Re: Topic");
        send.emit_clicked();
        send.emit_clicked();
        assert!(!compose.widget.is_sensitive());
        let context = gtk::glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        let Command::Compose {
            account,
            draft,
            request,
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected message send")
        };
        assert_eq!(account.email, "account@example.com");
        assert_eq!(draft.to, "other@example.com");
        assert_eq!(draft.subject, "Re: Topic");
        assert_eq!(draft.in_reply_to.as_deref(), Some("incoming"));
        assert!(draft.references.contains(&"incoming".into()));
        assert!(commands.try_recv().is_err());
        state.event(Event::Composed(request, Err("Offline".into())));
        assert!(compose.widget.is_sensitive());
        assert!(compose.widget.get_visible());
        assert!(error.get_visible());
        assert_eq!(error.text(), "Could not send message: Offline");
        assert_eq!(
            header
                .first_child()
                .unwrap()
                .downcast::<gtk::Entry>()
                .unwrap()
                .text(),
            "other@example.com"
        );
        subject.set_text("Updated topic");
        send.emit_clicked();
        send.emit_clicked();
        assert!(!error.get_visible());
        while context.pending() {
            context.iteration(false);
        }
        let Command::Compose { request, draft, .. } = commands.try_recv().unwrap() else {
            panic!("Expected send")
        };
        assert_eq!(draft.subject, "Updated topic");
        assert_eq!(draft.text, "Reply body");
        assert_eq!(draft.in_reply_to.as_deref(), Some("incoming"));
        assert!(commands.try_recv().is_err());
        state.event(Event::Composed(
            request,
            Ok(models::draft::Outcome {
                cleanup_error: None,
                message: Message {
                    message_id: "sent".into(),
                    body_loaded: true,
                    is_read: true,
                    subject: "Re: Topic".into(),
                    recipients: "other@example.com".into(),
                    ..Default::default()
                },
            }),
        ));
        assert!(!compose.widget.get_visible());
        assert!(!error.get_visible());
        assert!(subject.text().is_empty());
        *state.messages.borrow_mut() = vec![received.clone()];
        state.show_thread(vec![received]);
        state.event(Event::RelatedSent(
            state.generation.get(),
            state.selection.get(),
            vec![models::SentMessage {
                folder: "Drafts".into(),
                message: Message {
                    uid: 2,
                    message_id: "draft".into(),
                    is_draft: true,
                    is_read: true,
                    recipients: "other@example.com".into(),
                    body_loaded: true,
                    ..Default::default()
                },
            }],
        ));
        assert!(!state.sent_cards.borrow().contains_key("draft"));
        assert!(state.sent_cards.borrow().contains_key("sent"));
        let key = state.draft_key("Drafts", "draft");
        let editors = state.draft_editors.borrow();
        assert_eq!(
            editors[&key].widget.parent(),
            Some(state.viewer.clone().upcast())
        );
        assert!(editors[&key].compose.widget.is_sensitive());
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK message cards"]
    fn conversation_updates_keep_existing_cards_parented() {
        let state = unread_test_state("org.example.BrevladaCardLifecycleDiagnostic");
        let first = Message {
            uid: 1,
            message_id: "first".into(),
            timestamp: 3,
            sender: "sender@example.com".into(),
            ..Default::default()
        };
        let older = Message {
            uid: 2,
            message_id: "older".into(),
            timestamp: 1,
            ..first.clone()
        };
        *state.messages.borrow_mut() = vec![first.clone(), older.clone()];
        state.show_thread(vec![first, older]);
        let card = state.cards.borrow()[&1].widget.clone();
        let parent_changes = Rc::new(Cell::new(0));
        let changed = parent_changes.clone();
        card.connect_parent_notify(move |_| changed.set(changed.get() + 1));
        state.event(Event::RelatedSent(
            state.generation.get(),
            state.selection.get(),
            vec![models::SentMessage {
                folder: "Sent".into(),
                message: Message {
                    uid: 3,
                    message_id: "sent".into(),
                    timestamp: 2,
                    sender: "account@example.com".into(),
                    ..Default::default()
                },
            }],
        ));
        assert_eq!(state.cards.borrow()[&1].widget, card);
        assert_eq!(parent_changes.get(), 0);
        assert_eq!(card.parent(), Some(state.viewer.clone().upcast()));
        assert_eq!(state.sent_cards.borrow().len(), 1);
    }

    #[test]
    #[ignore = "Requires a graphical session for account-aware sender activation"]
    fn search_result_activation_uses_its_owning_account_and_folder() {
        let state = unread_test_state("org.example.BrevladaSearchAccountDiagnostic");
        let mut other = state.account.borrow().clone().unwrap();
        other.email = "other@example.com".into();
        let message = Message {
            uid: 1,
            sender: "sender@example.com".into(),
            search_match: true,
            ..Default::default()
        };
        state.folder_boxes.borrow_mut().insert(
            other.email.clone(),
            (other.clone(), gtk::Box::new(gtk::Orientation::Vertical, 0)),
        );
        state.folder_cache.borrow_mut().insert(
            (other.email.clone(), "Archive".into()),
            vec![message.clone()],
        );
        state.publish_search_groups(
            false,
            "",
            vec![vec![message]],
            vec![Some(crate::backend::search::Location {
                account: other.email.clone(),
                folder: "Archive".into(),
            })],
        );
        state.list.emit_by_name::<()>("activate", &[&0u32]);
        assert_eq!(state.account.borrow().as_ref().unwrap().email, other.email);
        assert_eq!(*state.folder.borrow(), "Archive");
        assert_eq!(
            state.sender_pane.borrow().sender(),
            Some("sender@example.com")
        );
        assert_eq!(
            state
                .search_sender
                .borrow()
                .as_ref()
                .unwrap()
                .messages
                .len(),
            1
        );
        assert_eq!(ui::virtual_list::groups(&state.thread_list)[0][0].uid, 1);
    }

    #[test]
    #[ignore = "Requires a graphical session for search pane navigation"]
    fn back_restores_accounts_after_search_results_switch_mailboxes() {
        let state = unread_test_state("org.example.BrevladaSearchBackDiagnostic");
        let first = state.account.borrow().clone().unwrap();
        let second = Account {
            email: "second@example.com".into(),
            ..first.clone()
        };
        for account in [&first, &second] {
            state.folder_boxes.borrow_mut().insert(
                account.email.clone(),
                (
                    account.clone(),
                    gtk::Box::new(gtk::Orientation::Vertical, 0),
                ),
            );
        }
        let bar = state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap();
        bar.set_search_mode(true);
        state.search.set_text("sender");
        state.cancel_search(false);
        state.publish_search_groups(
            false,
            "sender",
            (1..=3)
                .map(|uid| {
                    vec![Message {
                        uid,
                        sender: format!("sender{uid}@example.com"),
                        search_match: true,
                        ..Default::default()
                    }]
                })
                .collect(),
            [&first, &second, &second]
                .into_iter()
                .zip(["INBOX", "INBOX", "Archive"])
                .map(|(account, folder)| {
                    Some(crate::backend::search::Location {
                        account: account.email.clone(),
                        folder: folder.into(),
                    })
                })
                .collect(),
        );

        state.account_sidebar.set_visible(true);
        for position in [0, 0, 1, 2] {
            state.activate_sender(position);
            assert!(!state.account_sidebar.get_visible());
            assert!(state.thread_sidebar.get_visible());
            assert!(state.restore_accounts_on_back.get());
        }
        state.filter_sender(None);
        assert!(state.account_sidebar.get_visible());
        assert!(!state.thread_sidebar.get_visible());
        assert!(!state.back.get_visible());
        assert!(state.sender_search_active());
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 3);

        // A pane hidden before activation stays hidden after Back.
        state.account_sidebar.set_visible(false);
        for position in [0, 1] {
            state.activate_sender(position);
        }
        state.filter_sender(None);
        assert!(!state.account_sidebar.get_visible());

        // Manual visibility changes also override restoration across mailboxes.
        state.account_sidebar.set_visible(true);
        state.activate_sender(0);
        state.account_sidebar.set_visible(true);
        state.account_sidebar.set_visible(false);
        state.activate_sender(1);
        state.filter_sender(None);
        assert!(!state.account_sidebar.get_visible());

        state.account_sidebar.set_visible(true);
        state.activate_sender(0);
        let thread_bar = state
            .thread_search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap();
        thread_bar.set_search_mode(true);
        state.thread_search.set_text("sender");
        state.close_sender_pane();
        assert!(state.account_sidebar.get_visible());
        assert!(!state.thread_sidebar.get_visible());
        assert!(!state.back.get_visible());
        assert!(!state.sender_search_active());
        assert!(!state.search_open(true));
        assert!(state.search.text().is_empty());
        assert!(state.thread_search.text().is_empty());
        assert!(!state.search_running[0].get());
        assert!(!state.search_running[1].get());
        assert!(state.search_sender.borrow().is_none());
        assert!(state.search_locations.borrow().is_empty());
    }

    #[test]
    #[ignore = "Requires GTK and an isolated XDG_DATA_HOME matching BREVLADA_SEARCH_TEST_DATA"]
    fn account_search_without_current_appends_results_and_closing_restores_normal_loading() {
        let isolated = std::env::var_os("BREVLADA_SEARCH_TEST_DATA")
            .expect("Set isolated test data directory");
        assert_eq!(glib::user_data_dir(), std::path::PathBuf::from(isolated));
        let state = unread_test_state("org.example.BrevladaSearchLifecycleDiagnostic");
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.present();
        let first = state.account.borrow().clone().unwrap();
        let second = Account {
            email: "second@example.com".into(),
            ..first.clone()
        };
        *state.account.borrow_mut() = None;
        state.folder.borrow_mut().clear();
        let alpha = Message {
            uid: 1,
            timestamp: 1,
            sender: "alpha@example.com".into(),
            ..Default::default()
        };
        let beta = Message {
            uid: 1,
            timestamp: 2,
            sender: "beta@example.com".into(),
            ..Default::default()
        };
        let storage = crate::backend::storage::Storage::open(
            &glib::user_data_dir().join("brevlada/emails.db"),
        )
        .unwrap();
        storage.store(&first.email, "INBOX", &alpha).unwrap();
        storage.store(&second.email, "INBOX", &beta).unwrap();
        state.event(Event::Accounts(vec![first.clone(), second.clone()]));
        // A stale label from a previously displayed account cannot become the
        // title when the sidebar closes without a selected mailbox.
        state.account_title.set_label(&first.email);
        state.account_sidebar.set_visible(false);
        assert!(state.account_title.text().is_empty());
        let bar = state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap();
        bar.set_search_mode(true);
        assert_eq!(state.account_title.text(), "All accounts");
        assert!(state.account_title.get_visible());
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 0);
        assert!(!state.search_running[0].get());
        state.search.set_text("example.com");
        assert!(state.search_running[0].get());
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("state")
        );
        let accounts = state
            .search_filters
            .widget
            .first_child()
            .unwrap()
            .first_child()
            .and_downcast::<gtk::MenuButton>()
            .unwrap()
            .popover()
            .unwrap()
            .child()
            .and_downcast::<gtk::Box>()
            .unwrap();
        assert_eq!(state.search_filters.selected_accounts().len(), 2);
        let wait = |predicate: &dyn Fn() -> bool| {
            let context = glib::MainContext::default();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !predicate() && std::time::Instant::now() < deadline {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(predicate(), "Search did not finish");
        };
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 2
        });
        wait(&|| labels(state.list.clone().upcast()).contains(&format!("{} · INBOX", first.email)));
        assert!(labels(state.list.clone().upcast()).contains(&format!("{} · INBOX", second.email)));
        assert!(state.account.borrow().is_none());
        assert_eq!(state.account_title.text(), "All accounts");
        let second_check = accounts
            .last_child()
            .unwrap()
            .prev_sibling()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        second_check.set_active(false);
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 1
        });
        wait(&|| labels(state.list.clone().upcast()).contains(&"INBOX".into()));
        assert!(!labels(state.list.clone().upcast()).contains(&format!("{} · INBOX", first.email)));
        let retained = ui::virtual_list::item(&state.list, 0).unwrap().key;
        second_check.set_active(true);
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 2
        });
        assert_eq!(
            ui::virtual_list::item(&state.list, 0).unwrap().key,
            retained
        );
        let groups = ui::virtual_list::groups(&state.list);
        assert_eq!(groups[0][0].sender, alpha.sender);
        assert_eq!(groups[1][0].sender, beta.sender);
        let row = ui::virtual_list::model(&state.list).item(0).unwrap();
        let loader_shows = Rc::new(Cell::new(0));
        let observed = loader_shows.clone();
        let loader_observer = state.search_progress[0].connect_visible_notify(move |spinner| {
            if spinner.is_visible() {
                observed.set(observed.get() + 1);
            }
        });
        state.list.emit_by_name::<()>("activate", &[&1u32]);
        assert!(
            !state.search_running[0].get(),
            "Opening a result must not restart search"
        );
        assert_eq!(state.account.borrow().as_ref().unwrap().email, second.email);
        assert_eq!(state.search_filters.selected_accounts().len(), 2);
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
        wait(&|| !state.search_running[0].get());
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
        assert_eq!(ui::virtual_list::model(&state.list).item(0).unwrap(), row);
        state.event(Event::Messages(
            state.generation.get(),
            vec![beta.clone()],
            false,
        ));
        state.show_thread(vec![beta.clone()]);
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(350);
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            loader_shows.get(),
            0,
            "Opening mail must not show search progress"
        );
        assert_eq!(ui::virtual_list::model(&state.list).item(0).unwrap(), row);
        state.search_progress[0].disconnect(loader_observer);
        state.search.set_text(&alpha.sender);
        state.filter_sender(Some(alpha.clone()));
        state.render_list(); // Queue a refresh that must not revive a closed search.
        bar.set_search_mode(false);
        assert!(state.search.text().is_empty());
        assert!(state.sender_pane.borrow().sender().is_none());
        assert!(!state.sender_search_active());
        state.select(first, "INBOX".into());
        state.event(Event::Messages(
            state.generation.get(),
            vec![alpha.clone()],
            false,
        ));
        state.event(Event::CacheList(
            second.email.clone(),
            "INBOX".into(),
            vec![beta],
        ));
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!state.sender_search_active());
        assert!(state.search_locations.borrow().is_empty());
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 1);
        assert_eq!(
            ui::virtual_list::groups(&state.list)[0][0].sender,
            alpha.sender
        );
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("list")
        );
        let archived = Message {
            uid: 1,
            timestamp: 3,
            sender: "archived@example.com".into(),
            ..Default::default()
        };
        storage.store(&second.email, "Archive", &archived).unwrap();
        bar.set_search_mode(true);
        state.search.set_text("archived@example.com");
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 1
        });
        assert_eq!(
            state
                .search_locations
                .borrow()
                .values()
                .next()
                .unwrap()
                .folder,
            "Archive"
        );
        let folder_choices = state
            .search_filters
            .widget
            .first_child()
            .unwrap()
            .last_child()
            .and_downcast::<gtk::MenuButton>()
            .unwrap()
            .popover()
            .unwrap()
            .child()
            .unwrap();
        let current_folder = folder_choices
            .last_child()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        assert!(current_folder.is_sensitive());
        current_folder.set_active(true);
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 0
        });
        assert_eq!(state.search_filters.folder().as_deref(), Some("INBOX"));
        state.search.set_text("example.com");
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 2
        });
        // Only the second account has Archive. Navigation updates Current Folder
        // and searching that path must ignore the first account's Inbox.
        state.select(second, "Archive".into());
        wait(&|| {
            !state.search_running[0].get() && ui::virtual_list::model(&state.list).n_items() == 1
        });
        assert_eq!(state.search_filters.folder().as_deref(), Some("Archive"));
        assert_eq!(
            ui::virtual_list::groups(&state.list)[0][0].sender,
            archived.sender
        );
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session to verify streamed search feedback"]
    fn sender_search_streams_with_spinner_and_cancels_on_folder_change() {
        let state = unread_test_state("org.example.BrevladaStreamDiagnostic");
        let account = state.account.borrow().clone().unwrap();
        state
            .search_filters
            .set_accounts(std::slice::from_ref(&account));
        state.search_filters.set_current(Some(&account.email));
        *state.messages.borrow_mut() = (1..=1500)
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                sender: format!("sender{uid}@example.com"),
                subject: if uid == 1500 {
                    "Needle".into()
                } else {
                    "Unrelated".into()
                },
                body_html: "<p>Other cached HTML text</p>".repeat(128),
                ..Default::default()
            })
            .collect();
        let window = state
            .toast
            .root()
            .and_downcast::<adw::ApplicationWindow>()
            .unwrap();
        window.present();
        let bar = state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap();
        bar.set_search_mode(true);
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 0);
        assert!(labels(state.list_stack.clone().upcast()).contains(&"Enter a search query".into()));
        state.search.set_text("needle");
        assert!(state.search_running[0].get());
        assert!(!state.search_progress[0].is_visible());
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("state")
        );
        let context = glib::MainContext::default();
        let wait = |predicate: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !predicate() && std::time::Instant::now() < deadline {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(predicate(), "Search feedback did not arrive");
        };
        wait(&|| ui::virtual_list::model(&state.list).n_items() == 1);
        assert!(
            state.search_running[0].get(),
            "First hit must precede scan completion"
        );
        assert!(state.search_progress[0].is_visible());
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("list")
        );
        wait(&|| labels(state.list.clone().upcast()).contains(&"INBOX".into()));
        assert!(
            !labels(state.list.clone().upcast()).contains(&format!("{} · INBOX", account.email))
        );
        state.render_list(); // A cache refresh must not restart an unfinished scan.
        assert!(state.search_running[0].get());
        assert!(state.search_progress[0].is_visible());
        assert!(state.search_refresh_again[0].get());
        let archived = Message {
            uid: 1,
            sender: "archive@example.com".into(),
            subject: "Needle".into(),
            ..Default::default()
        };
        state
            .folder_cache
            .borrow_mut()
            .insert((account.email.clone(), "Archive".into()), vec![archived]);
        state.select(account, "Archive".into());
        wait(&|| {
            !state.search_running[0].get()
                && ui::virtual_list::groups(&state.list)
                    .first()
                    .is_some_and(|group| group[0].sender == "archive@example.com")
        });
        assert!(!state.search_progress[0].is_visible());
        wait(&|| labels(state.list.clone().upcast()).contains(&"Archive".into()));
        state.search.set_text("no matches");
        wait(&|| !state.search_running[0].get());
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 0);
        assert!(labels(state.list_stack.clone().upcast()).contains(&"No senders found".into()));
        bar.set_search_mode(false);
        assert!(!state.search_running[0].get());
        assert!(!state.search_progress[0].is_visible());
        window.close();
    }

    #[test]
    #[ignore = "Requires GTK to verify large histories and stable search refreshes"]
    fn large_histories_keep_rows_light_and_search_refreshes_visible() {
        let state = unread_test_state("org.example.BrevladaLargeHistoryDiagnostic");
        let account = state.account.borrow().clone().unwrap();
        state
            .search_filters
            .set_accounts(std::slice::from_ref(&account));
        state.search_filters.set_current(Some(&account.email));
        let messages: Vec<_> = (1..=5000)
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                message_id: format!("message-{uid}"),
                sender: "busy@example.com".into(),
                subject: format!("Message {uid}"),
                is_read: true,
                body_text: "Cached text ".repeat(1024),
                body_html: "<p>Cached HTML</p>".repeat(1024),
                list_preview: Some("Cached text".into()),
                display_prepared: true,
                remote_media: Some(false),
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages;
        let started = std::time::Instant::now();
        state.render_list();
        eprintln!(
            "5,000 cached messages, sender render: {:?}",
            started.elapsed()
        );
        let sender_row = ui::virtual_list::model(&state.list).item(0).unwrap();
        assert!(
            ui::virtual_list::groups(&state.list)
                .iter()
                .flatten()
                .all(|message| message.body_text.is_empty()
                    && message.body_html.is_empty()
                    && message.inline_media.is_empty())
        );
        let latest = state.messages.borrow().last().unwrap().clone();
        state.filter_sender(Some(latest.clone()));
        let page = state.sender_pane.borrow_mut().request().unwrap().ticket;
        let started = std::time::Instant::now();
        let batch = state
            .messages
            .borrow()
            .iter()
            .rev()
            .take(25)
            .cloned()
            .collect();
        state.event(Event::SenderPage(state.generation.get(), page, batch, true));
        eprintln!("First sender page: {:?}", started.elapsed());
        assert_eq!(ui::virtual_list::model(&state.thread_list).n_items(), 25);
        assert!(
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .all(|message| message.body_html.is_empty() && message.body_text.len() < 200)
        );
        let untouched = ui::virtual_list::model(&state.thread_list).item(1).unwrap();
        let mut changed = latest;
        changed.body_loaded = true;
        changed.display_prepared = true;
        changed.body_html.push_str("<p>New body</p>");
        let started = std::time::Instant::now();
        state.update_body(&changed);
        eprintln!(
            "Opening one cached body in 5,000 messages: {:?}",
            started.elapsed()
        );
        assert_eq!(
            ui::virtual_list::model(&state.list).item(0).unwrap(),
            sender_row
        );
        assert_eq!(
            ui::virtual_list::model(&state.thread_list).item(1).unwrap(),
            untouched
        );
        let groups = vec![
            vec![Message {
                uid: 10,
                sender: "a@example.com".into(),
                ..Default::default()
            }],
            vec![Message {
                uid: 20,
                sender: "b@example.com".into(),
                ..Default::default()
            }],
        ];
        let location = crate::backend::search::Location {
            account: account.email,
            folder: "INBOX".into(),
        };
        let bar = state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap();
        bar.set_search_mode(true);
        state.search.set_text("example.com");
        state.cancel_search(false);
        state.publish_search_groups(
            false,
            "example.com",
            groups.clone(),
            vec![Some(location.clone()); 2],
        );
        let retained = ui::virtual_list::model(&state.list).item(1).unwrap();
        state.begin_search(false, true);
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("list")
        );
        state.publish_search_groups(
            false,
            "example.com",
            groups[..1].to_vec(),
            vec![Some(location)],
        );
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
        assert_eq!(
            ui::virtual_list::model(&state.list).item(1).unwrap(),
            retained
        );
        state.cancel_search(false);
        bar.set_search_mode(false);
    }

    #[test]
    #[ignore = "Requires GTK to verify bounded conversation rendering"]
    fn long_conversations_render_in_batches_and_keep_existing_cards() {
        let state = unread_test_state("org.example.BrevladaLongConversationDiagnostic");
        let messages: Vec<_> = (1..=2000)
            .map(|uid| Message {
                uid,
                message_id: format!("message-{uid}"),
                timestamp: uid as i64,
                sender: "busy@example.com".into(),
                subject: "Same conversation".into(),
                is_read: true,
                body_loaded: true,
                body_text: "Cached body".repeat(1024),
                display_prepared: true,
                list_preview: Some("Cached body".into()),
                remote_media: Some(false),
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages;
        let headers: Vec<_> = state
            .messages
            .borrow()
            .iter()
            .map(|message| message.list_header(true))
            .collect();
        let started = std::time::Instant::now();
        state.show_thread(headers);
        eprintln!("Open 2,000-message conversation: {:?}", started.elapsed());
        assert_eq!(state.cards.borrow().len(), 25);
        assert!(state.conversation_more.parent().is_some());
        let first = state.cards.borrow().get(&2000).unwrap().widget.clone();
        state.conversation_more.emit_clicked();
        assert_eq!(state.cards.borrow().len(), 50);
        assert_eq!(state.cards.borrow().get(&2000).unwrap().widget, first);
        assert_eq!(first.parent(), Some(state.viewer.clone().upcast()));
        state.new_selection();
    }

    #[test]
    #[ignore = "Requires a graphical session for asynchronous search and sender navigation"]
    fn independent_searches_include_cached_history_and_cancel_on_navigation() {
        let state = unread_test_state("org.example.BrevladaSearchDiagnostic");
        let old = Message {
            uid: 1,
            timestamp: 1,
            sender: "Alice <alice@example.com>".into(),
            recipients: "Receiver <receiver@example.com>".into(),
            cc: "Copy <copy@example.com>".into(),
            subject: "Older subject".into(),
            body_text: "A needle in cached history".into(),
            ..Default::default()
        };
        let latest = Message {
            uid: 80,
            timestamp: 80,
            subject: "Latest subject".into(),
            body_text: "New message".into(),
            ..old.clone()
        };
        let other = Message {
            uid: 90,
            timestamp: 90,
            sender: "Bob <bob@example.com>".into(),
            body_text: "A needle from another sender".into(),
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![old, latest.clone(), other.clone()];
        state.render_list();
        let wait = |predicate: &dyn Fn() -> bool| {
            let context = glib::MainContext::default();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !predicate() && std::time::Instant::now() < deadline {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(predicate(), "Search did not finish");
        };
        state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap()
            .set_search_mode(true);
        state.search.set_text("latest");
        wait(&|| ui::virtual_list::model(&state.list).n_items() == 1);
        state.filter_sender(Some(latest.clone()));
        let ticket = sender_ticket(&state);
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![latest],
            true,
        ));
        assert_eq!(ui::virtual_list::model(&state.thread_list).n_items(), 1);
        state
            .thread_search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap()
            .set_search_mode(true);
        for query in ["needle", "older", "receiver", "copy", "alice"] {
            state.thread_search.set_text(query);
            state
                .thread_search
                .emit_by_name::<()>("search-changed", &[]);
            wait(&|| {
                let groups = ui::virtual_list::groups(&state.thread_list);
                groups.iter().flatten().any(|message| message.uid == 1)
                    && groups.iter().flatten().all(|message| message.uid != 90)
            });
            assert_eq!(state.search.text(), "latest");
            assert_eq!(ui::virtual_list::model(&state.list).n_items(), 1);
            state.thread_search.set_text("missing");
            state
                .thread_search
                .emit_by_name::<()>("search-changed", &[]);
            wait(&|| ui::virtual_list::model(&state.thread_list).n_items() == 0);
        }
        state.search.set_text("");
        state.search.emit_by_name::<()>("search-changed", &[]);
        state.thread_search.set_text("");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        let mut cached = state.messages.borrow()[0].clone();
        cached.body_text.clear();
        cached.body_html = "<p>HTML cache arrival</p>".into();
        cached.cc = "New copy recipient".into();
        state.apply_cached_body(cached);
        state.thread_search.set_text("arrival");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        wait(&|| {
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .any(|message| message.uid == 1)
        });
        state.thread_search.set_text("new copy");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        wait(&|| {
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .flatten()
                .any(|message| message.uid == 1)
        });
        state.thread_search.set_text("needle");
        state
            .thread_search
            .emit_by_name::<()>("search-changed", &[]);
        state.filter_sender(Some(other));
        assert!(state.thread_search.text().is_empty());
        let context = glib::MainContext::default();
        for _ in 0..50 {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(state.sender_pane.borrow().sender(), Some("bob@example.com"));
        assert_eq!(ui::virtual_list::model(&state.thread_list).n_items(), 0);
        state.search.set_text("missing");
        state.search.emit_by_name::<()>("search-changed", &[]);
        state
            .search
            .ancestor(gtk::SearchBar::static_type())
            .and_downcast::<gtk::SearchBar>()
            .unwrap()
            .set_search_mode(false);
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
        for _ in 0..50 {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 2);
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK row recycling"]
    fn repeated_search_sort_and_open_keeps_one_sender_and_rejects_old_pages() {
        let state = unread_test_state("org.example.BrevladaNavigationDiagnostic");
        let window = state
            .list
            .root()
            .unwrap()
            .downcast::<adw::ApplicationWindow>()
            .unwrap();
        let messages: Vec<_> = (1..=80)
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                sender: format!("Sender {uid} <sender{uid}@example.com>"),
                subject: format!("Subject {}", if uid % 2 == 0 { "even" } else { "odd" }),
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages;
        state.render_list();
        window.present();
        let context = gtk::glib::MainContext::default();
        let settle = || {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(50);
            while std::time::Instant::now() < until {
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        for cycle in 0..30 {
            state
                .search
                .set_text(if cycle % 2 == 0 { "even" } else { "odd" });
            state.sender_unread_first.set_unread_first(cycle % 3 == 0);
            state.render_list();
            settle();
            let first = ui::virtual_list::item(&state.list, 0).unwrap().messages[0].clone();
            state.list.emit_by_name::<()>("activate", &[&0u32]);
            let old_ticket = sender_ticket(&state);
            state.list.emit_by_name::<()>("activate", &[&1u32]);
            let second = ui::virtual_list::item(&state.list, 1).unwrap().messages[0].clone();
            let ticket = sender_ticket(&state);
            assert_ne!(old_ticket, ticket);
            state.event(Event::SenderPage(
                state.generation.get(),
                old_ticket,
                vec![first],
                false,
            ));
            assert!(state.sender_pane.borrow().messages().is_empty());
            state.event(Event::SenderPage(
                state.generation.get(),
                ticket,
                vec![second.clone()],
                false,
            ));
            assert_eq!(
                state.sender_pane.borrow().sender(),
                Some(models::senders::key(&second).as_str())
            );
            state.thread_list.emit_by_name::<()>("activate", &[&0u32]);
            assert_eq!(state.open_group.borrow()[0].uid, second.uid);
            let read = Message {
                is_read: true,
                ..second.clone()
            };
            state.event(Event::Body(
                state.generation.get(),
                state.selection.get(),
                read,
            ));
            for message in state.messages.borrow_mut().iter_mut() {
                message.timestamp = -message.timestamp;
            }
            state.render_list();
            settle();
            let mut opened = 0;
            for position in 0..ui::virtual_list::model(&state.list).n_items() {
                let Some(row) = ui::virtual_list::visible_row(&state.list, position) else {
                    continue;
                };
                let data = ui::virtual_list::item(&state.list, position).unwrap();
                assert!(
                    labels(row.clone()).contains(&models::senders::identity(&data.messages[0]).0)
                );
                let active = row.has_css_class("message-row-open");
                assert_eq!(active, data.key == models::senders::key(&second));
                opened += usize::from(active);
            }
            assert!(opened <= 1);
            state.filter_sender(None);
            settle();
            for position in 0..ui::virtual_list::model(&state.list).n_items() {
                if let Some(row) = ui::virtual_list::visible_row(&state.list, position) {
                    assert!(!row.has_css_class("message-row-open"));
                }
            }
        }
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn unread_sort_toggles_are_independent_and_apply_to_older_pages() {
        let state = unread_test_state("org.example.BrevladaUnreadSortDiagnostic");
        let message = |uid, sender: &str, is_read| Message {
            uid,
            timestamp: uid as i64,
            sender: sender.into(),
            subject: format!("Subject {uid}"),
            body_loaded: true,
            is_read,
            ..Default::default()
        };
        let older = message(10, "a@example.com", false);
        let latest = message(40, "a@example.com", true);
        *state.messages.borrow_mut() = vec![
            message(50, "b@example.com", true),
            latest.clone(),
            message(30, "c@example.com", false),
            older.clone(),
        ];
        let sender_uids = || {
            ui::virtual_list::groups(&state.list)
                .iter()
                .map(|g| g[0].uid)
                .collect::<Vec<_>>()
        };
        let thread_uids = || {
            ui::virtual_list::groups(&state.thread_list)
                .iter()
                .map(|g| g[0].uid)
                .collect::<Vec<_>>()
        };
        for toggle in [&state.sender_unread_first, &state.thread_unread_first] {
            assert!(toggle.unread_first());
            assert_eq!(
                toggle.widget.icon_name().as_deref(),
                Some("open-menu-symbolic")
            );
            assert!(
                toggle
                    .widget
                    .ancestor(adw::HeaderBar::static_type())
                    .is_some()
            );
        }
        state.render_list();
        assert_eq!(sender_uids(), vec![40, 30, 50]);
        ui::virtual_list::selection(&state.list).set_selected(0);
        state.sender_unread_first.set_unread_first(false);
        assert_eq!(sender_uids(), vec![50, 40, 30]);
        assert_eq!(ui::virtual_list::selected(&state.list), Some(1));
        assert!(state.thread_unread_first.unread_first());

        state.filter_sender(Some(latest.clone()));
        let ticket = state.sender_pane.borrow_mut().request().unwrap().ticket;
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![latest],
            true,
        ));
        let ticket = state.sender_pane.borrow_mut().request().unwrap().ticket;
        state.event(Event::SenderPage(
            state.generation.get(),
            ticket,
            vec![older],
            false,
        ));
        assert_eq!(thread_uids(), vec![10, 40]);
        ui::virtual_list::selection(&state.thread_list).set_selected(0);
        state.thread_unread_first.set_unread_first(false);
        assert_eq!(thread_uids(), vec![40, 10]);
        assert_eq!(ui::virtual_list::selected(&state.thread_list), Some(1));
        assert_eq!(sender_uids(), vec![50, 40, 30]);

        state.sender_unread_first.set_unread_first(true);
        let mut refreshed = state.messages.borrow().clone();
        refreshed.push(message(5, "d@example.com", false));
        state.event(Event::Messages(state.generation.get(), refreshed, false));
        assert_eq!(sender_uids(), vec![40, 30, 5, 50]);
        assert_eq!(thread_uids(), vec![40, 10]);
        state.sender_unread_first.set_unread_first(false);
        assert_eq!(sender_uids(), vec![50, 40, 30, 5]);
        state.thread_unread_first.set_unread_first(true);
        assert_eq!(thread_uids(), vec![10, 40]);
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn sort_menus_restore_independent_choices_and_search_follows_reload() {
        let directory =
            std::env::temp_dir().join(format!("brevlada-sort-{}", glib::uuid_string_random()));
        let path = directory.join("sorting.json");
        let state = unread_test_state("org.example.BrevladaSortMenuDiagnostic");
        state.remember_sorting(path.clone());
        let search = state
            .refresh
            .next_sibling()
            .unwrap()
            .downcast::<gtk::ToggleButton>()
            .unwrap();
        assert_eq!(
            search.icon_name().as_deref(),
            Some("system-search-symbolic")
        );
        for menu in [&state.sender_unread_first, &state.thread_unread_first] {
            assert!(menu.unread_first());
            let model = menu.widget.menu_model().unwrap();
            assert_eq!(model.n_items(), 1);
            assert_eq!(
                model
                    .item_attribute_value(0, "label", None)
                    .unwrap()
                    .get::<String>()
                    .as_deref(),
                Some("Sort unread first")
            );
            menu.widget
                .activate_action("sort.unread-first", None)
                .unwrap();
            assert!(!menu.unread_first());
        }
        let restored = unread_test_state("org.example.BrevladaRestoredSortDiagnostic");
        restored.remember_sorting(path.clone());
        assert!(!restored.sender_unread_first.unread_first());
        assert!(!restored.thread_unread_first.unread_first());
        restored
            .sender_unread_first
            .widget
            .activate_action("sort.unread-first", None)
            .unwrap();
        let restarted = unread_test_state("org.example.BrevladaRestartedSortDiagnostic");
        restarted.remember_sorting(path);
        assert!(restarted.sender_unread_first.unread_first());
        assert!(!restarted.thread_unread_first.unread_first());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without mail workers"]
    fn opening_cached_and_sender_only_messages_requests_read_updates() {
        let state = unread_test_state("org.example.BrevladaReadDiagnostic");
        let older = Message {
            uid: 1,
            timestamp: 1,
            sender: "sender@example.com".into(),
            subject: "Older".into(),
            body_text: "Cached body".into(),
            body_loaded: true,
            ..Default::default()
        };
        let latest = Message {
            uid: 2,
            timestamp: 2,
            subject: "Latest".into(),
            ..older.clone()
        };
        *state.messages.borrow_mut() = vec![latest.clone()];
        state
            .sender_pane
            .borrow_mut()
            .select(Some(models::senders::key(&older)));
        state
            .sender_pane
            .borrow_mut()
            .seed(vec![latest.clone(), older.clone()]);
        for message in [latest, older] {
            state.show_thread(vec![message.clone()]);
            assert!(state.pending.borrow().contains(&message.uid));
            let read = Message {
                is_read: true,
                ..message.clone()
            };
            state.event(Event::Body(
                state.generation.get(),
                state.selection.get(),
                read,
            ));
            assert!(!state.pending.borrow().contains(&message.uid));
            assert!(state.open_group.borrow()[0].is_read);
            assert!(
                state
                    .sender_pane
                    .borrow()
                    .messages()
                    .iter()
                    .find(|m| m.uid == message.uid)
                    .unwrap()
                    .is_read
            );
            state.render_conversation();
            assert!(!state.pending.borrow().contains(&message.uid));
        }

        // A preview completing after expansion must still trigger the read request.
        let preview = Message {
            uid: 3,
            subject: "Preview".into(),
            ..state.sender_pane.borrow().messages()[1].clone()
        };
        let header = Message {
            is_read: false,
            body_loaded: false,
            body_text: String::new(),
            ..preview.clone()
        };
        let mut messages = state.sender_pane.borrow().messages().to_vec();
        messages.push(header.clone());
        state.sender_pane.borrow_mut().seed(messages);
        state.show_thread(vec![header]);
        state.pending.borrow_mut().clear();
        state.preview_pending.borrow_mut().insert(preview.uid);
        state.event(Event::Preview(
            state.generation.get(),
            state.selection.get(),
            preview.clone(),
        ));
        assert!(!state.preview_pending.borrow().contains(&preview.uid));
        assert!(state.pending.borrow().contains(&preview.uid));
        assert!(!state.open_group.borrow()[0].is_read);
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK hover state"]
    fn opened_row_style_stays_on_opened_sender_when_another_row_is_hovered() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaHoverDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let state = State::new(
            shell,
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        let first = Message {
            uid: 2,
            timestamp: 2,
            sender: "first@example.com".into(),
            ..Default::default()
        };
        let second = Message {
            uid: 1,
            timestamp: 1,
            sender: "second@example.com".into(),
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![first.clone(), second];
        state.render_list();
        assert!(state.list_scroll.is_kinetic_scrolling());
        assert!(state.thread_scroll.is_kinetic_scrolling());
        window.present();
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
        }
        state.filter_sender(Some(first.clone()));
        assert!(ui::virtual_list::selected(&state.list).is_none());
        let opened = ui::virtual_list::visible_row(&state.list, 0).unwrap();
        let hovered = ui::virtual_list::visible_row(&state.list, 1).unwrap();
        assert!(opened.has_css_class("message-row-open"));
        assert!(!hovered.has_css_class("message-row-open"));
        ui::virtual_list::selection(&state.list).set_selected(1);
        hovered.set_state_flags(gtk::StateFlags::PRELIGHT, false);
        assert!(opened.has_css_class("message-row-open"));
        assert!(!hovered.has_css_class("message-row-open"));
        state.messages.borrow_mut()[1].timestamp = 3;
        state.render_list();
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        state.refresh_open_styles();
        assert_eq!(
            ui::virtual_list::item(&state.list, 0)
                .map(|row| row.key)
                .as_deref(),
            Some("second@example.com")
        );
        for position in 0..ui::virtual_list::model(&state.list).n_items() {
            let row = ui::virtual_list::visible_row(&state.list, position).unwrap();
            assert_eq!(
                row.has_css_class("message-row-open"),
                ui::virtual_list::item(&state.list, position).unwrap().key == "first@example.com"
            );
        }
        let older = Message {
            uid: 3,
            timestamp: 0,
            sender: first.sender.clone(),
            subject: "Older".into(),
            ..Default::default()
        };
        state.event(Event::SenderPage(
            state.generation.get(),
            sender_ticket(&state),
            vec![first.clone(), older],
            false,
        ));
        state.show_thread(vec![first]);
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        let opened_thread = ui::virtual_list::visible_row(&state.thread_list, 0).unwrap();
        let hovered_thread = ui::virtual_list::visible_row(&state.thread_list, 1).unwrap();
        ui::virtual_list::selection(&state.thread_list).set_selected(1);
        hovered_thread.set_state_flags(gtk::StateFlags::PRELIGHT, false);
        assert!(opened_thread.has_css_class("message-row-open"));
        assert!(!hovered_thread.has_css_class("message-row-open"));
        state.filter_sender(None);
        for position in 0..ui::virtual_list::model(&state.list).n_items() {
            assert!(
                !ui::virtual_list::visible_row(&state.list, position)
                    .unwrap()
                    .has_css_class("message-row-open")
            );
        }
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK list inspection"]
    fn sender_open_requests_bounded_pages_without_grouping_folder() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSenderPagingDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        let account = Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        *state.account.borrow_mut() = Some(account.clone());
        *state.folder.borrow_mut() = "INBOX".into();
        let messages: Vec<_> = (1..=1000)
            .rev()
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                sender: "sender@example.com".into(),
                subject: format!("Subject {uid}"),
                ..Default::default()
            })
            .collect();
        *state.messages.borrow_mut() = messages.clone();
        state.filter_sender(Some(messages[0].clone()));
        assert!(ui::virtual_list::groups(&state.thread_list).is_empty());
        assert!(state.sender_pane.borrow().messages().is_empty());
        assert!(matches!(
            commands.try_recv().unwrap(),
            Command::LoadSenderPage {
                before: None,
                limit: 25,
                ..
            }
        ));
        let page = messages[..25].to_vec();
        state.event(Event::SenderPage(
            state.generation.get(),
            sender_ticket(&state),
            page,
            true,
        ));
        assert_eq!(state.sender_pane.borrow().messages().len(), 25);
        assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 25);
        assert!(!state.thread_load_more.get_visible());
        state.load_sender_page();
        assert!(matches!(
            commands.try_recv().unwrap(),
            Command::LoadSenderPage {
                before: Some((976, 976)),
                limit: 25,
                ..
            }
        ));
        state.event(Event::SenderPageError(
            state.generation.get(),
            sender_ticket(&state),
            "offline".into(),
        ));
        assert!(state.thread_load_more.get_visible());
        assert_eq!(
            state.thread_load_more.label().as_deref(),
            Some("Retry loading")
        );
        state.select(account, "Archive".into());
        assert!(state.messages.borrow().is_empty());
        assert_eq!(
            state
                .folder_cache
                .borrow()
                .get(&("account@example.com".into(), "INBOX".into()))
                .unwrap()
                .len(),
            1000
        );
        assert!(matches!(commands.try_recv().unwrap(), Command::Load { .. }));
    }

    #[test]
    #[ignore = "Requires a graphical session to measure scrolling during pagination"]
    fn cached_page_keeps_sender_scroll_position() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaPagingDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let state = State::new(
            shell,
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "account@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.folder.borrow_mut() = "INBOX".into();
        *state.messages.borrow_mut() = (1..=150)
            .rev()
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                sender: format!("sender{uid}@example.com"),
                is_read: uid > 50,
                ..Default::default()
            })
            .chain([Message {
                uid: 0,
                timestamp: 0,
                sender: "sender150@example.com".into(),
                ..Default::default()
            }])
            .collect();
        state.render_list();
        window.present();
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let adjustment = state.list_scroll.vadjustment();
        adjustment.set_value(1000.0);
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        let before = adjustment.value();
        let before_keys: Vec<_> = ui::virtual_list::groups(&state.list)
            .iter()
            .map(|group| group[0].uid)
            .collect();
        assert_eq!(before_keys.len(), theme::MESSAGE_LIMIT);
        assert_eq!(before_keys[0], 150);
        assert_eq!(before_keys[1], 50);
        state.load_more();
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        let after_keys: Vec<_> = ui::virtual_list::groups(&state.list)
            .iter()
            .map(|group| group[0].uid)
            .collect();
        assert_eq!(&after_keys[..before_keys.len()], &before_keys);
        assert_eq!(after_keys.len(), 100);
        assert!((adjustment.value() - before).abs() < 1.0);
        state.load_more();
        assert_eq!(ui::virtual_list::model(&state.list).n_items(), 150);
        assert!(!state.has_more_senders.get());
        state.load_more();
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        assert!((adjustment.value() - before).abs() < 1.0);
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn loading_and_refresh_keep_selection_rows_and_cards() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaLoadingDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let requested: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorder = requested.clone();
        let state = State::new(
            shell,
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(move |email| recorder.borrow_mut().push(email.to_owned())),
        );
        let mut sync = models::sync::Status {
            activity: Some(models::sync::Activity {
                description: "Syncing previews",
                folder: "INBOX".into(),
                ..Default::default()
            }),
            active: 2,
            queued: 10,
            ..Default::default()
        };
        state.sync_status.update(&sync);
        let spinner = state
            .sync_status
            .widget
            .first_child()
            .unwrap()
            .downcast::<gtk::Spinner>()
            .unwrap();
        assert!(spinner.is_spinning() && spinner.get_visible());
        assert!(labels(state.sync_status.widget.clone().upcast()).contains(&"Syncing".into()));
        let messages_counter = state.sync_status.widget.last_child().unwrap();
        let folders_counter = messages_counter.prev_sibling().unwrap();
        let accounts_counter = folders_counter.prev_sibling().unwrap();
        assert!(!accounts_counter.get_visible());
        assert!(!folders_counter.get_visible() && !messages_counter.get_visible());
        sync.activity.as_mut().unwrap().progress.accounts = Some(models::sync::Count {
            cached: 1,
            total: 3,
        });
        state.sync_status.update(&sync);
        assert!(accounts_counter.get_visible());
        assert!(labels(accounts_counter.clone()).contains(&"1/3".into()));
        assert_eq!(
            accounts_counter
                .last_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("avatar-default-symbolic")
        );
        sync.activity.as_mut().unwrap().progress.folders = Some(models::sync::Count {
            cached: 3,
            total: 6,
        });
        state.sync_status.update(&sync);
        assert!(folders_counter.get_visible() && !messages_counter.get_visible());
        assert!(labels(folders_counter.clone()).contains(&"3/6".into()));
        sync.activity.as_mut().unwrap().progress.messages = Some(models::sync::Count {
            cached: 15,
            total: 892,
        });
        state.sync_status.update(&sync);
        assert!(messages_counter.get_visible());
        assert!(labels(messages_counter.clone()).contains(&"15/892".into()));
        assert_eq!(
            folders_counter
                .last_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("folder-symbolic")
        );
        assert_eq!(
            messages_counter
                .last_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("mail-unread-symbolic")
        );
        assert!(
            state
                .sync_status
                .widget
                .measure(gtk::Orientation::Horizontal, -1)
                .0
                <= theme::SIDEBAR_WIDTH
        );
        state.sync_status.update(&models::sync::Status {
            last_started: Some(std::time::SystemTime::now()),
            ..Default::default()
        });
        assert!(!spinner.is_spinning() && !spinner.get_visible());
        assert!(!accounts_counter.get_visible());
        assert!(!folders_counter.get_visible() && !messages_counter.get_visible());
        assert!(
            labels(state.sync_status.widget.clone().upcast())
                .contains(&"Last synced: just now".into())
        );
        *state.account.borrow_mut() = Some(Account {
            path: String::new(),
            email: "fixture".into(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: true,
            smtp: None,
        });
        let account = state.account.borrow().clone().unwrap();
        state.event(Event::Accounts(vec![account]));
        let account_row = state.sidebar.first_child().unwrap();
        let expand = account_row
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let folders = state.folder_boxes.borrow()["fixture"].1.clone();
        expand.emit_clicked();
        assert!(folders.get_visible());
        assert_eq!(expand.icon_name().as_deref(), Some("pan-down-symbolic"));
        assert!(folders.has_css_class("folders-loading"));
        state.event(Event::Unread(
            "fixture".into(),
            vec![("Work/Updates".into(), true)],
        ));
        state.event(Event::Folders(
            "fixture".into(),
            vec!["Work/Updates".into()],
        ));
        assert!(folders.get_visible());
        assert!(!folders.has_css_class("folders-loading"));
        let folder_row = folders.first_child().unwrap();
        assert!(folder_row.get_visible());
        assert!(folder_row.has_css_class("folder-item"));
        let folder_button = folder_row
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        folder_button.emit_clicked();
        let nested = folder_row.last_child().unwrap();
        assert!(nested.get_visible());
        // A background scan of an unchanged folder list must preserve expansion.
        state.event(Event::Folders(
            "fixture".into(),
            vec!["Work/Updates".into()],
        ));
        assert_eq!(folders.first_child().unwrap(), folder_row);
        assert!(nested.get_visible());
        expand.emit_clicked();
        assert!(!folders.get_visible());
        state.event(Event::UnreadSnapshot(
            "fixture".into(),
            vec![("Work/Updates".into(), false)],
        ));
        assert!(!folders.get_visible());
        expand.emit_clicked();
        assert!(folders.get_visible());
        assert_eq!(folders.first_child().unwrap(), folder_row);
        let account = state.account.borrow().clone().unwrap();
        state.event(Event::Accounts(vec![account]));
        let restored_folders = state.folder_boxes.borrow()["fixture"].1.clone();
        assert!(restored_folders.get_visible());
        assert!(restored_folders.has_css_class("folders-loading"));
        state.event(Event::Folders(
            "fixture".into(),
            vec!["Work/Updates".into(), "Work/Projects".into()],
        ));
        let restored_work = restored_folders.first_child().unwrap();
        assert!(restored_work.last_child().unwrap().get_visible());
        assert!(state.expansion.for_account("fixture").is_expanded(""));
        assert!(state.expansion.for_account("fixture").is_expanded("Work"));
        let first = Message {
            uid: 1,
            subject: "First".into(),
            sender: "Ada Lovelace <Ada@Example.com>".into(),
            timestamp: 2,
            ..Default::default()
        };
        let second = Message {
            uid: 2,
            subject: "Second".into(),
            sender: "Ada Lovelace <ada@example.com>".into(),
            timestamp: 1,
            is_read: true,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![first.clone(), second.clone()];
        state.render_list();
        assert_eq!(ui::virtual_list::groups(&state.list).len(), 1);
        assert!(
            labels(ui::virtual_list::visible_row(&state.list, 0).unwrap())
                .contains(&"First".into())
        );
        assert!(!state.back.get_visible());
        // The sidebar asked for the account's own avatar when the account
        // appeared, and the sender list then asks for the sender's once,
        // whatever casing the individual messages used.
        assert_eq!(
            *requested.borrow(),
            vec!["fixture".to_string(), "ada@example.com".to_string()]
        );
        let sender_avatar = || {
            ui::virtual_list::visible_row(&state.list, 0)
                .unwrap()
                .first_child()
                .unwrap()
                .downcast::<adw::Avatar>()
                .unwrap()
        };
        assert!(sender_avatar().custom_image().is_none());
        state.event(Event::Avatar(
            "ada@example.com".into(),
            Some(ui::avatars::PIXEL.to_vec()),
        ));
        assert!(sender_avatar().custom_image().is_some());
        // Rebuilding the row on a refresh keeps the avatar without asking again.
        state.render_list();
        assert!(sender_avatar().custom_image().is_some());
        assert_eq!(requested.borrow().len(), 2);
        // The account row in the sidebar is filled from the same cache.
        let account_avatar = state
            .sidebar
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .last_child()
            .unwrap()
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<adw::Avatar>()
            .unwrap();
        assert!(account_avatar.custom_image().is_none());
        state.event(Event::Avatar(
            "fixture".into(),
            Some(ui::avatars::PIXEL.to_vec()),
        ));
        assert!(account_avatar.custom_image().is_some());
        let sender_row = ui::virtual_list::groups(&state.list)[0][0].sender.clone();
        state.account_sidebar.set_visible(true);
        state.filter_sender(Some(first.clone()));
        assert!(state.thread_sidebar.get_visible());
        assert!(!state.account_sidebar.get_visible());
        assert_eq!(
            ui::virtual_list::groups(&state.list)[0][0].sender,
            sender_row
        );
        assert_eq!(ui::virtual_list::groups(&state.list).len(), 1);
        state.filter_sender(None);
        assert!(state.account_sidebar.get_visible());
        state.account_sidebar.set_visible(false);
        state.filter_sender(Some(first.clone()));
        state.filter_sender(None);
        assert!(!state.account_sidebar.get_visible());
        state.account_sidebar.set_visible(true);
        state.filter_sender(Some(first.clone()));
        state.account_sidebar.set_visible(true);
        state.account_sidebar.set_visible(false);
        state.filter_sender(None);
        assert!(!state.account_sidebar.get_visible());
        state.account_sidebar.set_visible(true);
        state.filter_sender(Some(first.clone()));
        state.account_sidebar.set_visible(true);
        state.filter_sender(Some(second.clone()));
        assert!(state.account_sidebar.get_visible());
        state.filter_sender(None);
        assert!(!state.thread_sidebar.get_visible());
        assert!(state.account_sidebar.get_visible());
        state.filter_sender(Some(first.clone()));
        assert!(!state.account_sidebar.get_visible());
        let compose_header = state
            .compose_button
            .ancestor(adw::HeaderBar::static_type())
            .unwrap();
        assert!(compose_header.has_css_class("content-header"));
        state.compose_button.emit_clicked();
        let compose = state
            .open_composers
            .borrow()
            .last()
            .unwrap()
            .compose
            .clone();
        assert!(compose.widget.get_visible());
        let header = compose
            .widget
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap();
        let receiver = header
            .first_child()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        assert_eq!(receiver.text(), "ada@example.com");
        receiver.set_text("edited@example.com");
        state.compose_button.emit_clicked();
        let second_compose = state
            .open_composers
            .borrow()
            .last()
            .unwrap()
            .compose
            .clone();
        assert!(!Rc::ptr_eq(&compose, &second_compose));
        assert_eq!(state.open_composers.borrow().len(), 2);
        assert_eq!(receiver.text(), "edited@example.com");
        let cancel = header
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let send = cancel
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(cancel.icon_name().is_some());
        assert!(cancel.label().is_none());
        assert!(!send.is_sensitive());
        assert!(labels(send.clone().upcast()).contains(&"Send".into()));
        let subject = header
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        let body_header = subject.next_sibling().unwrap();
        let modes = body_header.first_child().unwrap();
        let formatting = modes.next_sibling().unwrap();
        let group = body_header
            .next_sibling()
            .unwrap()
            .downcast::<adw::PreferencesGroup>()
            .unwrap();
        fn find_editor(widget: gtk::Widget) -> Option<gtk::TextView> {
            if let Ok(editor) = widget.clone().downcast::<gtk::TextView>() {
                return Some(editor);
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(editor) = find_editor(widget.clone()) {
                    return Some(editor);
                }
                child = widget.next_sibling();
            }
            None
        }
        let editor = find_editor(group.clone().upcast()).unwrap();
        subject.set_text("Subject");
        assert!(!send.is_sensitive());
        editor.buffer().set_text("  ");
        assert!(!send.is_sensitive());
        editor.buffer().set_text("Hello");
        assert!(send.is_sensitive());
        let html = modes
            .last_child()
            .unwrap()
            .downcast::<gtk::ToggleButton>()
            .unwrap();
        html.set_active(true);
        assert!(formatting.get_visible());
        assert!(!editor.is_monospace());
        editor.buffer().set_text("Hello");
        assert!(send.is_sensitive());
        subject.set_text("");
        assert!(!send.is_sensitive());
        subject.set_text("Subject");
        assert!(send.is_sensitive());
        send.emit_clicked();
        assert!(compose.widget.get_visible());
        assert_eq!(receiver.text(), "edited@example.com");
        cancel.emit_clicked();
        assert!(!compose.widget.get_visible());
        assert!(receiver.text().is_empty());
        assert!(second_compose.widget.get_visible());
        state.compose_button.emit_clicked();
        let third_compose = state
            .open_composers
            .borrow()
            .last()
            .unwrap()
            .compose
            .clone();
        assert_eq!(
            third_compose
                .widget
                .first_child()
                .unwrap()
                .first_child()
                .unwrap()
                .downcast::<gtk::Entry>()
                .unwrap()
                .text(),
            "ada@example.com"
        );
        state.event(Event::SenderPage(
            state.generation.get(),
            sender_ticket(&state),
            vec![first.clone(), second.clone()],
            false,
        ));
        assert_eq!(ui::virtual_list::groups(&state.thread_list).len(), 2);
        assert!(state.preview_pending.borrow().contains(&first.uid));
        let preview = Message {
            body_loaded: true,
            body_text: "Unread preview".into(),
            ..first.clone()
        };
        state.event(Event::Preview(
            state.generation.get(),
            state.selection.get(),
            preview,
        ));
        assert!(
            !state
                .messages
                .borrow()
                .iter()
                .find(|m| m.uid == first.uid)
                .unwrap()
                .is_read
        );
        assert!(!state.preview_pending.borrow().contains(&first.uid));
        assert!(
            labels(ui::virtual_list::visible_row(&state.thread_list, 0).unwrap())
                .contains(&"Unread preview".into())
        );
        let mut refreshed = state.messages.borrow().clone();
        refreshed
            .iter_mut()
            .find(|m| m.uid == first.uid)
            .unwrap()
            .is_read = true;
        refreshed
            .iter_mut()
            .find(|m| m.uid == second.uid)
            .unwrap()
            .is_read = false;
        state.event(Event::Messages(state.generation.get(), refreshed, false));
        assert_eq!(
            ui::virtual_list::groups(&state.thread_list)[0][0].uid,
            second.uid
        );
        assert!(state.back.get_visible());
        state.filter_sender(None);
        assert_eq!(ui::virtual_list::groups(&state.list).len(), 1);
        assert!(state.selected.borrow().is_empty());
        assert!(state.cards.borrow().is_empty());
        state.filter_sender(Some(first.clone()));
        let sender_snapshot = state.messages.borrow().clone();
        state.event(Event::SenderPage(
            state.generation.get(),
            sender_ticket(&state),
            sender_snapshot,
            false,
        ));
        ui::virtual_list::selection(&state.thread_list).set_selected(1);
        state.show_thread(vec![second.clone()]);
        let card = state.cards.borrow().get(&2).unwrap().widget.clone();
        state
            .thread_scroll
            .vadjustment()
            .configure(200.0, 0.0, 1500.0, 10.0, 100.0, 400.0);
        let generation = state.generation.get();
        let selection = state.selection.get();
        state.load();
        assert_eq!(state.generation.get(), generation);
        assert_eq!(state.selection.get(), selection);
        let loaded = Message {
            body_loaded: true,
            body_text: "Fetched body".into(),
            ..second.clone()
        };
        state.event(Event::Body(generation, selection, loaded.clone()));
        state.event(Event::Body(
            generation,
            selection,
            Message {
                is_read: true,
                ..loaded.clone()
            },
        ));
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, card);
        assert_eq!(
            ui::virtual_list::groups(&state.thread_list)
                [ui::virtual_list::selected(&state.thread_list).unwrap() as usize][0]
                .uid,
            1
        );
        assert_eq!(state.thread_scroll.vadjustment().value(), 200.0);
        state.event(Event::Messages(generation, vec![first, second], false));
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, card);
        assert_eq!(
            ui::virtual_list::groups(&state.thread_list)
                [ui::virtual_list::selected(&state.thread_list).unwrap() as usize][0]
                .uid,
            1
        );
        assert!(
            state
                .messages
                .borrow()
                .iter()
                .find(|m| m.uid == 2)
                .unwrap()
                .body_loaded
        );
        state.pending.borrow_mut().insert(2);
        state.filter_sender(None);
        assert!(!state.back.get_visible());
        assert_eq!(ui::virtual_list::groups(&state.list).len(), 1);
        assert_eq!(*state.selected.borrow(), vec![2]);
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, card);
        assert_eq!(state.viewer.first_child().unwrap(), card);
        assert_eq!(state.selection.get(), selection);
        assert!(state.pending.borrow().contains(&2));
        state.event(Event::BodyError(generation, selection, 2, "Timeout".into()));
        assert!(!state.pending.borrow().contains(&2));
        state.show_thread(vec![loaded]);
        let current = state.cards.borrow().get(&2).unwrap().widget.clone();
        state.event(Event::Body(
            generation,
            selection,
            Message {
                uid: 2,
                body_loaded: true,
                body_text: "Stale".into(),
                ..Default::default()
            },
        ));
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, current);
        let cached = Message {
            uid: 2,
            body_loaded: true,
            body_text: "Background cache".into(),
            ..Default::default()
        };
        let folder = state.folder.borrow().clone();
        state.event(Event::CacheBody(
            "another-account".into(),
            folder.clone(),
            cached.clone(),
        ));
        assert_ne!(
            state
                .messages
                .borrow()
                .iter()
                .find(|m| m.uid == 2)
                .unwrap()
                .body_text,
            "Background cache"
        );
        let was_read = state
            .messages
            .borrow()
            .iter()
            .find(|m| m.uid == 2)
            .unwrap()
            .is_read;
        state.event(Event::CacheBody("fixture".into(), folder, cached));
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, current);
        let messages = state.messages.borrow();
        let message = messages.iter().find(|m| m.uid == 2).unwrap();
        assert_eq!(message.body_text, "Background cache");
        assert_eq!(message.is_read, was_read);
        drop(messages);
        // Returning to a folder renders its snapshot before the worker responds.
        let account = state.account.borrow().clone().unwrap();
        let folder = state.folder.borrow().clone();
        let snapshot = state.messages.borrow().clone();
        state.event(Event::Messages(
            state.generation.get(),
            snapshot.clone(),
            false,
        ));
        state.select(account.clone(), "Archive".into());
        state.event(Event::Messages(state.generation.get(), Vec::new(), false));
        state.select(account.clone(), folder.clone());
        assert_eq!(*state.messages.borrow(), snapshot);
        assert_eq!(
            state.list_stack.visible_child_name().as_deref(),
            Some("list")
        );
        assert!(state.loading.get());
        assert!(!state.refresh.is_sensitive());
        let stale_generation = state.generation.get();
        state.select(account.clone(), "Archive".into());
        assert!(state.messages.borrow().is_empty());
        assert!(state.loading.get());
        assert!(
            labels(state.list_stack.clone().upcast())
                .contains(&"No messages in this folder".into())
        );
        state.event(Event::Messages(stale_generation, snapshot.clone(), false));
        assert!(state.messages.borrow().is_empty());
        // Background updates to an inactive folder are available on return.
        state.event(Event::CacheList(
            account.email.clone(),
            folder.clone(),
            Vec::new(),
        ));
        state.select(account.clone(), folder.clone());
        assert!(state.messages.borrow().is_empty());
        state.event(Event::Messages(state.generation.get(), snapshot, false));
        assert!(!state.messages.borrow().is_empty());
        let mut other_account = account;
        other_account.email = "other-fixture".into();
        state.select(other_account, folder);
        assert!(!state.has_cached_folder());
        assert!(state.messages.borrow().is_empty());
        assert!(!window.is_visible());
        window.close();
    }

    fn labels(widget: gtk::Widget) -> Vec<String> {
        let mut result = Vec::new();
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            result.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            result.extend(labels(widget));
        }
        result
    }
}
