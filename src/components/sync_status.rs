use super::{horizontal, label};
use crate::{
    models::sync::{Count, Status},
    theme,
};
use adw::prelude::*;
use std::time::SystemTime;

pub struct SyncStatus {
    pub widget: gtk::Box,
    spinner: gtk::Spinner,
    label: gtk::Label,
    accounts: Counter,
    folders: Counter,
    messages: Counter,
}

impl SyncStatus {
    pub fn new() -> Self {
        let widget = horizontal("sync-status", theme::ROW_GAP);
        widget.set_halign(gtk::Align::Center);
        let spinner = gtk::Spinner::builder()
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let label = label("Last synced: —", "sync-status-label");
        label.set_max_width_chars(24);
        label.set_hexpand(false);
        widget.append(&spinner);
        widget.append(&label);
        let accounts = Counter::new("avatar-default-symbolic", "accounts");
        widget.append(&accounts.widget);
        let folders = Counter::new("folder-symbolic", "folders");
        let messages = Counter::new("mail-unread-symbolic", "messages");
        widget.append(&folders.widget);
        widget.append(&messages.widget);
        Self {
            widget,
            spinner,
            label,
            accounts,
            folders,
            messages,
        }
    }

    pub fn update(&self, status: &Status) {
        let (text, tooltip) = presentation(status, SystemTime::now());
        self.spinner.set_spinning(status.activity.is_some());
        self.spinner.set_visible(status.activity.is_some());
        if self.label.text() != text {
            self.label.set_label(&text);
        }
        self.accounts.update(
            status
                .activity
                .as_ref()
                .and_then(|activity| activity.progress.accounts.as_ref()),
        );
        self.folders.update(
            status
                .activity
                .as_ref()
                .and_then(|activity| activity.progress.folders.as_ref()),
        );
        self.messages.update(
            status
                .activity
                .as_ref()
                .and_then(|activity| activity.progress.messages.as_ref()),
        );
        self.widget.set_tooltip_text(Some(&tooltip));
    }
}

struct Counter {
    widget: gtk::Box,
    label: gtk::Label,
    kind: &'static str,
}

impl Counter {
    fn new(icon: &str, kind: &'static str) -> Self {
        let widget = horizontal("sync-counter", theme::ROW_GAP);
        widget.set_visible(false);
        widget.append(&label("·", "dim-label"));
        let count = label("", "sync-counter-label");
        count.set_hexpand(false);
        count.set_ellipsize(gtk::pango::EllipsizeMode::None);
        widget.append(&count);
        widget.append(&gtk::Image::from_icon_name(icon));
        Self {
            widget,
            label: count,
            kind,
        }
    }

    fn update(&self, count: Option<&Count>) {
        self.widget.set_visible(count.is_some());
        if let Some(count) = count {
            self.label
                .set_label(&format!("{}/{}", count.cached, count.total));
            self.widget.set_tooltip_text(Some(&format!(
                "{} of {} {} cached for this sync",
                count.cached, count.total, self.kind
            )));
        }
    }
}

fn presentation(status: &Status, now: SystemTime) -> (String, String) {
    if let Some(activity) = &status.activity {
        let mut detail = activity.description.to_owned();
        if !activity.folder.is_empty() {
            detail.push_str(&format!("\n{}", activity.folder));
        }
        detail.push_str(&format!(
            "\n{} active, {} queued",
            status.active, status.queued
        ));
        if status.failed > 0 {
            detail.push_str(&format!(
                "\n{} failed; will retry on the next sync",
                status.failed
            ));
        }
        return ("Syncing".into(), detail);
    }
    let last = status.last_started.map(|at| {
        let date: chrono::DateTime<chrono::Local> = at.into();
        format!("Last sync started: {}", date.format("%b %d, %Y %H:%M:%S"))
    });
    let mut detail = last.unwrap_or_else(|| "No background sync has started yet.".into());
    if status.failed > 0 {
        detail.push_str(&format!(
            "\n{} sync requests failed. They will retry automatically.",
            status.failed
        ));
    }
    if let Some(at) = status.last_started {
        let seconds = now.duration_since(at).unwrap_or_default().as_secs();
        let ago = match seconds {
            0..60 => "just now".into(),
            60..3600 => format!("{}m ago", seconds / 60),
            3600..86400 => format!("{}h ago", seconds / 3600),
            _ => format!("{}d ago", seconds / 86400),
        };
        return (format!("Last synced: {ago}"), detail);
    }
    ("Last synced: —".into(), detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shows_elapsed_time_since_start_even_when_sync_has_failed() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100_000);
        let mut status = Status::default();
        assert_eq!(presentation(&status, now).0, "Last synced: —");
        for (seconds, expected) in [
            (0, "just now"),
            (59, "just now"),
            (60, "1m ago"),
            (7200, "2h ago"),
            (86400, "1d ago"),
        ] {
            status.last_started = Some(now - Duration::from_secs(seconds));
            assert_eq!(
                presentation(&status, now).0,
                format!("Last synced: {expected}")
            );
        }
        status.failed = 1;
        assert_eq!(presentation(&status, now).0, "Last synced: 1d ago");
        assert!(presentation(&status, now).1.contains("Last sync started"));
    }
}
