use super::*;
use models::{
    Draft,
    draft::{Saved, Target},
};

impl State {
    pub(super) fn connect_autosave(
        self: &Rc<Self>,
        compose: &Rc<ui::compose::Compose>,
        editor: Option<&Rc<drafts::Editor>>,
    ) {
        let weak = Rc::downgrade(self);
        let composer = Rc::downgrade(compose);
        let target = editor.map(Rc::downgrade);
        compose.connect_autosave(move || -> ui::compose::Complete {
            let (Some(state), Some(compose)) = (weak.upgrade(), composer.upgrade()) else {
                return Box::new(|_| {});
            };
            let Some(submission) =
                state.prepare_composer(compose, target.as_ref().and_then(std::rc::Weak::upgrade))
            else {
                return Box::new(|_| {});
            };
            let weak = Rc::downgrade(&state);
            Box::new(move |draft| {
                if let Some(state) = weak.upgrade() {
                    state.save_composer(submission, draft);
                }
            })
        });
    }

    fn save_composer(
        self: &Rc<Self>,
        submission: drafts::Submission,
        draft: Result<Draft, String>,
    ) {
        let drafts::Submission {
            request,
            account,
            target,
        } = submission;
        let result = draft.and_then(|draft| {
            if target.is_none()
                && draft.to.trim().is_empty()
                && draft.cc.trim().is_empty()
                && draft.subject.trim().is_empty()
                && draft.text.trim().is_empty()
                && draft
                    .html
                    .as_ref()
                    .is_none_or(|html| html.trim().is_empty())
                && draft.attachments.is_empty()
                && draft.attachment_source.is_none()
            {
                let pending = self.composer_pending.borrow_mut().remove(&request);
                if let Some(pending) = pending {
                    pending.compose.autosave.complete(Ok(()));
                    pending.compose.autosave.reset();
                }
                return Ok(());
            }
            self.sender
                .send(Command::SaveDraft {
                    account,
                    draft,
                    target,
                    request,
                })
                .map_err(|error| error.to_string())
        });
        if let Err(error) = result {
            self.draft_saved(request, Err(error));
        }
    }

    pub(super) fn forget_saved_draft(&self, account: &str, target: &Target) {
        let key = (
            account.to_owned(),
            target.folder.clone(),
            target.message_id.clone(),
        );
        self.draft_removed.borrow_mut().insert(key.clone());
        self.draft_local.borrow_mut().remove(&key);
        self.related_sent.borrow_mut().retain(|item| {
            item.folder != target.folder || item.message.message_id != target.message_id
        });
    }

    pub(super) fn draft_in_composer(&self, folder: &str, id: &str) -> bool {
        self.open_composers.borrow().iter().any(|open| {
            open.compose
                .draft_target()
                .is_some_and(|target| target.folder == folder && target.message_id == id)
        })
    }

