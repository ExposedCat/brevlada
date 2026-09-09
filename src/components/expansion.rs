use adw::prelude::*;
use gtk::glib;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    path::PathBuf,
    rc::Rc,
};

#[derive(Clone, Default)]
pub struct Expansion {
    accounts: Rc<RefCell<BTreeMap<String, BTreeSet<String>>>>,
    path: Option<Rc<PathBuf>>,
    account: String,
}

impl Expansion {
    pub fn load() -> Self {
        Self::load_from(glib::user_config_dir().join("brevlada/expansion.json"))
    }

    fn load_from(path: PathBuf) -> Self {
        let accounts = (|| -> anyhow::Result<_> {
            match std::fs::read(&path) {
                Ok(data) => Ok(serde_json::from_slice(&data)?),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(BTreeMap::new()),
                Err(error) => Err(error.into()),
            }
        })()
        .unwrap_or_else(|error| {
            eprintln!("Could not restore expanded folders: {error}");
            BTreeMap::new()
        });
        Self {
            accounts: Rc::new(RefCell::new(accounts)),
            path: Some(Rc::new(path)),
            account: String::new(),
        }
    }

    pub fn for_account(&self, account: &str) -> Self {
        Self {
            account: account.into(),
            ..self.clone()
        }
    }

    pub fn is_expanded(&self, folder: &str) -> bool {
        self.accounts
            .borrow()
            .get(&self.account)
            .is_some_and(|folders| folders.contains(folder))
    }

    pub fn bind(&self, folder: &str, children: &gtk::Box) {
        children.set_visible(self.is_expanded(folder));
        let state = self.clone();
        let folder = folder.to_owned();
        children.connect_visible_notify(move |children| {
            state.set(&folder, children.get_visible());
        });
    }

    fn set(&self, folder: &str, expanded: bool) {
        let mut accounts = self.accounts.borrow_mut();
        let folders = accounts.entry(self.account.clone()).or_default();
        if expanded {
            folders.insert(folder.into());
        } else {
            folders.remove(folder);
        }
        if let Some(path) = &self.path {
            let result = (|| -> anyhow::Result<()> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                glib::file_set_contents(path.as_ref(), &serde_json::to_vec(&*accounts)?)?;
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Could not save expanded folders: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_accounts_and_nested_folders_independently() {
        let directory =
            std::env::temp_dir().join(format!("brevlada-expansion-{}", glib::uuid_string_random()));
        let path = directory.join("expansion.json");
        let state = Expansion::load_from(path.clone());
        let first = state.for_account("first");
        first.set("", true);
        first.set("Work", true);
        first.set("Work/Projects", true);
        state.for_account("second").set("Work", true);
        first.set("", false);
        first.set("Work", false);
        let restored = Expansion::load_from(path);
        let first = restored.for_account("first");
        assert!(!first.is_expanded(""));
        assert!(!first.is_expanded("Work"));
        assert!(first.is_expanded("Work/Projects"));
        assert!(restored.for_account("second").is_expanded("Work"));
        assert!(!restored.for_account("second").is_expanded("Work/Projects"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
