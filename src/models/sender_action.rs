#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SenderAction {
    MarkRead,
    MarkUnread,
    Star,
    Unstar,
    MoveTo(String),
    Restore,
    Unspam,
    Spam,
    Archive,
    Delete,
}

impl SenderAction {
    #[cfg(test)]
    pub const ALL: [(Self, &'static str); 10] = [
        (Self::MarkRead, "Mark all as read"),
        (Self::Spam, "Mark as spam"),
        (Self::Archive, "Archive all"),
        (Self::Delete, "Delete all"),
        (Self::MarkUnread, "Mark all as unread"),
        (Self::Star, "Star all"),
        (Self::Unstar, "Unstar all"),
        (Self::MoveTo(String::new()), "Move to…"),
        (Self::Restore, "Restore from Trash"),
        (Self::Unspam, "Not spam"),
    ];

    pub fn removes_messages(&self) -> bool {
        !matches!(
            self,
            Self::MarkRead | Self::MarkUnread | Self::Star | Self::Unstar
        )
    }

    pub fn read_state(&self) -> Option<bool> {
        match self {
            Self::MarkRead => Some(true),
            Self::MarkUnread => Some(false),
            _ => None,
        }
    }

    pub fn flagged_state(&self) -> Option<bool> {
        match self {
            Self::Star => Some(true),
            Self::Unstar => Some(false),
            _ => None,
        }
    }

    pub fn title(&self, bulk: bool) -> &'static str {
        match (self, bulk) {
            (Self::MarkRead, true) => "Mark all as read",
            (Self::MarkRead, false) => "Mark as read",
            (Self::MarkUnread, true) => "Mark all as unread",
            (Self::MarkUnread, false) => "Mark as unread",
            (Self::Star, true) => "Star all",
            (Self::Star, false) => "Star",
            (Self::Unstar, true) => "Unstar all",
            (Self::Unstar, false) => "Unstar",
            (Self::MoveTo(_), _) => "Move to…",
            (Self::Restore, _) => "Restore from Trash",
            (Self::Unspam, _) => "Not spam",
            (Self::Spam, _) => "Mark as spam",
            (Self::Archive, true) => "Archive all",
            (Self::Archive, false) => "Archive",
            (Self::Delete, true) => "Delete all",
            (Self::Delete, false) => "Delete",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::MarkRead => "brevlada-mail-read-symbolic",
            Self::MarkUnread => "mail-unread-symbolic",
            Self::Star => "starred-symbolic",
            Self::Unstar => "non-starred-symbolic",
            Self::MoveTo(_) => "folder-symbolic",
            Self::Restore => "edit-undo-symbolic",
            Self::Unspam => "mail-mark-notjunk-symbolic",
            Self::Spam => "mail-mark-junk-symbolic",
            Self::Archive => "package-x-generic-symbolic",
            Self::Delete => "user-trash-symbolic",
        }
    }
}
