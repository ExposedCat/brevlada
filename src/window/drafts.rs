use super::*;
use models::{
    Draft, SentMessage,
    draft::{Outcome, Target},
};

pub(super) type Key = (String, String, String);

pub(super) struct Editor {
    pub widget: gtk::Box,
    pub compose: Rc<ui::compose::Compose>,
    account: Account,
    source: RefCell<SentMessage>,
    pub(super) loaded: Cell<bool>,
    confirming: Cell<bool>,
    status: gtk::Box,
    status_label: gtk::Label,
    retry: gtk::Button,
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn pump() {
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
    }

    fn command(commands: &std::sync::mpsc::Receiver<Command>) -> Command {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            pump();
            match commands.try_recv() {
                Ok(command) => return command,
                Err(std::sync::mpsc::TryRecvError::Empty)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("Expected composer command: {error}"),
            }
        }
    }

    fn subject(compose: &ui::compose::Compose) -> gtk::Entry {
        compose
            .widget
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast()
            .unwrap()
    }

    fn action(compose: &ui::compose::Compose, index_from_end: usize) -> gtk::Button {
        let mut widget = compose.widget.first_child().unwrap().last_child().unwrap();
        for _ in 0..index_from_end {
            widget = widget.prev_sibling().unwrap();
        }
        widget.downcast().unwrap()
    }

    fn body(widget: &gtk::Widget) -> Option<gtk::TextView> {
        if let Some(view) = widget.downcast_ref::<gtk::TextView>() {
            return Some(view.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(view) = body(&widget) {
                return Some(view);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn draft(id: &str, uid: u32) -> SentMessage {
        SentMessage {
            folder: "Drafts".into(),
            message: Message {
                uid,
                uid_validity: Some(7),
                message_id: id.into(),
                subject: "Re: Topic".into(),
                recipients: "other@example.com".into(),
                cc: "copy@example.com".into(),
                in_reply_to: Some("incoming".into()),
                references: vec!["incoming".into()],
                is_draft: true,
                is_read: true,
                body_loaded: true,
                body_text: format!("Body {id}"),
                ..Default::default()
            },
        }
    }

    fn dialog() -> adw::MessageDialog {
        gtk::Window::list_toplevels()
            .into_iter()
            .filter(|widget| widget.is_visible())
            .find_map(|widget| widget.downcast::<adw::MessageDialog>().ok())
            .unwrap()
    }

    #[test]
    #[ignore = "Requires a graphical session for editable drafts and deletion dialogs"]
    fn drafts_update_immediately_keep_independent_edits_and_confirm_targeted_deletion() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaEditableDraftDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let (worker, commands) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        *state.account.borrow_mut() = Some(Account {
            email: "me@example.com".into(),
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
        let incoming = Message {
            uid: 1,
            message_id: "incoming".into(),
            subject: "Topic".into(),
            sender: "other@example.com".into(),
            is_read: true,
            body_loaded: true,
            ..Default::default()
        };
        *state.messages.borrow_mut() = vec![incoming.clone()];
        state.show_thread(vec![incoming]);
        let incoming_card = state.cards.borrow()[&1].widget.clone();
        while commands.try_recv().is_ok() {}
        state.compose.show("other@example.com");
        subject(&state.compose).set_text("New subject");
        body(&state.compose.widget.clone().upcast())
            .unwrap()
            .buffer()
            .set_text("New draft body");
        action(&state.compose, 1).emit_clicked();
        pump();
        let Command::Compose {
            request,
            draft: submitted,
            save: true,
            target: None,
            ..
        } = command(&commands)
        else {
            panic!("Expected new draft")
        };
        assert!(submitted.references.contains(&"incoming".into()));
        let first = draft("first", 10);
        state.event(Event::Composed(
            request,
            Ok(Outcome::Saved(models::draft::Saved {
                folder: first.folder.clone(),
                message: first.message.clone(),
                cleanup_error: None,
            })),
        ));
        let first_editor =
            state.draft_editors.borrow()[&state.draft_key("Drafts", "first")].clone();
        assert_eq!(
            first_editor.widget.parent(),
            Some(state.viewer.clone().upcast())
        );
        assert!(!state.compose.widget.get_visible());
        assert_eq!(state.cards.borrow()[&1].widget, incoming_card);
        assert!(
            commands.try_recv().is_err(),
            "Saving a draft must not reload existing messages"
        );
        assert_eq!(subject(&first_editor.compose).text(), "Re: Topic");

        let mut second = draft("second", 11);
        second.message.body_html = "<p><b>Formatted draft</b></p>".into();
        let mut headers_only = second.clone();
        headers_only.message.body_loaded = false;
        headers_only.message.body_text.clear();
        headers_only.message.body_html.clear();
        state.event(Event::RelatedSent(
            state.generation.get(),
            state.selection.get(),
            vec![first.clone(), headers_only],
        ));
        assert_eq!(state.cards.borrow()[&1].widget, incoming_card);
        assert!(matches!(
            command(&commands),
            Command::SentBody { uid: 11, .. }
        ));
        assert!(
            commands.try_recv().is_err(),
            "Only the uncached draft should load"
        );
        let loading_box = state.draft_editors.borrow()[&state.draft_key("Drafts", "second")]
            .widget
            .clone();
        state.event(Event::SentBody(
            state.generation.get(),
            state.selection.get(),
            "Drafts".into(),
            11,
            Ok(second.message.clone()),
        ));
        assert_eq!(
            state.draft_editors.borrow()[&state.draft_key("Drafts", "second")].widget,
            loading_box
        );
        assert_eq!(state.cards.borrow()[&1].widget, incoming_card);
        assert!(
            commands.try_recv().is_err(),
            "Loading a draft must not reload the thread"
        );
        let second_editor =
            state.draft_editors.borrow()[&state.draft_key("Drafts", "second")].clone();
        assert_eq!(
            second_editor.widget.parent(),
            Some(state.viewer.clone().upcast())
        );
        let cc = second_editor
            .compose
            .widget
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        assert_eq!(cc.text(), "copy@example.com");
        subject(&first_editor.compose).set_text("First edited");
        subject(&second_editor.compose).set_text("Second edited");
        state.event(Event::RelatedSent(
            state.generation.get(),
            state.selection.get(),
            vec![first.clone(), second.clone()],
        ));
        assert_eq!(subject(&first_editor.compose).text(), "First edited");
        assert_eq!(subject(&second_editor.compose).text(), "Second edited");
        action(&first_editor.compose, 1).emit_clicked();
        action(&second_editor.compose, 1).emit_clicked();
        pump();
        let Command::Compose {
            request: first_request,
            draft: first_submit,
            target: Some(first_target),
            ..
        } = command(&commands)
        else {
            panic!("Expected first save")
        };
        let Command::Compose {
            request: second_request,
            draft: second_submit,
            target: Some(second_target),
            ..
        } = command(&commands)
        else {
            panic!("Expected second save")
        };
        assert_eq!(first_target.uid, 10);
        assert_eq!(second_target.uid, 11);
        assert_eq!(first_submit.subject, "First edited");
        assert_eq!(second_submit.subject, "Second edited");
        assert_eq!(
            second_submit.html.as_deref(),
            Some("<p><b>Formatted draft</b></p>")
        );
        assert_eq!(second_submit.text.trim(), "Formatted draft");
        state.event(Event::Composed(
            second_request,
            Err("Quota exceeded".into()),
        ));
        assert!(second_editor.compose.widget.is_sensitive());
        assert!(!first_editor.compose.widget.is_sensitive());
        let mut replacement = draft("replacement", 12);
        replacement.message.subject = first_submit.subject;
        state.event(Event::Composed(
            first_request,
            Ok(Outcome::Saved(models::draft::Saved {
                folder: replacement.folder.clone(),
                message: replacement.message.clone(),
                cleanup_error: None,
            })),
        ));
        assert!(Rc::ptr_eq(
            &first_editor,
            &state.draft_editors.borrow()[&state.draft_key("Drafts", "replacement")]
        ));
        assert_eq!(state.cards.borrow()[&1].widget, incoming_card);
        assert_eq!(first_editor.target().uid, 12);
        assert_eq!(subject(&first_editor.compose).text(), "First edited");

        action(&second_editor.compose, 0).emit_clicked();
        assert_eq!(
            dialog().heading().as_deref(),
            Some("Are you sure you want to delete this draft?")
        );
        dialog().response("cancel");
        assert!(commands.try_recv().is_err());
        assert!(second_editor.widget.parent().is_some());
        action(&second_editor.compose, 0).emit_clicked();
        dialog().response("delete");
        let Command::DeleteDraft {
            request: delete_request,
            target,
            ..
        } = command(&commands)
        else {
            panic!("Expected deletion")
        };
        assert_eq!(target.uid, 11);
        assert_eq!(state.cards.borrow()[&1].widget, incoming_card);
        assert!(
            second_editor.widget.parent().is_none(),
            "Confirmed deletion must disappear before the server responds"
        );
        state.event(Event::RelatedSent(
            state.generation.get(),
            state.selection.get(),
            vec![first.clone(), second.clone()],
        ));
        assert!(second_editor.widget.parent().is_none());
        assert!(first_editor.widget.parent().is_some());
        state.event(Event::DraftDeleted(delete_request, Err("Offline".into())));
        assert!(second_editor.widget.parent().is_some());
        assert_eq!(subject(&second_editor.compose).text(), "Second edited");
        action(&second_editor.compose, 0).emit_clicked();
        dialog().response("delete");
        let Command::DeleteDraft { request, .. } = command(&commands) else {
            panic!("Expected retry")
        };
        state.event(Event::DraftDeleted(request, Ok(())));
        assert!(second_editor.widget.parent().is_none());

        action(&first_editor.compose, 2).emit_clicked();
        pump();
        let Command::Compose {
            request,
            save: false,
            target: Some(target),
            ..
        } = command(&commands)
        else {
            panic!("Expected send")
        };
        assert_eq!(target.message_id, "replacement");
        state.event(Event::Composed(
            request,
            Ok(Outcome::Sent(
                Message {
                    message_id: "sent".into(),
                    body_loaded: true,
                    is_read: true,
                    ..Default::default()
                },
                None,
            )),
        ));
        assert!(first_editor.widget.parent().is_none());
        assert!(state.sent_cards.borrow().contains_key("sent"));
        let mut late = draft("late", 20);
        late.message.body_html = "<p>Late formatted draft</p>".into();
        state.related_sent.borrow_mut().push(late);
        state.render_conversation();
        let late_editor = state.draft_editors.borrow()[&state.draft_key("Drafts", "late")].clone();
        action(&late_editor.compose, 1).emit_clicked();
        // The async HTML read must keep the account and thread from the click.
        state.account.borrow_mut().as_mut().unwrap().email = "another@example.com".into();
        state.generation.set(state.generation.get() + 1);
        state.related_sent.borrow_mut().clear();
        state.open_group.borrow_mut().clear();
        state.render_conversation();
        let Command::Compose {
            request, account, ..
        } = command(&commands)
        else {
            panic!("Expected save after switching views")
        };
        assert_eq!(account.email, "me@example.com");
        let late_saved = draft("late-saved", 21);
        state.event(Event::Composed(
            request,
            Ok(Outcome::Saved(models::draft::Saved {
                folder: late_saved.folder,
                message: late_saved.message,
                cleanup_error: None,
            })),
        ));
        assert!(state.related_sent.borrow().is_empty());
        assert!(late_editor.widget.parent().is_none());
        window.close();
    }
}

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Save,
    Send,
    Delete,
}

pub(super) struct Pending {
    compose: Rc<ui::compose::Compose>,
    editor: Option<Rc<Editor>>,
    account: String,
    generation: u64,
    selection: u64,
    operation: Operation,
}

struct Submission {
    request: u64,
    account: Account,
    target: Option<Target>,
    anchor: Option<String>,
    save: bool,
}

pub(super) struct Local {
    pub item: SentMessage,
    pub generation: u64,
    pub selection: u64,
}

impl Editor {
    fn key(&self) -> Key {
        let source = self.source.borrow();
        (
            self.account.email.clone(),
            source.folder.clone(),
            source.message.message_id.clone(),
        )
    }

    fn target(&self) -> Target {
        let source = self.source.borrow();
        Target::new(&source.folder, &source.message)
    }
}

impl State {
    pub(super) fn draft_key(&self, folder: &str, id: &str) -> Key {
        (
            self.account
                .borrow()
                .as_ref()
                .map(|account| account.email.clone())
                .unwrap_or_default(),
            folder.into(),
            id.into(),
        )
    }

    pub(super) fn draft_editor(self: &Rc<Self>, folder: &str, message: &Message) -> Rc<Editor> {
        let key = self.draft_key(folder, &message.message_id);
        let existing = self.draft_editors.borrow().get(&key).cloned();
        let editor = existing.unwrap_or_else(|| {
            let compose = Rc::new(ui::compose::Compose::new(&self.compose_button));
            if !message.body_loaded {
                compose.load_draft(&Draft::from(message));
            }
            compose.set_ready(false);
            let widget = ui::column("draft-editor");
            widget.append(&compose.widget);
            let status = ui::horizontal("draft-status", theme::SMALL_SPACING);
            let status_label = ui::label("Loading draft…", "dim-label");
            let retry = gtk::Button::with_label("Retry");
            retry.set_visible(false);
            status.append(&status_label);
            status.append(&retry);
            widget.append(&status);
            let editor = Rc::new(Editor {
                widget,
                compose,
                account: self
                    .account
                    .borrow()
                    .as_ref()
                    .expect("Draft needs an account")
                    .clone(),
                source: RefCell::new(SentMessage {
                    folder: folder.into(),
                    message: message.clone(),
                }),
                loaded: Cell::new(false),
                confirming: Cell::new(false),
                status,
                status_label,
                retry,
            });
            self.connect_composer(&editor.compose, Some(&editor));
            let weak = Rc::downgrade(self);
            let target = Rc::downgrade(&editor);
            editor.compose.connect_cancel(move || {
                if let (Some(state), Some(editor)) = (weak.upgrade(), target.upgrade())
                    && !editor.confirming.replace(true)
                {
                    state.confirm_draft_delete(&editor);
                }
            });
            let weak = Rc::downgrade(self);
            let target = Rc::downgrade(&editor);
            editor.retry.connect_clicked(move |_| {
                if let (Some(state), Some(editor)) = (weak.upgrade(), target.upgrade()) {
                    let source = editor.source.borrow().clone();
                    editor.status_label.set_text("Loading draft…");
                    editor.retry.set_visible(false);
                    state
                        .sent_pending
                        .borrow_mut()
                        .remove(&source.message.message_id);
                    state.open_sent(
                        &source.folder,
                        source.message.uid,
                        &source.message.message_id,
                    );
                }
            });
            self.draft_editors.borrow_mut().insert(key, editor.clone());
            editor
        });
        if message.body_loaded && !editor.loaded.replace(message.body_loaded) {
            editor.compose.load_draft(&Draft::from(message));
            editor.compose.set_ready(true);
            editor.compose.widget.set_sensitive(true);
            editor.status.set_visible(false);
            *editor.source.borrow_mut() = SentMessage {
                folder: folder.into(),
                message: message.clone(),
            };
        }
        editor
    }

    // Draft changes leave existing message cards, WebViews, and expansion state untouched.
    pub(super) fn update_draft_widgets(self: &Rc<Self>) {
        let folder = self.folder.borrow().clone();
        let mut items: Vec<_> = self
            .open_group
            .borrow()
            .iter()
            .map(|original| {
                let message = self
                    .messages
                    .borrow()
                    .iter()
                    .find(|message| {
                        message.uid == original.uid && message.message_id == original.message_id
                    })
                    .cloned()
                    .unwrap_or_else(|| original.clone());
                (false, folder.clone(), message)
            })
            .collect();
        items.extend(
            self.related_sent
                .borrow()
                .iter()
                .map(|item| (true, item.folder.clone(), item.message.clone())),
        );
        items.retain(|(_, folder, message)| {
            !self
                .draft_removed
                .borrow()
                .contains(&self.draft_key(folder, &message.message_id))
        });
        items.sort_by_key(|(_, _, message)| std::cmp::Reverse((message.timestamp, message.uid)));
        let mut desired = Vec::new();
        for (sent, folder, message) in &items {
            if message.is_draft {
                let editor = self.draft_editor(folder, message);
                desired.push((true, editor.widget.clone().upcast::<gtk::Widget>()));
                if !message.body_loaded && !editor.loaded.get() {
                    if *sent {
                        self.open_sent(folder, message.uid, &message.message_id);
                    } else {
                        self.open(message.uid);
                    }
                }
            } else {
                let card = if *sent {
                    self.sent_cards.borrow().get(&message.message_id).cloned()
                } else {
                    self.cards.borrow().get(&message.uid).cloned()
                };
                let Some(card) = card else {
                    self.render_conversation();
                    return;
                };
                desired.push((false, card.widget.upcast::<gtk::Widget>()));
            }
        }
        let mut previous = self.viewer.first_child().filter(|widget| {
            !widget.has_css_class("draft-editor") && !desired.iter().any(|(_, item)| item == widget)
        });
        let mut child = self.viewer.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if widget.has_css_class("draft-editor")
                && !desired.iter().any(|(_, item)| item == &widget)
            {
                self.viewer.remove(&widget);
            }
        }
        for (draft, widget) in desired {
            if draft {
                if widget.parent().is_some() {
                    self.viewer.reorder_child_after(&widget, previous.as_ref());
                } else {
                    self.viewer.insert_child_after(&widget, previous.as_ref());
                }
            }
            previous = Some(widget);
        }
    }

    pub(super) fn draft_body_error(&self, folder: &str, id: &str, error: &str) {
        if let Some(editor) = self.draft_editors.borrow().get(&self.draft_key(folder, id)) {
            editor.status_label.set_text(error);
            editor.retry.set_visible(true);
        }
    }

    pub(super) fn connect_composer(
        self: &Rc<Self>,
        compose: &Rc<ui::compose::Compose>,
        editor: Option<&Rc<Editor>>,
    ) {
        for save in [false, true] {
            let weak = Rc::downgrade(self);
            let composer = Rc::downgrade(compose);
            let target = editor.map(Rc::downgrade);
            let callback = move || -> ui::compose::Complete {
                let (Some(state), Some(compose)) = (weak.upgrade(), composer.upgrade()) else {
                    return Box::new(|_| {});
                };
                let Some(submission) = state.prepare_composer(
                    compose,
                    target.as_ref().and_then(std::rc::Weak::upgrade),
                    save,
                ) else {
                    return Box::new(|_| {});
                };
                let weak = Rc::downgrade(&state);
                Box::new(move |draft| {
                    if let Some(state) = weak.upgrade() {
                        state.submit_composer(submission, draft);
                    }
                })
            };
            if save {
                compose.connect_save(callback);
            } else {
                compose.connect_send(callback);
            }
        }
    }

    fn next_composer_request(&self) -> u64 {
        let request = self.composer_request.get() + 1;
        self.composer_request.set(request);
        request
    }

    fn prepare_composer(
        self: &Rc<Self>,
        compose: Rc<ui::compose::Compose>,
        editor: Option<Rc<Editor>>,
        save: bool,
    ) -> Option<Submission> {
        let account = editor
            .as_ref()
            .map(|editor| editor.account.clone())
            .or_else(|| self.account.borrow().clone());
        let Some(account) = account else {
            compose.finish_send(false);
            self.toast
                .add_toast(adw::Toast::new("Select an account first"));
            return None;
        };
        let request = self.next_composer_request();
        let target = editor.as_ref().map(|editor| editor.target());
        let anchor = self
            .open_group
            .borrow()
            .iter()
            .find(|message| !message.is_draft && !message.message_id.is_empty())
            .map(|message| message.message_id.clone());
        self.composer_pending.borrow_mut().insert(
            request,
            Pending {
                compose,
                editor,
                account: account.email.clone(),
                generation: self.generation.get(),
                selection: self.selection.get(),
                operation: if save {
                    Operation::Save
                } else {
                    Operation::Send
                },
            },
        );
        Some(Submission {
            request,
            account,
            target,
            anchor,
            save,
        })
    }

    fn submit_composer(self: &Rc<Self>, submission: Submission, draft: Result<Draft, String>) {
        let Submission {
            request,
            account,
            target,
            anchor,
            save,
        } = submission;
        let result = draft.and_then(|mut draft| {
            if save
                && target.is_none()
                && draft.references.is_empty()
                && let Some(anchor) = anchor
            {
                draft.references.push(anchor);
            }
            self.sender
                .send(Command::Compose {
                    account,
                    draft,
                    target,
                    request,
                    save,
                })
                .map_err(|error| error.to_string())
        });
        if let Err(error) = result {
            self.composer_finished(request, Err(error));
        }
    }

    pub(super) fn confirm_draft_delete(self: &Rc<Self>, editor: &Rc<Editor>) -> adw::MessageDialog {
        let parent = self.toast.root().and_downcast::<gtk::Window>();
        let dialog = adw::MessageDialog::new(
            parent.as_ref(),
            Some("Are you sure you want to delete this draft?"),
            None,
        );
        dialog.add_responses(&[("cancel", "Keep Draft"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = Rc::downgrade(self);
        let target = Rc::downgrade(editor);
        dialog.connect_response(None, move |_, response| {
            if let Some(editor) = target.upgrade() {
                editor.confirming.set(false);
                if response == "delete"
                    && let Some(state) = weak.upgrade()
                {
                    state.delete_draft(editor);
                }
            }
        });
        dialog.present();
        dialog
    }

    fn remove_draft(&self, editor: &Editor) {
        let key = editor.key();
        self.draft_removed.borrow_mut().insert(key.clone());
        self.draft_local.borrow_mut().remove(&key);
        self.draft_editors.borrow_mut().remove(&key);
        self.related_sent
            .borrow_mut()
            .retain(|item| item.folder != key.1 || item.message.message_id != key.2);
    }

    fn delete_draft(self: &Rc<Self>, editor: Rc<Editor>) {
        if !editor.compose.widget.is_sensitive() {
            return;
        }
        editor.compose.widget.set_sensitive(false);
        let request = self.next_composer_request();
        let account = editor.account.clone();
        let target = editor.target();
        self.composer_pending.borrow_mut().insert(
            request,
            Pending {
                compose: editor.compose.clone(),
                editor: Some(editor.clone()),
                account: account.email.clone(),
                generation: self.generation.get(),
                selection: self.selection.get(),
                operation: Operation::Delete,
            },
        );
        self.remove_draft(&editor);
        self.update_draft_widgets();
        if let Err(error) = self.sender.send(Command::DeleteDraft {
            account,
            target,
            request,
        }) {
            self.draft_deleted(request, Err(error.to_string()));
        }
    }

    fn pending_visible(&self, pending: &Pending) -> bool {
        pending.generation == self.generation.get()
            && pending.selection == self.selection.get()
            && self
                .account
                .borrow()
                .as_ref()
                .is_some_and(|account| account.email == pending.account)
    }

    pub(super) fn composer_finished(
        self: &Rc<Self>,
        request: u64,
        result: Result<Outcome, String>,
    ) {
        let Some(pending) = self.composer_pending.borrow_mut().remove(&request) else {
            return;
        };
        let visible = self.pending_visible(&pending);
        match result {
            Err(error) => {
                pending.compose.finish_send(false);
                let verb = if matches!(pending.operation, Operation::Save) {
                    "save draft"
                } else {
                    "send message"
                };
                self.toast
                    .add_toast(adw::Toast::new(&format!("Could not {verb}: {error}")));
            }
            Ok(outcome) => {
                let (item, cleanup_error, saved) = match outcome {
                    Outcome::Saved(saved) => (
                        SentMessage {
                            folder: saved.folder,
                            message: saved.message,
                        },
                        saved.cleanup_error,
                        true,
                    ),
                    Outcome::Sent(message, error) => (
                        SentMessage {
                            folder: String::new(),
                            message,
                        },
                        error,
                        false,
                    ),
                };
                if let Some(editor) = &pending.editor {
                    self.remove_draft(editor);
                }
                if saved && let Some(editor) = &pending.editor {
                    *editor.source.borrow_mut() = item.clone();
                    editor.compose.finish_send(false);
                    self.draft_editors
                        .borrow_mut()
                        .insert(editor.key(), editor.clone());
                } else {
                    pending.compose.finish_send(true);
                }
                let key = (
                    pending.account.clone(),
                    item.folder.clone(),
                    item.message.message_id.clone(),
                );
                self.draft_local.borrow_mut().insert(
                    key,
                    Local {
                        item: item.clone(),
                        generation: pending.generation,
                        selection: pending.selection,
                    },
                );
                if visible {
                    self.related_sent
                        .borrow_mut()
                        .retain(|old| old.message.message_id != item.message.message_id);
                    self.related_sent.borrow_mut().push(item);
                    if saved {
                        self.update_draft_widgets();
                    } else {
                        self.render_conversation();
                    }
                }
                let message = if saved { "Draft saved" } else { "Message sent" };
                self.toast.add_toast(adw::Toast::new(&match cleanup_error {
                    Some(error) => {
                        format!("{message}, but could not remove the previous draft: {error}")
                    }
                    None => message.into(),
                }));
            }
        }
    }

    pub(super) fn draft_deleted(self: &Rc<Self>, request: u64, result: Result<(), String>) {
        let Some(pending) = self.composer_pending.borrow_mut().remove(&request) else {
            return;
        };
        if let Err(error) = result {
            if let Some(editor) = &pending.editor {
                self.draft_removed.borrow_mut().remove(&editor.key());
                self.draft_editors
                    .borrow_mut()
                    .insert(editor.key(), editor.clone());
                editor.compose.finish_send(false);
                if self.pending_visible(&pending) {
                    self.related_sent
                        .borrow_mut()
                        .push(editor.source.borrow().clone());
                    self.update_draft_widgets();
                }
            }
            self.toast
                .add_toast(adw::Toast::new(&format!("Could not delete draft: {error}")));
        }
    }

    pub(super) fn reconcile_drafts(&self, messages: &mut Vec<SentMessage>) {
        let Some(account) = self
            .account
            .borrow()
            .as_ref()
            .map(|account| account.email.clone())
        else {
            return;
        };
        messages.retain(|item| {
            !self.draft_removed.borrow().contains(&(
                account.clone(),
                item.folder.clone(),
                item.message.message_id.clone(),
            ))
        });
        let group = self.open_group.borrow();
        self.draft_local.borrow_mut().retain(|key, local| {
            if key.0 != account {
                return true;
            }
            if messages.iter().any(|item| {
                item.folder == local.item.folder
                    && item.message.message_id == local.item.message.message_id
                    && item.message.uid != 0
            }) {
                return false;
            }
            if (local.generation == self.generation.get()
                && local.selection == self.selection.get())
                || !models::conversation::related(&group, std::slice::from_ref(&local.item))
                    .is_empty()
            {
                messages.push(local.item.clone());
            }
            true
        });
    }
}
