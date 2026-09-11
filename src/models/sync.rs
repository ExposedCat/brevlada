use std::time::SystemTime;

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub activity: Option<Activity>,
    pub active: usize,
    pub queued: usize,
    pub failed: usize,
    pub last_started: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct Activity {
    pub description: &'static str,
    pub folder: String,
    pub progress: Progress,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub accounts: Option<Count>,
    pub folders: Option<Count>,
    pub messages: Option<Count>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
    pub cached: usize,
    pub total: usize,
}
