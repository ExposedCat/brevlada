use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub trust_all_senders: bool,
    pub background_service: bool,
    pub start_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trust_all_senders: false,
            background_service: true,
            start_at_login: true,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(data) => Ok(serde_json::from_slice(&data)?),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        gtk::glib::file_set_contents(path, &serde_json::to_vec(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_defaults_on_and_preserves_saved_preferences() {
        let mut settings: Settings = serde_json::from_str("{\"trust_all_senders\":true}").unwrap();
        assert!(settings.background_service && settings.start_at_login);
        settings.background_service = false;
        settings.start_at_login = false;
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(!restored.background_service && !restored.start_at_login);
        assert!(restored.trust_all_senders);
        let previous: Settings = serde_json::from_str("{\"background_service\":false}").unwrap();
        assert!(!previous.background_service && previous.start_at_login);
    }

    #[test]
    fn trust_all_senders_defaults_off_and_persists_both_states() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-settings-{}",
            gtk::glib::uuid_string_random()
        ));
        let path = directory.join("settings.json");
        assert!(!Settings::load(&path).unwrap().trust_all_senders);
        assert!(
            !serde_json::from_str::<Settings>("{}")
                .unwrap()
                .trust_all_senders
        );
        for trust_all_senders in [true, false] {
            Settings {
                trust_all_senders,
                ..Settings::default()
            }
            .save(&path)
            .unwrap();
            assert_eq!(
                Settings::load(&path).unwrap().trust_all_senders,
                trust_all_senders
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
