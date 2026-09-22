use crate::theme;
use adw::prelude::*;
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::Path};

#[derive(Deserialize, Serialize)]
struct PaneState {
    sidebar_width: i32,
    sender_width: i32,
    thread_width: i32,
    #[serde(default)]
    maximized: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedState {
    Widths(PaneState),
    Proportions {
        sidebar_fraction: f64,
        messages_fraction: f64,
        threads_fraction: Option<f64>,
        #[serde(default)]
        maximized: bool,
        #[serde(default)]
        sidebar_collapsed: bool,
    },
    Pixels {
        sidebar: i32,
        messages: i32,
    },
}

impl Default for PaneState {
    fn default() -> Self {
        Self {
            sidebar_width: theme::SIDEBAR_WIDTH,
            sender_width: theme::LIST_WIDTH,
            thread_width: theme::LIST_WIDTH,
            maximized: false,
        }
    }
}

impl PaneState {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        Ok(match serde_json::from_slice(&data)? {
            SavedState::Widths(state) => state,
            SavedState::Pixels { sidebar, messages } => Self {
                sidebar_width: sidebar,
                sender_width: messages,
                ..Self::default()
            },
            SavedState::Proportions {
                sidebar_fraction,
                messages_fraction,
                threads_fraction,
                maximized,
                sidebar_collapsed,
            } => {
                // Legacy layouts only recorded fractions, not the window size.
                let sidebar_width =
                    (sidebar_fraction * f64::from(theme::WINDOW_WIDTH)).round() as i32;
                let remaining =
                    theme::WINDOW_WIDTH - if sidebar_collapsed { 0 } else { sidebar_width };
                let sender_width = (messages_fraction * f64::from(remaining)).round() as i32;
                let thread_width = threads_fraction
                    .map(|fraction| (fraction * f64::from(remaining - sender_width)).round() as i32)
                    .unwrap_or(theme::LIST_WIDTH);
                Self {
                    sidebar_width,
                    sender_width,
                    thread_width,
                    maximized,
                }
            }
        })
    }

    fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        glib::file_set_contents(path, &serde_json::to_vec(self)?)?;
        Ok(())
    }
}

pub fn remember(
    window: &adw::ApplicationWindow,
    sidebar: &super::motion::Sidebar,
    messages: &gtk::Paned,
    threads: &super::motion::Sidebar,
) {
    let path = glib::user_config_dir().join("brevlada/layout.json");
    let state = PaneState::load(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore window layout: {error}");
        PaneState::default()
    });
    if state.maximized {
        window.maximize();
    }
    sidebar.restore_width(state.sidebar_width);
    messages.set_position(state.sender_width.max(1));
    threads.restore_width(state.thread_width);
    let sidebar = sidebar.clone();
    let messages = messages.downgrade();
    let threads = threads.clone();
    window.connect_close_request(move |window| {
        if let Some(messages) = messages.upgrade() {
            let state = PaneState {
                sidebar_width: sidebar.width(),
                sender_width: messages.position().max(1),
                thread_width: threads.width(),
                maximized: window.is_maximized(),
            };
            if let Err(error) = state.save(&path) {
                eprintln!("Could not save window layout: {error}");
            }
        }
        glib::Propagation::Proceed
    });
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use crate::components::{column, pane};

    #[test]
    #[ignore = "Requires a graphical session; allocates widgets without presenting a window"]
    fn toggling_sidebars_preserves_widths_and_gives_space_to_viewer() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let accounts = column("diagnostic");
        let senders = column("diagnostic");
        let messages = column("diagnostic");
        let viewer = column("diagnostic");
        let threads = super::super::motion::Sidebar::new(&messages, &viewer, 350, true);
        let content = pane(&senders, threads.pane(), 300);
        let account_pane = super::super::motion::Sidebar::new(&accounts, &content, 250, true);
        let main = account_pane.pane();
        main.allocate(1600, 900, -1, None);
        let initial_viewer = viewer.width();
        for _ in 0..20 {
            account_pane.set_visible(false);
            main.allocate(1600, 900, -1, None);
            assert_eq!(senders.width(), 300);
            assert_eq!(messages.width(), 350);
            assert!(viewer.width() >= initial_viewer + 250);
            assert_eq!(account_pane.width(), 250);
            account_pane.set_visible(true);
            main.allocate(1600, 900, -1, None);
            assert_eq!(accounts.width(), 250);
            assert_eq!(senders.width(), 300);
            assert_eq!(messages.width(), 350);
            assert_eq!(viewer.width(), initial_viewer);
            threads.set_visible(false);
            main.allocate(1600, 900, -1, None);
            assert_eq!(senders.width(), 300);
            assert_eq!(threads.width(), 350);
            threads.set_visible(true);
            main.allocate(1600, 900, -1, None);
            assert_eq!(messages.width(), 350);
            assert_eq!(viewer.width(), initial_viewer);
        }
        main.set_position(280);
        threads.pane().set_position(370);
        main.allocate(1600, 900, -1, None);
        account_pane.set_visible(false);
        threads.set_visible(false);
        main.allocate(1900, 900, -1, None);
        account_pane.set_visible(true);
        threads.set_visible(true);
        main.allocate(1900, 900, -1, None);
        assert_eq!(accounts.width(), 280);
        assert_eq!(senders.width(), 300);
        assert_eq!(messages.width(), 370);
    }
}