    pub(super) fn draft_saved(self: &Rc<Self>, request: u64, result: Result<Saved, String>) {
        let Some(pending) = self.composer_pending.borrow_mut().remove(&request) else {
            return;
        };
        match result {
            Err(error) => pending.compose.autosave.complete(Err(error)),
            Ok(saved) => {
                if let Some(previous) = pending.compose.draft_target() {
                    self.forget_saved_draft(&pending.account, &previous);
                    self.draft_editors.borrow_mut().remove(&(
                        pending.account.clone(),
                        previous.folder,
                        previous.message_id,
                    ));
                }
                pending.compose.draft_saved(&saved);
                let item = saved.item;
                let key = (
                    pending.account.clone(),
                    item.folder.clone(),
                    item.message.message_id.clone(),
                );
                if let Some(editor) = &pending.editor {
                    *editor.source.borrow_mut() = item.clone();
                    self.draft_editors
                        .borrow_mut()
                        .insert(key.clone(), editor.clone());
                }
                self.draft_local.borrow_mut().insert(
                    key,
                    drafts::Local {
                        item: item.clone(),
                        generation: pending.generation,
                        selection: pending.selection,
                    },
                );
                if self.pending_visible(&pending) {
                    self.related_sent.borrow_mut().push(item);
                    self.update_draft_widgets();
                }
                pending
                    .compose
                    .autosave
                    .complete(match saved.cleanup_error {
                        Some(error) => Err(format!(
                            "Saved, but could not remove the previous draft: {error}"
                        )),
                        None => Ok(()),
                    });
            }
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn command(commands: &std::sync::mpsc::Receiver<Command>) -> Command {
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        let context = glib::MainContext::default();
        loop {
            while context.pending() {
                context.iteration(false);
            }
            if let Ok(command) = commands.try_recv() {
                return command;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Expected autosave or send command"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn saved(id: &str, uid: u32, subject: &str) -> Saved {
        Saved {
            item: models::SentMessage {
                folder: "Drafts".into(),
                message: Message {
                    uid,
                    uid_validity: Some(7),
                    message_id: id.into(),
                    subject: subject.into(),
                    recipients: "other@example.com".into(),
                    body_text: "Body".into(),
                    is_draft: true,
                    is_read: true,
                    body_loaded: true,
                    ..Default::default()
                },
            },
            original_parts: Vec::new(),
            cleanup_error: None,
        }
    }

    #[test]
    #[ignore = "Requires a graphical session for autosaving composers"]
    fn autosave_keeps_typing_and_sends_the_latest_text_using_the_saved_target() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaAutosaveDiagnostic")
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
        let compose = state.new_composer("other@example.com", None).unwrap();
        compose.load_draft(&Draft::from(&Message {
            recipients: "other@example.com".into(),
            body_text: "Body".into(),
            ..Default::default()
        }));
        let header = compose
            .widget
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap();
        let subject = header
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        subject.set_text("First");
        subject.set_text("Second");
        assert!(commands.try_recv().is_err());
        let Command::SaveDraft {
            request,
            draft,
            target: None,
            account,
        } = command(&commands)
        else {
            panic!("Expected debounced save");
        };
        assert_eq!(account.email, "me@example.com");
        assert_eq!(draft.subject, "Second");
        assert!(compose.widget.is_sensitive());
        subject.set_text("Latest");
        let send = header
            .last_child()
            .unwrap()
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(send.tooltip_text().as_deref(), Some("Send"));
        send.emit_clicked();
        send.emit_clicked();
        assert!(commands.try_recv().is_err());
        state.event(Event::DraftSaved(request, Ok(saved("saved", 40, "Second"))));
        let Command::Compose {
            request,
            draft,
            target: Some(target),
            ..
        } = command(&commands)
        else {
            panic!("Expected send after autosave completed");
        };
        assert_eq!(draft.subject, "Latest");
        assert!(draft.attachments_loaded);
        assert_eq!(target.message_id, "saved");
        assert_eq!(target.uid, 40);
        assert!(commands.try_recv().is_err());
        state.event(Event::Composed(
            request,
            Ok(models::draft::Outcome {
                message: Message {
                    message_id: "sent".into(),
                    body_loaded: true,
                    ..Default::default()
                },
                cleanup_error: None,
            }),
        ));
        assert!(!compose.widget.get_visible());
        assert!(state.open_composers.borrow().is_empty());
        assert!(state.draft_removed.borrow().contains(&(
            "me@example.com".into(),
            "Drafts".into(),
            "saved".into()
        )));
    }

    #[test]
    #[ignore = "Requires a graphical session for autosaving composers"]
    fn closing_flushes_edits_and_failed_saves_keep_the_composer_open() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaAutosaveCloseDiagnostic")
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
        let compose = state.new_composer("other@example.com", None).unwrap();
        let header = compose
            .widget
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap();
        let subject = header
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        let close = header
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        subject.set_text("Unfinished");
        close.emit_clicked();
        let Command::SaveDraft { request, draft, .. } = command(&commands) else {
            panic!("Expected flush");
        };
        assert_eq!(draft.subject, "Unfinished");
        assert!(draft.text.is_empty());
        state.event(Event::DraftSaved(request, Err("Offline".into())));
        assert!(compose.widget.get_visible());
        assert!(compose.autosave.status.text().contains("Offline"));
        assert_eq!(subject.text(), "Unfinished");
        close.emit_clicked();
        let Command::SaveDraft { request, .. } = command(&commands) else {
            panic!("Expected save retry");
        };
        subject.set_text("Final close");
        state.event(Event::DraftSaved(
            request,
            Ok(saved("closed", 41, "Unfinished")),
        ));
        assert!(compose.widget.get_visible());
        let Command::SaveDraft {
            request,
            draft,
            target: Some(target),
            ..
        } = command(&commands)
        else {
            panic!("Expected the edits made during saving to flush before closing");
        };
        assert_eq!(target.uid, 41);
        assert_eq!(draft.subject, "Final close");
        state.event(Event::DraftSaved(
            request,
            Ok(saved("closed-latest", 42, "Final close")),
        ));
        assert!(!compose.widget.get_visible());
        assert!(state.open_composers.borrow().is_empty());
        assert!(state.draft_local.borrow().contains_key(&(
            "me@example.com".into(),
            "Drafts".into(),
            "closed-latest".into()
        )));
    }
}
