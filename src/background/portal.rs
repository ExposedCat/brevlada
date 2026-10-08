use anyhow::{Context, Result, ensure};
use glib::variant::ToVariant;
use gtk::{gio, glib};
use std::collections::HashMap;

const SERVICE: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.Background";

pub async fn request() -> Result<()> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    let token = format!("brevlada_{}", glib::uuid_string_random().replace('-', "_"));
    let sender = bus
        .unique_name()
        .context("Missing session-bus identity")?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let (response, results) = async_channel::bounded(1);
    let _subscription = bus.subscribe_to_signal(
        Some(SERVICE),
        Some("org.freedesktop.portal.Request"),
        Some("Response"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let _ = response.try_send(signal.parameters.clone());
        },
    );
    let options = HashMap::from([
        ("handle_token", token.to_variant()),
        (
            "reason",
            "Check for new mail and notify you when Brevlada is closed".to_variant(),
        ),
        ("autostart", false.to_variant()),
    ]);
    let reply = bus
        .call_future(
            Some(SERVICE),
            PATH,
            INTERFACE,
            "RequestBackground",
            Some(&("", options).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            30_000,
        )
        .await?;
    let (handle,) = reply
        .get::<(glib::variant::ObjectPath,)>()
        .context("Invalid background request")?;
    ensure!(
        handle.as_str() == path,
        "Unexpected background request path"
    );
    validate_response(&results.recv().await?)
}

fn validate_response(result: &glib::Variant) -> Result<()> {
    let (code, values) = result
        .get::<(u32, HashMap<String, glib::Variant>)>()
        .context("Invalid background response")?;
    ensure!(code == 0, "Background request was cancelled or denied");
    ensure!(
        values
            .get("autostart")
            .and_then(|value| value.get::<bool>())
            == Some(false),
        "Login startup was not disabled"
    );
    ensure!(
        values
            .get("background")
            .and_then(|value| value.get::<bool>())
            == Some(true),
        "Background access was denied"
    );
    Ok(())
}

pub async fn status() -> Result<()> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    let properties = bus
        .call_future(
            Some(SERVICE),
            PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(INTERFACE, "version").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            30_000,
        )
        .await?;
    let (version,) = properties
        .get::<(glib::Variant,)>()
        .context("Invalid background portal version")?;
    if version.get::<u32>().is_some_and(|version| version >= 2) {
        let options = HashMap::from([("message", "Checking for new mail".to_variant())]);
        bus.call_future(
            Some(SERVICE),
            PATH,
            INTERFACE,
            "SetStatus",
            Some(&(options,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            30_000,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(code: u32, background: bool, autostart: bool) -> glib::Variant {
        (
            code,
            HashMap::from([
                ("background", background.to_variant()),
                ("autostart", autostart.to_variant()),
            ]),
        )
            .to_variant()
    }

    #[test]
    fn background_permission_requires_login_startup_to_stay_disabled() {
        assert!(validate_response(&response(0, true, false)).is_ok());
        assert!(validate_response(&response(0, false, false)).is_err());
        assert!(validate_response(&response(0, true, true)).is_err());
        assert!(validate_response(&response(1, true, false)).is_err());
    }
}
