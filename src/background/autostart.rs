use anyhow::Result;
use gtk::glib;

pub fn set(enabled: bool) -> Result<()> {
    let directory = glib::user_config_dir().join("autostart");
    let path = directory.join("io.github.ExposedCat.Brevlada.desktop");
    if !enabled {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        };
    }
    let executable = std::env::current_exe()?;
    let desktop = entry(&executable.to_string_lossy());
    std::fs::create_dir_all(directory)?;
    glib::file_set_contents(path, desktop.to_data().as_bytes())?;
    Ok(())
}

fn entry(executable: &str) -> glib::KeyFile {
    let executable = executable
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    let desktop = glib::KeyFile::new();
    desktop.set_string("Desktop Entry", "Type", "Application");
    desktop.set_string("Desktop Entry", "Name", "Brevlada");
    desktop.set_string(
        "Desktop Entry",
        "Exec",
        &format!("\"{executable}\" --background"),
    );
    desktop.set_string("Desktop Entry", "Icon", "io.github.ExposedCat.Brevlada");
    desktop.set_boolean("Desktop Entry", "Terminal", false);
    desktop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_is_headless_and_quotes_executable_paths() {
        let desktop = entry("/tmp/Mail App/brevlada");
        assert_eq!(
            desktop.string("Desktop Entry", "Exec").unwrap(),
            "\"/tmp/Mail App/brevlada\" --background"
        );
        let desktop = entry("/tmp/$Mail%\\\"`/brevlada");
        let restored = glib::KeyFile::new();
        restored
            .load_from_data(&desktop.to_data(), glib::KeyFileFlags::NONE)
            .unwrap();
        assert_eq!(
            restored.string("Desktop Entry", "Exec").unwrap(),
            desktop.string("Desktop Entry", "Exec").unwrap()
        );
    }
}
