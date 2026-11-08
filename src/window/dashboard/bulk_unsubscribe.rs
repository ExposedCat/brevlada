use super::*;
use crate::backend::unsubscribe::Outcome;
use std::collections::VecDeque;

pub(super) struct Batch {
    queue: VecDeque<Entry>,
    current: Option<Entry>,
    total: usize,
    completed: usize,
    succeeded: usize,
    failed: usize,
    manual: usize,
    stopped: bool,
    retry: bool,
}

impl State {
    pub(in crate::window) fn bulk_unsubscribe_active(&self) -> bool {
        self.dashboard.bulk_unsubscribe.borrow().is_some()
    }

    pub(super) fn show_subscriptions(self: &Rc<Self>) {
        if !self.bulk_unsubscribe_active() {
            self.subscriptions.load(self.subscription_entries());
            for id in self.dashboard.unsubscribing.borrow().keys() {
                self.subscriptions
                    .status(id, ui::subscriptions::Status::Pending);
            }
        }
        self.content_stack.set_visible_child_name("subscriptions");
    }

    fn subscription_entries(&self) -> Vec<Entry> {
        self.dashboard
            .items
            .borrow()
            .iter()
            .filter(|entry| {
                matches!(entry.content, models::dashboard::Content::Subscription)
                    && !self.dashboard.dismissed.borrow().contains(&entry.id)
            })
            .cloned()
            .collect()
    }

    pub(super) fn refresh_subscriptions(&self) {
        if self.content_stack.visible_child_name().as_deref() == Some("subscriptions") {
            self.subscriptions.update(self.subscription_entries());
        }
    }

    pub(super) fn confirm_subscription_selection(
        self: &Rc<Self>,
        entries: Vec<Entry>,
        retry: bool,
    ) {
        if entries.is_empty() || self.bulk_unsubscribe_active() {
            return;
        }
        if retry && entries.len() == 1 && self.subscriptions.has_failed(&entries[0].id) {
            self.start_bulk_unsubscribe(entries, true);
            return;
        }
        let parent = self.toast.root().and_downcast::<gtk::Window>();
        let weak = Rc::downgrade(self);
        ui::action_dialog::confirm(
            parent.as_ref(),
            &format!("Unsubscribe from {} selected senders?", entries.len()),
            "Unsubscribe",
            true,
            move || {
                if let Some(state) = weak.upgrade() {
                    state.start_bulk_unsubscribe(entries.clone(), false);
                }
            },
        );
    }

