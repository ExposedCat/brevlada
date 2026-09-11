mod actions;
mod events;
mod previews;
mod rendering;
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
    account: RefCell<Option<Account>>,
    folder: RefCell<String>,
    generation: Cell<u64>,
    selection: Cell<u64>,
    pending: RefCell<HashSet<u32>>,
    preview_pending: RefCell<HashSet<u32>>,
    loading: Cell<bool>,
    rendering: Cell<bool>,
    messages: RefCell<Vec<Message>>,
    groups: RefCell<Vec<Vec<Message>>>,
    selected_sender: RefCell<Option<String>>,
    selected: RefCell<Vec<u32>>,
    cards: RefCell<HashMap<u32, ui::viewer::Card>>,
    sidebar: gtk::Box,
    sync_status: ui::sync_status::SyncStatus,
    folder_boxes: RefCell<HashMap<String, (Account, gtk::Box)>>,
    folder_names: RefCell<HashMap<String, Vec<String>>>,
    unread: RefCell<HashMap<String, ui::sidebar::Unread>>,
    list: gtk::ListBox,
    list_scroll: gtk::ScrolledWindow,
    viewer_scroll: gtk::ScrolledWindow,
    viewer: gtk::Box,
    list_stack: gtk::Stack,
    toast: adw::ToastOverlay,
    navigation_selection: ui::sidebar::Selection,
    expansion: ui::expansion::Expansion,
    avatars: Rc<ui::avatars::Avatars>,
    compose_button: gtk::Button,
    compose: ui::compose::Compose,
    refresh: gtk::Button,
    sync: gtk::Button,
    back: gtk::Button,
    search: gtk::SearchEntry,
}

pub fn create(app: &adw::Application) {
    let shell = ui::shell::Shell::new(app);
    let window = shell.window.clone();
    let (sender, events) = worker::start(glib::user_data_dir().join("brevlada/emails.db"));
    let queue = sender.avatars();
    let avatars = ui::avatars::Avatars::new(move |email| queue.push(email.to_owned()));
    let state = State::new(shell, sender, ui::expansion::Expansion::load(), avatars);
    let weak = Rc::downgrade(&state);
    state.back.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.filter_sender(None);
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
    state.search.connect_search_changed(move |_| {
        if let Some(state) = weak.upgrade() {
            state.render_list();
        }
    });
    let weak = Rc::downgrade(&state);
    state.list.connect_row_selected(move |_, row| {
        if let (Some(state), Some(row)) = (weak.upgrade(), row) {
            if state.rendering.get() {
                return;
            }
            let group = state.groups.borrow().get(row.index() as usize).cloned();
            if let Some(group) = group {
                if state.selected_sender.borrow().is_none() {
                    state.filter_sender(group.first().cloned());
                } else {
                    state.show_thread(group);
                }
            }
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
    fn new(
        shell: ui::shell::Shell,
        sender: worker::Worker,
        expansion: ui::expansion::Expansion,
        avatars: Rc<ui::avatars::Avatars>,
    ) -> Rc<Self> {
        let ui::shell::Shell {
            window: _,
            sidebar,
            sync_status,
            list,
            list_scroll,
            viewer_scroll,
            list_stack,
            viewer,
            compose_button,
            compose,
            refresh,
            sync,
            back,
            search,
            toast,
        } = shell;
        let state = Rc::new(State {
            sender,
            account: RefCell::new(None),
            folder: RefCell::new(String::new()),
            generation: Cell::new(0),
            selection: Cell::new(0),
            pending: RefCell::new(HashSet::new()),
            preview_pending: RefCell::new(HashSet::new()),
            loading: Cell::new(false),
            rendering: Cell::new(false),
            messages: RefCell::new(Vec::new()),
            groups: RefCell::new(Vec::new()),
            selected_sender: RefCell::new(None),
            selected: RefCell::new(Vec::new()),
            cards: RefCell::new(HashMap::new()),
            sidebar,
            sync_status,
            folder_boxes: RefCell::new(HashMap::new()),
            folder_names: RefCell::new(HashMap::new()),
            unread: RefCell::new(HashMap::new()),
            list,
            list_scroll,
            viewer_scroll,
            viewer,
            list_stack,
            toast,
            navigation_selection: ui::sidebar::Selection::default(),
            expansion,
            avatars,
            compose_button,
            compose,
            refresh,
            sync,
            back,
            search,
        });
        let weak = Rc::downgrade(&state);
        state.compose_button.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                state.compose.show(
                    state
                        .selected_sender
                        .borrow()
                        .as_deref()
                        .unwrap_or_default(),
                );
                let adjustment = state.viewer_scroll.vadjustment();
                adjustment.set_value(adjustment.lower());
            }
        });
        state
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

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
        assert_eq!(state.groups.borrow().len(), 1);
        assert!(labels(state.list.row_at_index(0).unwrap().upcast()).contains(&"First".into()));
        assert!(!state.back.get_visible());
        // The sidebar asked for the account's own avatar when the account
        // appeared, and the sender list then asks for the sender's once,
        // whatever casing the individual messages used.
        assert_eq!(
            *requested.borrow(),
            vec!["fixture".to_string(), "ada@example.com".to_string()]
        );
        let sender_avatar = || {
            state
                .list
                .row_at_index(0)
                .unwrap()
                .child()
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
        state.filter_sender(Some(first.clone()));
        let compose_header = state
            .compose_button
            .ancestor(adw::HeaderBar::static_type())
            .unwrap();
        assert!(compose_header.has_css_class("content-header"));
        state.compose_button.emit_clicked();
        assert!(state.compose.widget.get_visible());
        let header = state.compose.widget.first_child().unwrap();
        let receiver = header
            .first_child()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        assert_eq!(receiver.text(), "ada@example.com");
        receiver.set_text("edited@example.com");
        state.compose_button.emit_clicked();
        assert_eq!(receiver.text(), "edited@example.com");
        let cancel = header
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let draft = cancel
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let send = draft
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        for button in [&draft, &cancel] {
            assert!(button.icon_name().is_some());
            assert!(button.label().is_none());
        }
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
        draft.emit_clicked();
        assert!(state.compose.widget.get_visible());
        assert_eq!(receiver.text(), "edited@example.com");
        cancel.emit_clicked();
        assert!(!state.compose.widget.get_visible());
        assert!(receiver.text().is_empty());
        state.compose_button.emit_clicked();
        assert_eq!(receiver.text(), "ada@example.com");
        cancel.emit_clicked();
        assert_eq!(state.groups.borrow().len(), 2);
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
            labels(state.list.row_at_index(0).unwrap().upcast()).contains(&"Unread preview".into())
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
        assert_eq!(state.groups.borrow()[0][0].uid, second.uid);
        assert!(state.back.get_visible());
        state.filter_sender(None);
        assert_eq!(state.groups.borrow().len(), 1);
        assert!(state.selected.borrow().is_empty());
        assert!(state.cards.borrow().is_empty());
        state.filter_sender(Some(first.clone()));
        let row = state.list.row_at_index(1).unwrap();
        state.list.select_row(Some(&row));
        state.show_thread(vec![second.clone()]);
        let card = state.cards.borrow().get(&2).unwrap().widget.clone();
        state
            .list_scroll
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
        assert_eq!(state.list.selected_row().unwrap(), row);
        assert_eq!(state.list_scroll.vadjustment().value(), 200.0);
        state.event(Event::Messages(generation, vec![first, second], false));
        assert_eq!(state.cards.borrow().get(&2).unwrap().widget, card);
        assert_eq!(state.list.selected_row().unwrap(), row);
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
        assert_eq!(state.groups.borrow().len(), 1);
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
