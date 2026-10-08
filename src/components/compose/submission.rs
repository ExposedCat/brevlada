use super::*;

impl Compose {
    pub fn draft_target(&self) -> Option<Target> {
        self.draft_target.borrow().clone()
    }

    pub fn set_draft_target(&self, target: Target) {
        *self.draft_target.borrow_mut() = Some(target);
    }

    pub fn draft_saved(&self, saved: &crate::models::draft::Saved) {
        self.set_draft_target(Target::new(&saved.item.folder, &saved.item.message));
        self.attachments.resolve(&saved.original_parts);
    }

    pub fn snapshot(&self) -> impl std::future::Future<Output = Result<Draft, String>> + use<> {
        let draft = Draft {
            to: self.receiver.text().to_string(),
            cc: self.cc.text().to_string(),
            subject: self.subject.text().to_string(),
            text: self
                .body
                .buffer()
                .text(
                    &self.body.buffer().start_iter(),
                    &self.body.buffer().end_iter(),
                    true,
                )
                .to_string(),
            html: None,
            attachments: self.attachments.parts(),
            attachments_loaded: self.attachments.loaded(),
            attachment_names: self.attachments.names(),
            removed_attachments: self.attachments.removed(),
            attachment_source: self.reply_to.borrow().attachment_source.clone(),
            in_reply_to: self.reply_to.borrow().in_reply_to.clone(),
            references: self.reply_to.borrow().references.clone(),
        };
        let formatted = self.html_mode.is_active();
        let editor = self.html_editor.clone();
        async move {
            if formatted {
                editor
                    .message()
                    .await
                    .map(|(html, text)| Draft {
                        html: Some(html),
                        text,
                        ..draft
                    })
                    .map_err(|error| format!("Could not read formatted message: {error}"))
            } else {
                Ok(draft)
            }
        }
    }

    pub fn connect_autosave(self: &Rc<Self>, callback: impl Fn() -> Complete + 'static) {
        let weak = Rc::downgrade(self);
        self.autosave.connect(move || {
            let Some(compose) = weak.upgrade() else {
                return;
            };
            if !compose.widget.get_visible() || !compose.receiver.is_sensitive() {
                compose.autosave.complete(Ok(()));
                return;
            }
            let complete = callback();
            gtk::glib::MainContext::default().spawn_local(async move {
                while compose.attachments.loading() {
                    gtk::glib::timeout_future(std::time::Duration::from_millis(50)).await;
                }
                compose.autosave.capture();
                complete(compose.snapshot().await);
            });
        });
    }

    pub fn connect_send(self: &Rc<Self>, callback: impl Fn() -> Complete + 'static) {
        let weak = Rc::downgrade(self);
        let callback = Rc::new(callback);
        self.send.connect_clicked(move |_| {
            let Some(compose) = weak.upgrade() else {
                return;
            };
            if !compose.widget.is_sensitive()
                || compose.attachments.loading()
                || compose.autosave.paused()
            {
                return;
            }
            compose.widget.set_sensitive(false);
            let callback = callback.clone();
            let autosave = compose.autosave.clone();
            autosave.defer(false, move || {
                compose.widget.set_sensitive(false);
                compose.send_error.set_visible(false);
                let snapshot = compose.snapshot();
                let complete = callback();
                gtk::glib::MainContext::default().spawn_local(async move {
                    complete(snapshot.await);
                });
            });
        });
    }
}
