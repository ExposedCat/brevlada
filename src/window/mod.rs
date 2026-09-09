mod actions;
mod events;
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
    visited: RefCell<HashMap<String, Account>>,
    generation: Cell<u64>,
    selection: Cell<u64>,
    pending: RefCell<HashSet<u32>>,
    loading: Cell<bool>,
    rendering: Cell<bool>,
    messages: RefCell<Vec<Message>>,
    groups: RefCell<Vec<Vec<Message>>>,
    selected: RefCell<Vec<u32>>,
    cards: RefCell<HashMap<u32, ui::viewer::Card>>,
    sidebar: gtk::Box,
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
    refresh: gtk::Button,
    search: gtk::SearchEntry,
    list_title: gtk::Label,
    content_title: adw::WindowTitle,
}

pub fn create(app: &adw::Application) {
    let shell = ui::shell::Shell::new(app);
    let window = shell.window.clone();
    let (sender, events) = worker::start(glib::user_data_dir().join("brevlada/emails.db"));
    let state = State::new(shell, sender, ui::expansion::Expansion::load());
    let weak = Rc::downgrade(&state);
    state.refresh.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            state.load();
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
                state.show_thread(group);
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
            for account in state.visited.borrow().values() {
                let current = state.account.borrow();
                if state.folder.borrow().as_str() != "INBOX"
                    || current.as_ref().is_none_or(|a| a.email != account.email)
                {
                    state.send(Command::SyncInbox(account.clone()));
                }
            }
            for (account, _) in state.folder_boxes.borrow().values() {
                state.send(Command::Unread(account.clone()));
            }
        }
        glib::ControlFlow::Continue
    });
    let owner = RefCell::new(Some(state.clone()));
    let timer = RefCell::new(Some(timer));
    window.connect_close_request(move |_| {
        owner.borrow_mut().take();
        if let Some(timer) = timer.borrow_mut().take() {
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
    ) -> Rc<Self> {
        let ui::shell::Shell {
            window: _,
            sidebar,
            list,
            list_scroll,
            viewer_scroll,
            list_stack,
            viewer,
            refresh,
            search,
            list_title,
            content_title,
            toast,
        } = shell;
        Rc::new(State {
            sender,
            account: RefCell::new(None),
            folder: RefCell::new(String::new()),
            visited: RefCell::new(HashMap::new()),
            generation: Cell::new(0),
            selection: Cell::new(0),
            pending: RefCell::new(HashSet::new()),
            loading: Cell::new(false),
            rendering: Cell::new(false),
            messages: RefCell::new(Vec::new()),
            groups: RefCell::new(Vec::new()),
            selected: RefCell::new(Vec::new()),
            cards: RefCell::new(HashMap::new()),
            sidebar,
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
            refresh,
            search,
            list_title,
            content_title,
        })
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
        let state = State::new(
            shell,
            worker::Worker::disconnected(),
            ui::expansion::Expansion::default(),
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
            timestamp: 2,
            ..Default::default()
        };
        let second = Message {
            uid: 2,
            subject: "Second".into(),
            timestamp: 1,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![first.clone(), second.clone()];
        state.render_list();
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
        assert!(!window.is_visible());
        window.close();
    }
}
