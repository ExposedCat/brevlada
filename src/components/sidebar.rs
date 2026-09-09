use super::{button, column, horizontal, label};
use crate::{models::Account, theme};
use adw::prelude::*;
use std::{cell::RefCell, rc::Rc};

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
        content.append(&label(
            if account.name.is_empty() {
                &account.email
            } else {
                &account.name
            },
            "account-text",
        ));
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
            let expanded = !target.is_visible();
            target.set_visible(expanded);
            button.set_icon_name(if expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            });
            if expanded && target.first_child().is_none() {
                discover();
            }
        });
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
