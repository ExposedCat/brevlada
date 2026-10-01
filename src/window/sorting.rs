use super::*;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct SortPreferences {
    senders_unread_first: bool,
    messages_unread_first: bool,
}

impl Default for SortPreferences {
    fn default() -> Self {
        Self {
            senders_unread_first: true,
            messages_unread_first: true,
        }
    }
}

impl SortPreferences {
    fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(data) => Ok(serde_json::from_slice(&data)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        glib::file_set_contents(path, &serde_json::to_vec(self)?)?;
        Ok(())
    }
}

impl State {
    pub(super) fn remember_sorting(self: &Rc<Self>, path: PathBuf) {
        let preferences = SortPreferences::load(&path).unwrap_or_else(|error| {
            eprintln!("Could not restore sorting options: {error}");
            SortPreferences::default()
        });
        self.sender_unread_first
            .set_unread_first(preferences.senders_unread_first);
        self.thread_unread_first
            .set_unread_first(preferences.messages_unread_first);
        for menu in [&self.sender_unread_first, &self.thread_unread_first] {
            let weak = Rc::downgrade(self);
            let path = path.clone();
            menu.connect_changed(move || {
                if let Some(state) = weak.upgrade() {
                    let preferences = SortPreferences {
                        senders_unread_first: state.sender_unread_first.unread_first(),
                        messages_unread_first: state.thread_unread_first.unread_first(),
                    };
                    if let Err(error) = preferences.save(&path) {
                        eprintln!("Could not save sorting options: {error}");
                        state
                            .toast
                            .add_toast(adw::Toast::new("Could not save sorting options"));
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sort_preferences_default_to_unread_first() {
        let path =
            std::env::temp_dir().join(format!("brevlada-sort-{}.json", glib::uuid_string_random()));
        let preferences = SortPreferences::load(&path).unwrap();
        assert!(preferences.senders_unread_first && preferences.messages_unread_first);
        let partial: SortPreferences =
            serde_json::from_str(r#"{"senders_unread_first":false}"#).unwrap();
        assert!(!partial.senders_unread_first && partial.messages_unread_first);
    }
}
