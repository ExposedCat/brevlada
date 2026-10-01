use std::ffi::{OsStr, OsString};

pub fn open(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return;
    }
    if std::path::Path::new("/.flatpak-info").exists() {
        gtk::UriLauncher::new(url).launch(
            None::<&gtk::Window>,
            gtk::gio::Cancellable::NONE,
            |result| {
                if let Err(error) = result {
                    gtk::glib::g_warning!("brevlada", "Unable to open link: {error}");
                }
            },
        );
        return;
    }
    let arguments = command(url);
    let arguments: Vec<&OsStr> = arguments.iter().map(OsString::as_os_str).collect();
    match gtk::gio::Subprocess::newv(&arguments, gtk::gio::SubprocessFlags::NONE) {
        Ok(process) => process.wait_check_async(gtk::gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                gtk::glib::g_warning!("brevlada", "Unable to open link: {error}");
            }
        }),
        Err(error) => gtk::glib::g_warning!("brevlada", "Unable to open link: {error}"),
    }
}

fn command(url: &str) -> Vec<OsString> {
    if std::env::var_os("DISTROBOX_ENTER_PATH").is_some()
        && std::path::Path::new("/usr/bin/distrobox-host-exec").exists()
    {
        let mut arguments = vec![OsString::from("distrobox-host-exec"), OsString::from("env")];
        for key in ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"] {
            if let Some(value) = std::env::var_os(key) {
                let mut assignment = OsString::from(key);
                assignment.push("=");
                assignment.push(value);
                arguments.push(assignment);
            }
        }
        arguments.push(OsString::from("xdg-open"));
        arguments.push(OsString::from(url));
        arguments
    } else {
        vec![OsString::from("xdg-open"), OsString::from(url)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_graphical_session_when_running_in_distrobox() {
        let arguments = command("https://example.com");
        if std::env::var_os("DISTROBOX_ENTER_PATH").is_some() {
            assert_eq!(arguments[0], "distrobox-host-exec");
            for key in ["DISPLAY", "WAYLAND_DISPLAY"] {
                if let Some(value) = std::env::var_os(key) {
                    let mut assignment = OsString::from(key);
                    assignment.push("=");
                    assignment.push(value);
                    assert!(arguments.contains(&assignment));
                }
            }
        }
        assert_eq!(arguments.last().unwrap(), "https://example.com");
    }
}
