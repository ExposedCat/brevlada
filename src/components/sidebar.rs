use super::{button, column, horizontal, label};
use crate::{models::Account, theme};
use adw::prelude::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[derive(Clone, Default)]
pub struct Unread(Rc<RefCell<UnreadState>>);

#[derive(Default)]
struct UnreadState {
    folders: HashSet<String>,
    labels: HashMap<String, gtk::Label>,
}

impl Unread {
    pub fn bind(&self, folder: &str, label: &gtk::Label) {
        self.0
            .borrow_mut()
            .labels
            .insert(folder.into(), label.clone());
        self.refresh();
    }

    pub fn clear_folder_labels(&self) {
        self.0
            .borrow_mut()
            .labels
            .retain(|folder, _| folder.is_empty());
    }

    pub fn update(&self, folders: Vec<(String, bool)>) {
        let mut state = self.0.borrow_mut();
        for (folder, unread) in folders {
            if unread {
                state.folders.insert(folder);
            } else {
                state.folders.remove(&folder);
            }
        }
        drop(state);
        self.refresh();
    }

    pub fn replace(&self, folders: Vec<(String, bool)>) {
        self.0.borrow_mut().folders.clear();
        self.update(folders);
    }

    fn refresh(&self) {
        let state = self.0.borrow();
        for (folder, label) in &state.labels {
            let unread = state.folders.iter().any(|path| {
                folder.is_empty() || path == folder || path.starts_with(&format!("{folder}/"))
            });
            if unread {
                label.add_css_class("unread");
            } else {
                label.remove_css_class("unread");
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Selection(Rc<RefCell<Option<gtk::Button>>>);
impl Selection {
    pub fn activate(&self, button: &gtk::Button) {
        if let Some(previous) = self.0.borrow_mut().replace(button.clone()) {
            previous.unset_state_flags(gtk::StateFlags::CHECKED);
        }
        button.set_state_flags(gtk::StateFlags::CHECKED, false);
    }
}

pub struct AccountRow {
    pub widget: gtk::Box,
    pub folders: gtk::Box,
}

impl AccountRow {
    pub fn new(
        account: &Account,
        unread: &Unread,
        expansion: &super::expansion::Expansion,
        selection: Selection,
        select: impl Fn() + 'static,
        discover: impl Fn() + 'static,
    ) -> Self {
        let widget = column("account-item");
        let row = horizontal("account-container", theme::SMALL_SPACING);
        let expand = button("pan-end-symbolic", "Expand account");
        expand.add_css_class("account-expand");
        row.append(&expand);
        let content = horizontal("account-content", theme::SMALL_SPACING);
        content.append(&gtk::Image::from_icon_name("mail-unread-symbolic"));
        let name = label(
            if account.name.is_empty() {
                &account.email
            } else {
                &account.name
            },
            "account-text",
        );
        unread.bind("", &name);
        content.append(&name);
        let account_button = gtk::Button::builder()
            .child(&content)
            .hexpand(true)
            .css_classes(["flat", "account-button"])
            .build();
        account_button.connect_clicked(move |button| {
            selection.activate(button);
            select();
        });
        row.append(&account_button);
        widget.append(&row);
        let folders = column("account-folders");
        folders.set_visible(false);
        widget.append(&folders);
        let target = folders.clone();
        expand.connect_clicked(move |button| {
            let expanded = !target.get_visible();
            target.set_visible(expanded);
            button.set_icon_name(if expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            });
            if expanded
                && (target.first_child().is_none() || target.has_css_class("folders-loading"))
            {
                if target.first_child().is_none() {
                    let loading = label("Loading folders…", "dim-label");
                    loading.set_margin_start(theme::INDENT);
                    target.append(&loading);
                }
                target.add_css_class("folders-loading");
                discover();
            }
        });
        if expansion.is_expanded("") {
            expand.emit_clicked();
        }
        expansion.bind("", &folders);
        Self { widget, folders }
    }
}

pub fn folder_icon(name: &str) -> &'static str {
    let name = name.to_uppercase();
    if name == "INBOX" {
        "mail-unread-symbolic"
    } else if name.contains("SENT") || name.contains("ITEMS") {
        "mail-send-symbolic"
    } else if name.contains("DRAFT") {
        "document-edit-symbolic"
    } else if name.contains("TRASH") || name.contains("DELETED") {
        "user-trash-symbolic"
    } else if name.contains("SPAM") || name.contains("JUNK") || name.contains("BULK") {
        "mail-mark-junk-symbolic"
    } else if name.contains("ALL MAIL") {
        "mail-unread-symbolic"
    } else if name.contains("ARCHIVE") {
        "shoe-box-symbolic"
    } else if name.contains("IMPORTANT") {
        "mail-mark-important-symbolic"
    } else if name.contains("STARRED") {
        "starred-symbolic"
    } else {
        "folder-symbolic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn unread_highlights_follow_nested_folders_and_clear_after_reading() {
        gtk::init().unwrap();
        let unread = Unread::default();
        let account = label("Account", "account-text");
        unread.bind("", &account);
        let container = column("account-folders");
        let populate = || {
            super::super::clear(&container);
            super::super::folders::populate(
                &container,
                vec!["Work/Updates".into(), "Personal".into()],
                &unread,
                &super::super::expansion::Expansion::default(),
                Selection::default(),
                |_| {},
            );
        };
        populate();
        assert!(!account.has_css_class("unread"));
        unread.update(vec![("Work/Updates".into(), true)]);
        assert!(account.has_css_class("unread"));
        for folder in ["Work", "Work/Updates"] {
            assert!(unread.0.borrow().labels[folder].has_css_class("unread"));
        }
        assert!(!unread.0.borrow().labels["Personal"].has_css_class("unread"));
        populate();
        assert!(unread.0.borrow().labels["Work/Updates"].has_css_class("unread"));
        unread.update(vec![("Work/Updates".into(), false)]);
        assert!(
            unread
                .0
                .borrow()
                .labels
                .values()
                .all(|label| !label.has_css_class("unread"))
        );
        unread.update(vec![("INBOX".into(), true)]);
        assert!(account.has_css_class("unread"));
        assert!(!unread.0.borrow().labels["Work"].has_css_class("unread"));
        unread.replace(Vec::new());
        assert!(!account.has_css_class("unread"));
    }
}