    fn start_bulk_unsubscribe(self: &Rc<Self>, entries: Vec<Entry>, retry: bool) {
        if entries.is_empty() || self.bulk_unsubscribe_active() {
            return;
        }
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                !self.dashboard.dismissed.borrow().contains(&entry.id)
                    && !self
                        .dashboard
                        .unsubscribing
                        .borrow()
                        .contains_key(&entry.id)
            })
            .collect();
        if entries.is_empty() {
            return;
        }
        *self.dashboard.bulk_unsubscribe.borrow_mut() = Some(Batch {
            total: entries.len(),
            queue: entries.into(),
            current: None,
            completed: 0,
            succeeded: 0,
            failed: 0,
            manual: 0,
            stopped: false,
            retry,
        });
        self.update_bulk_unsubscribe();
        self.next_bulk_unsubscribe();
    }

    pub(super) fn stop_bulk_unsubscribe(self: &Rc<Self>) {
        if let Some(batch) = self.dashboard.bulk_unsubscribe.borrow_mut().as_mut() {
            batch.stopped = true;
            batch.queue.clear();
        }
        self.update_bulk_unsubscribe();
    }

    fn next_bulk_unsubscribe(self: &Rc<Self>) {
        loop {
            let entry = {
                let mut batch = self.dashboard.bulk_unsubscribe.borrow_mut();
                let Some(batch) = batch.as_mut() else { return };
                batch.queue.pop_front()
            };
            let Some(entry) = entry else {
                self.finish_bulk_unsubscribe();
                return;
            };
            let retry = self
                .dashboard
                .bulk_unsubscribe
                .borrow()
                .as_ref()
                .unwrap()
                .retry;
            self.subscriptions.begin_unsubscribe(&entry, retry);
            let account = self
                .folder_boxes
                .borrow()
                .get(&entry.source.account)
                .map(|(account, _)| account.clone());
            let result = account
                .ok_or_else(|| "Account no longer available".to_string())
                .and_then(|account| {
                    self.sender
                        .send(Command::Unsubscribe {
                            account,
                            folder: entry.source.folder.clone(),
                            uid: entry.source.message.uid,
                            message_id: entry.source.message.message_id.clone(),
                        })
                        .map_err(|error| error.to_string())
                });
            if result.is_ok() {
                self.dashboard
                    .unsubscribing
                    .borrow_mut()
                    .insert(entry.id.clone(), entry.source.clone());
                self.dashboard
                    .bulk_unsubscribe
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .current = Some(entry);
                self.update_bulk_unsubscribe();
                return;
            }
            self.subscriptions.status(
                &entry.id,
                ui::subscriptions::Status::Failed(result.unwrap_err()),
            );
            let mut batch = self.dashboard.bulk_unsubscribe.borrow_mut();
            let batch = batch.as_mut().unwrap();
            batch.completed += 1;
            batch.failed += 1;
        }
    }

    pub(in crate::window) fn bulk_unsubscribe_finished(
        self: &Rc<Self>,
        account: &str,
        folder: &str,
        uid: u32,
        message_id: &str,
        result: &Result<Outcome, String>,
    ) -> bool {
        {
            let mut batch = self.dashboard.bulk_unsubscribe.borrow_mut();
            let Some(batch) = batch.as_mut() else {
                return false;
            };
            if !batch.current.as_ref().is_some_and(|entry| {
                let source = &entry.source;
                source.account == account
                    && source.folder == folder
                    && source.message.uid == uid
                    && source.message.message_id == message_id
            }) {
                return false;
            }
            batch.current = None;
            batch.completed += 1;
            match result {
                Ok(Outcome::Done) => batch.succeeded += 1,
                Ok(Outcome::Open(_)) => batch.manual += 1,
                Err(_) => batch.failed += 1,
            }
        }
        self.next_bulk_unsubscribe();
        true
    }

    fn update_bulk_unsubscribe(self: &Rc<Self>) {
        let text = self
            .dashboard
            .bulk_unsubscribe
            .borrow()
            .as_ref()
            .map(|batch| {
                let sender = batch
                    .current
                    .as_ref()
                    .map(|entry| models::senders::identity(&entry.source.message).1)
                    .unwrap_or_default();
                let text = if batch.stopped {
                    format!(
                        "Stopping after the current request… {} of {} processed",
                        batch.completed, batch.total
                    )
                } else {
                    format!(
                        "Unsubscribing… {} of {} processed · {sender}",
                        batch.completed, batch.total
                    )
                };
                (text, batch.stopped)
            });
        if let Some((text, stopping)) = text {
            self.dashboard
                .view
                .unsubscribe_progress(&text, true, stopping);
            self.subscriptions.progress(&text, true, stopping);
        }
        self.block_unsubscribe_buttons();
    }

    fn finish_bulk_unsubscribe(self: &Rc<Self>) {
        let Some(batch) = self.dashboard.bulk_unsubscribe.borrow_mut().take() else {
            return;
        };
        let text = format!(
            "{} · {} of {} processed · {} unsubscribed · {} need manual action · {} failed",
            if batch.stopped { "Stopped" } else { "Finished" },
            batch.completed,
            batch.total,
            batch.succeeded,
            batch.manual,
            batch.failed
        );
        self.dashboard
            .view
            .unsubscribe_progress(&text, false, false);
        self.subscriptions.progress(&text, false, false);
        self.block_unsubscribe_buttons();
    }

    fn block_unsubscribe_buttons(self: &Rc<Self>) {
        let blocked = self.bulk_unsubscribe_active();
        for card in self
            .cards
            .borrow()
            .values()
            .chain(self.sent_cards.borrow().values())
        {
            card.block_unsubscribe(blocked);
        }
        if self.dashboard_visible() {
            self.render_dashboard();
        }
    }
}
