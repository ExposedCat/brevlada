use super::*;

impl State {
    pub(super) fn configure_composer(&self, compose: &ui::compose::Compose, selected: &str) {
        let mut accounts: Vec<_> = self
            .folder_boxes
            .borrow()
            .values()
            .map(|(account, _)| account.clone())
            .collect();
        if let Some(account) = self.account.borrow().as_ref()
            && !accounts.iter().any(|item| item.email == account.email)
        {
            accounts.push(account.clone());
        }
        accounts.sort_by(|left, right| left.email.cmp(&right.email));
        compose.set_accounts(&accounts, selected);
    }

    pub(super) fn refresh_composer_accounts(&self) {
        for open in self.open_composers.borrow().iter() {
            let selected = open
                .compose
                .account()
                .unwrap_or_else(|| open.account.clone());
            self.configure_composer(&open.compose, &selected.email);
        }
        for editor in self.draft_editors.borrow().values() {
            let selected = editor
                .compose
                .account()
                .unwrap_or_else(|| editor.account.clone());
            self.configure_composer(&editor.compose, &selected.email);
        }
    }

    pub(super) fn refresh_composers(&self) {
        for open in self.open_composers.borrow().iter() {
            let same_view = self
                .account
                .borrow()
                .as_ref()
                .is_some_and(|account| account.email == open.account.email)
                && *self.folder.borrow() == open.folder
                && self.sender_pane.borrow().sender() == open.sender.as_deref();
            open.compose
                .widget
                .set_visible(open.compose.is_pinned() || same_view);
        }
        for editor in self.draft_editors.borrow().values() {
            let parent = editor.widget.parent();
            if editor.compose.is_pinned() {
                if parent.as_ref() == Some(self.viewer.upcast_ref()) {
                    self.viewer.remove(&editor.widget);
                }
                if editor.widget.parent().is_none() {
                    self.composers.prepend(&editor.widget);
                }
            } else if parent.as_ref() == Some(self.composers.upcast_ref()) {
                self.composers.remove(&editor.widget);
            }
        }
    }
}
