#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenderAction {
    MarkRead,
    Spam,
    Archive,
    Delete,
}

impl SenderAction {
    pub const ALL: [(Self, &'static str); 4] = [
        (Self::MarkRead, "Mark all as read"),
        (Self::Spam, "Mark as spam"),
        (Self::Archive, "Archive all"),
        (Self::Delete, "Delete all"),
    ];
}
