use super::*;

impl Subscriptions {
    pub fn has_failed(&self, id: &str) -> bool {
        matches!(self.statuses.borrow().get(id), Some(Status::Failed(_)))
    }

    pub fn begin_unsubscribe(&self, entry: &Entry, retry: bool) {
        if retry
            && self.has_failed(&entry.id)
            && let Some(unsubscribe) = &entry.source.message.unsubscribe
        {
            self.retry_urls
                .borrow_mut()
                .insert(entry.id.clone(), unsubscribe.url.clone());
        }
        self.status(&entry.id, Status::Pending);
    }

    pub fn status(&self, id: &str, status: Status) {
        let fallback = if !matches!(status, Status::Pending) {
            self.retry_urls
                .borrow_mut()
                .remove(id)
                .and_then(|url| match &status {
                    Status::Failed(_) => Some(url),
                    Status::Manual(url) => Some(url.clone()),
                    _ => None,
                })
        } else {
            None
        };
        self.statuses.borrow_mut().insert(id.into(), status);
        self.unselect_unavailable();
        let selected = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        self.sort(selected);
        self.refresh_rows();
        self.update_controls();
        if let Some(url) = fallback {
            self.open(&url);
        }
    }

    pub(super) fn compare(&self, left: &Entry, right: &Entry) -> std::cmp::Ordering {
        self.has_failed(&right.id)
            .cmp(&self.has_failed(&left.id))
            .then_with(|| compare_entries(left, right))
    }

    pub(super) fn unselect_unavailable(&self) {
        for position in 0..self.store.n_items() {
            if self
                .entry(position)
                .is_some_and(|entry| self.unavailable(&entry.id))
            {
                self.selection.unselect_item(position);
            }
        }
    }

    pub(super) fn unavailable(&self, id: &str) -> bool {
        matches!(
            self.statuses.borrow().get(id),
            Some(Status::Done | Status::Pending)
        )
    }

    pub(super) fn open(&self, url: &str) {
        (self.open_link.borrow())(url);
    }

    #[cfg(test)]
    pub fn record_links(&self, opened: Rc<RefCell<Vec<String>>>) {
        *self.open_link.borrow_mut() = Rc::new(move |url| opened.borrow_mut().push(url.to_owned()));
    }
}
