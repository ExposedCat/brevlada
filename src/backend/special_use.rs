use anyhow::{Context, Result, ensure};
use imap::{Session, types::NameAttribute};
use std::io::{Read, Write};

pub(super) fn quoted(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "Folder name contains invalid characters"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub(super) fn marked(attributes: &[NameAttribute<'_>], role: &str) -> bool {
    attributes.iter().any(|attribute| matches!(attribute, NameAttribute::Custom(value) if value.eq_ignore_ascii_case(role)))
}

pub(super) fn can_share<T: Read + Write>(session: &mut Session<T>) -> Result<bool> {
    let capabilities = session.capabilities()?;
    Ok(capabilities.has_str("METADATA") && capabilities.has_str("SPECIAL-USE"))
}

pub(super) fn share_archive<T: Read + Write>(session: &mut Session<T>, folder: &str) -> Result<()> {
    ensure!(
        can_share(session)?,
        "This server cannot share archive folder changes with other mail apps"
    );
    let folders = session.list(None, Some("\"*\" RETURN (SPECIAL-USE)"))?;
    let target = folders
        .iter()
        .find(|name| name.name() == folder && !name.attributes().contains(&NameAttribute::NoSelect))
        .context("The selected archive folder is no longer available")?;
    ensure!(
        !folder.eq_ignore_ascii_case("INBOX"),
        "Inbox cannot be the archive folder"
    );
    let mut roles = folder_roles(target.attributes())?;
    if !roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case("\\Archive"))
    {
        roles.push("\\Archive".into());
        set(session, folder, &roles)?;
    }
    let folders = session.list(None, Some("\"*\" RETURN (SPECIAL-USE)"))?;
    ensure!(
        folders
            .iter()
            .any(|name| name.name() == folder && marked(name.attributes(), "\\Archive")),
        "The server did not assign the archive role to the selected folder"
    );
    for previous in folders
        .iter()
        .filter(|name| name.name() != folder && marked(name.attributes(), "\\Archive"))
    {
        let mut roles = folder_roles(previous.attributes())?;
        roles.retain(|role| !role.eq_ignore_ascii_case("\\Archive"));
        set(session, previous.name(), &roles).context("The new archive folder was assigned, but the previous folder could not be updated. Reload account settings to check the server state")?;
    }
    let folders = session.list(None, Some("\"*\" RETURN (SPECIAL-USE)"))?;
    let archives: Vec<_> = folders
        .iter()
        .filter(|name| marked(name.attributes(), "\\Archive"))
        .map(|name| name.name())
        .collect();
    ensure!(
        archives == [folder],
        "The server did not keep a unique archive folder. Reload account settings to check the server state"
    );
    Ok(())
}

fn folder_roles(attributes: &[NameAttribute<'_>]) -> Result<Vec<String>> {
    let mut roles = Vec::new();
    for attribute in attributes {
        if let NameAttribute::Custom(value) = attribute {
            match value.to_ascii_lowercase().as_str() {
                "\\all" | "\\archive" | "\\drafts" | "\\flagged" | "\\junk" | "\\sent"
                | "\\trash" | "\\important" => roles.push(value.to_string()),
                "\\haschildren" | "\\hasnochildren" | "\\subscribed" | "\\remote"
                | "\\nonexistent" => {}
                _ => anyhow::bail!(
                    "Cannot safely preserve an unknown folder role reported by this server: {value}"
                ),
            }
        }
    }
    Ok(roles)
}

fn set<T: Read + Write>(session: &mut Session<T>, folder: &str, roles: &[String]) -> Result<()> {
    let value = if roles.is_empty() {
        "NIL".into()
    } else {
        quoted(&roles.join(" "))?
    };
    session.run_command_and_check_ok(format!(
        "SETMETADATA {} (/private/specialuse {value})",
        quoted(folder)?
    ))?;
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
    };

    #[derive(Debug)]
    pub(in crate::backend) struct Stream {
        response: Cursor<Vec<u8>>,
        commands: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for Stream {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.response.read(bytes)
        }
    }

    impl Write for Stream {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.commands.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub(in crate::backend) fn session(response: &str) -> (Session<Stream>, Arc<Mutex<Vec<u8>>>) {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let stream = Stream {
            response: Cursor::new(format!("a1 OK login\r\n{response}").into_bytes()),
            commands: commands.clone(),
        };
        (
            imap::Client::new(stream)
                .login("fixture", "fixture")
                .unwrap(),
            commands,
        )
    }

    #[test]
    fn shares_and_verifies_archive_without_removing_other_roles() {
        let (mut session, commands) = session(concat!(
            "* CAPABILITY IMAP4rev1 SPECIAL-USE METADATA\r\na2 OK caps\r\n",
            "* LIST (\\Flagged) \"/\" New\r\n* LIST (\\Archive \\Sent) \"/\" Old\r\na3 OK list\r\n",
            "a4 OK set\r\n",
            "* LIST (\\Archive \\Flagged) \"/\" New\r\n* LIST (\\Archive \\Sent) \"/\" Old\r\na5 OK list\r\n",
            "a6 OK set\r\n",
            "* LIST (\\Archive \\Flagged) \"/\" New\r\n* LIST (\\Sent) \"/\" Old\r\na7 OK list\r\n",
        ));
        share_archive(&mut session, "New").unwrap();
        let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
        assert!(
            sent.contains("SETMETADATA \"New\" (/private/specialuse \"\\\\Flagged \\\\Archive\")"),
            "{sent}"
        );
        assert!(
            sent.contains("SETMETADATA \"Old\" (/private/specialuse \"\\\\Sent\")"),
            "{sent}"
        );
        assert!(!sent.contains("CREATE") && !sent.contains("UID "));
    }

    #[test]
    fn respects_server_rejections_and_requires_list_verification() {
        for tail in [
            "a4 NO forbidden\r\n",
            "a4 OK ignored\r\n* LIST () \"/\" New\r\na5 OK list\r\n",
        ] {
            let (mut session, commands) = session(&format!(
                "* CAPABILITY IMAP4rev1 SPECIAL-USE METADATA\r\na2 OK caps\r\n* LIST () \"/\" New\r\na3 OK list\r\n{tail}"
            ));
            assert!(share_archive(&mut session, "New").is_err());
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert_eq!(sent.matches("SETMETADATA").count(), 1);
            assert!(!sent.contains("Old"));
        }
    }

    #[test]
    fn unsupported_servers_are_never_sent_metadata_writes() {
        for caps in ["SPECIAL-USE", "SPECIAL-USE METADATA-SERVER", "METADATA"] {
            let (mut session, commands) =
                session(&format!("* CAPABILITY IMAP4rev1 {caps}\r\na2 OK caps\r\n"));
            assert!(share_archive(&mut session, "New").is_err());
            let sent = String::from_utf8(commands.lock().unwrap().clone()).unwrap();
            assert!(!sent.contains("GETMETADATA") && !sent.contains("SETMETADATA"));
        }
        assert_eq!(quoted("A\"B\\C").unwrap(), "\"A\\\"B\\\\C\"");
        for name in ["Bad\r\nFolder", "Bad\0Folder"] {
            assert!(quoted(name).is_err());
        }
    }
}
