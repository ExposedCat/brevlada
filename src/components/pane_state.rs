use crate::theme;
use adw::prelude::*;
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::Path};

#[derive(Deserialize, Serialize)]
struct PaneState {
    sidebar_fraction: f64,
    messages_fraction: f64,
    #[serde(default)]
    maximized: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedState {
    Proportions(PaneState),
    Pixels { sidebar: i32, messages: i32 },
}

impl Default for PaneState {
    fn default() -> Self {
        Self::from_pixels(theme::SIDEBAR_WIDTH, theme::LIST_WIDTH)
    }
}

impl PaneState {
    fn from_pixels(sidebar: i32, messages: i32) -> Self {
        Self {
            sidebar_fraction: f64::from(sidebar) / f64::from(theme::WINDOW_WIDTH),
            messages_fraction: f64::from(messages)
                / f64::from((theme::WINDOW_WIDTH - sidebar).max(1)),
            maximized: false,
        }
    }

    fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(data) => match serde_json::from_slice(&data)? {
                SavedState::Proportions(state) => Ok(state),
                SavedState::Pixels { sidebar, messages } => {
                    Ok(Self::from_pixels(sidebar, messages))
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
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

fn available_width(pane: &gtk::Paned) -> i32 {
    [pane.start_child(), pane.end_child()]
        .into_iter()
        .flatten()
        .filter_map(|child| child.compute_bounds(pane))
        .map(|bounds| bounds.width().round() as i32)
        .sum()
}

fn restore(pane: &gtk::Paned, fraction: f64) {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return;
    }
    pane.add_tick_callback(move |pane, _| {
        if apply_fraction(pane, fraction) {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn apply_fraction(pane: &gtk::Paned, fraction: f64) -> bool {
    let width = available_width(pane);
    if width <= 0 {
        return false;
    }
    pane.set_position((fraction * f64::from(width)).round() as i32);
    true
}

pub fn remember(window: &adw::ApplicationWindow, sidebar: &gtk::Paned, messages: &gtk::Paned) {
    let path = glib::user_config_dir().join("brevlada/layout.json");
    let state = PaneState::load(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore window layout: {error}");
        PaneState::default()
    });
    if state.maximized {
        window.maximize();
    }
    restore(sidebar, state.sidebar_fraction);
    restore(messages, state.messages_fraction);
    let sidebar = sidebar.downgrade();
    let messages = messages.downgrade();
    window.connect_close_request(move |window| {
        if let (Some(sidebar), Some(messages)) = (sidebar.upgrade(), messages.upgrade()) {
            let sidebar_width = available_width(&sidebar);
            let messages_width = available_width(&messages);
            if sidebar_width > 0 && messages_width > 0 {
                let state = PaneState {
                    sidebar_fraction: f64::from(sidebar.position()) / f64::from(sidebar_width),
                    messages_fraction: f64::from(messages.position()) / f64::from(messages_width),
                    maximized: window.is_maximized(),
                };
                if let Err(error) = state.save(&path) {
                    eprintln!("Could not save window layout: {error}");
                }
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
    fn restores_proportions_at_different_sizes_and_keeps_them_on_resize() {
        gtk::init().unwrap();
        for initial_width in [1400, 1920] {
            let messages = pane(&column("diagnostic"), &column("diagnostic"), 400);
            let sidebar = pane(&column("diagnostic"), &messages, 300);
            sidebar.allocate(initial_width, 900, -1, None);
            assert!(apply_fraction(&sidebar, 0.25));
            assert!(apply_fraction(&messages, 0.45));
            for width in [initial_width, 1920, 1200, 1400] {
                sidebar.allocate(width, 900, -1, None);
                for (pane, fraction) in [(&sidebar, 0.25), (&messages, 0.45)] {
                    let expected = f64::from(available_width(pane)) * fraction;
                    assert!((f64::from(pane.position()) - expected).abs() <= 2.0);
                }
            }
        }
    }
}
