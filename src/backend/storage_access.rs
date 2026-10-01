use anyhow::{Result, anyhow};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

#[derive(Default)]
pub(super) struct State {
    pub initialized: bool,
}

/// All connections to a cache share this gate. Hold it only for database writes,
/// including initialization, never while fetching mail or sending UI events.
#[derive(Default)]
pub(super) struct Access(Mutex<State>);

impl Access {
    pub fn for_path(path: &Path) -> Result<Arc<Self>> {
        // Each in-memory connection is a separate database.
        if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            return Ok(Arc::new(Self::default()));
        }
        let key = if path.exists() {
            path.canonicalize()?
        } else {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
            parent
                .unwrap_or_else(|| Path::new("."))
                .canonicalize()?
                .join(
                    path.file_name()
                        .ok_or_else(|| anyhow!("Invalid cache path"))?,
                )
        };
        static CACHES: OnceLock<Mutex<HashMap<PathBuf, Weak<Access>>>> = OnceLock::new();
        let mut caches = CACHES
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| anyhow!("Cache connection registry was poisoned"))?;
        caches.retain(|_, access| access.strong_count() > 0);
        if let Some(access) = caches.get(&key).and_then(Weak::upgrade) {
            return Ok(access);
        }
        let access = Arc::new(Self::default());
        caches.insert(key, Arc::downgrade(&access));
        Ok(access)
    }

    pub fn initialized() -> Arc<Self> {
        Arc::new(Self(Mutex::new(State { initialized: true })))
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.0
            .lock()
            .map_err(|_| anyhow!("Cache write coordination was poisoned"))
    }
}
