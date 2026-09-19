use crate::theme;
use adw::prelude::*;
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::{cell::Cell, io::ErrorKind, path::Path, rc::Rc};

#[derive(Deserialize, Serialize)]
struct PaneState {
    sidebar_width: i32,
    sender_width: i32,
    thread_width: i32,
    #[serde(default)]
    maximized: bool,
    #[serde(default)]
    sidebar_collapsed: bool,
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
            sidebar_collapsed: false,
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
                    sidebar_collapsed,
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

// Keep logical pixel widths: only the viewer absorbs changes in available space.
// Restoring a fraction of a newly enlarged parent would grow a reopened sidebar.
fn remember_width(pane: &gtk::Paned, initial: i32) -> Rc<Cell<i32>> {
    let width = Rc::new(Cell::new(initial.max(1)));
    pane.set_position(width.get());
    if let Some(child) = pane.start_child() {
        let weak = pane.downgrade();
        let saved = width.clone();
        child.connect_visible_notify(move |child| {
            if let Some(pane) = weak.upgrade() {
                if child.get_visible() {
                    pane.set_position(saved.get());
                } else {
                    // Capture the allocated width before the hidden layout is applied.
                    saved.set(pane.position().max(1));
                }
            }
        });
    }
    width
}

fn current_width(pane: &gtk::Paned, saved: i32) -> i32 {
    if pane.start_child().is_some_and(|child| child.get_visible()) {
        pane.position().max(1)
    } else {
        saved
    }
}

pub fn remember(
    window: &adw::ApplicationWindow,
    sidebar: &gtk::Paned,
    messages: &gtk::Paned,
    threads: &gtk::Paned,
) {
    let path = glib::user_config_dir().join("brevlada/layout.json");
    let state = PaneState::load(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore window layout: {error}");
        PaneState::default()
    });
    if state.maximized {
        window.maximize();
    }
    if let Some(child) = sidebar.start_child() {
        child.set_visible(!state.sidebar_collapsed);
    }
    let sidebar_width = remember_width(sidebar, state.sidebar_width);
    let sender_width = remember_width(messages, state.sender_width);
    let thread_width = remember_width(threads, state.thread_width);
    let sidebar = sidebar.downgrade();
    let messages = messages.downgrade();
    let threads = threads.downgrade();
    window.connect_close_request(move |window| {
        if let (Some(sidebar), Some(messages), Some(threads)) =
            (sidebar.upgrade(), messages.upgrade(), threads.upgrade())
        {
            let state = PaneState {
                sidebar_width: current_width(&sidebar, sidebar_width.get()),
                sender_width: current_width(&messages, sender_width.get()),
                thread_width: current_width(&threads, thread_width.get()),
                maximized: window.is_maximized(),
                sidebar_collapsed: sidebar
                    .start_child()
                    .is_some_and(|child| !child.get_visible()),
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
        let accounts = column("diagnostic");
        let senders = column("diagnostic");
        let messages = column("diagnostic");
        let viewer = column("diagnostic");
        let threads = pane(&messages, &viewer, 350);
        let content = pane(&senders, &threads, 300);
        let main = pane(&accounts, &content, 250);
        let account_width = remember_width(&main, 250);
        let thread_width = remember_width(&threads, 350);
        remember_width(&content, 300);
        main.allocate(1600, 900, -1, None);
        let initial_viewer = viewer.width();
        for _ in 0..20 {
            accounts.set_visible(false);
            main.allocate(1600, 900, -1, None);
            assert_eq!(senders.width(), 300);
            assert_eq!(messages.width(), 350);
            assert!(viewer.width() >= initial_viewer + 250);
            assert_eq!(current_width(&main, account_width.get()), 250);
            accounts.set_visible(true);
            main.allocate(1600, 900, -1, None);
            assert_eq!(accounts.width(), 250);
            assert_eq!(senders.width(), 300);
            assert_eq!(messages.width(), 350);
            assert_eq!(viewer.width(), initial_viewer);
            messages.set_visible(false);
            main.allocate(1600, 900, -1, None);
            assert_eq!(senders.width(), 300);
            assert_eq!(current_width(&threads, thread_width.get()), 350);
            messages.set_visible(true);
            main.allocate(1600, 900, -1, None);
            assert_eq!(messages.width(), 350);
            assert_eq!(viewer.width(), initial_viewer);
        }
        main.set_position(280);
        threads.set_position(370);
        main.allocate(1600, 900, -1, None);
        accounts.set_visible(false);
        messages.set_visible(false);
        main.allocate(1900, 900, -1, None);
        accounts.set_visible(true);
        messages.set_visible(true);
        main.allocate(1900, 900, -1, None);
        assert_eq!(accounts.width(), 280);
        assert_eq!(senders.width(), 300);
        assert_eq!(messages.width(), 370);
    }
}
