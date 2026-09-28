use crate::models::{Account, SmtpSettings};
use anyhow::{Context, Result};
use gtk::{gio, glib};
use std::collections::HashMap;

const SERVICE: &str = "org.gnome.OnlineAccounts";
const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
const MAIL: &str = "org.gnome.OnlineAccounts.Mail";
const OAUTH: &str = "org.gnome.OnlineAccounts.OAuth2Based";
type Properties = HashMap<String, glib::Variant>;
type Objects = HashMap<glib::variant::ObjectPath, HashMap<String, Properties>>;

fn call(
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
) -> Result<glib::Variant> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    Ok(bus.call_sync(
        Some(SERVICE),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::NONE,
        30_000,
        gio::Cancellable::NONE,
    )?)
}

pub fn discover() -> Result<Vec<Account>> {
    let reply = call(
        "/org/gnome/OnlineAccounts",
        "org.freedesktop.DBus.ObjectManager",
        "GetManagedObjects",
        None,
    )?;
    let (objects,) = reply
        .get::<(Objects,)>()
        .context("Invalid Online Accounts response")?;
    let mut result = Vec::new();
    for (path, interfaces) in objects {
        let (Some(account), Some(mail)) = (interfaces.get(ACCOUNT), interfaces.get(MAIL)) else {
            continue;
        };
        if account
            .get("MailDisabled")
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false)
        {
            continue;
        }
        let string = |props: &Properties, key: &str| {
            props
                .get(key)
                .and_then(|v| v.str())
                .unwrap_or("")
                .to_owned()
        };
        let boolean = |key: &str| mail.get(key).and_then(|v| v.get::<bool>()).unwrap_or(false);
        let email = string(mail, "EmailAddress");
        let username = string(mail, "ImapUserName");
        let ssl = boolean("ImapUseSsl");
        let smtp_ssl = boolean("SmtpUseSsl");
        let smtp = if boolean("SmtpSupported") {
            let (host, port) = smtp_endpoint(&string(mail, "SmtpHost"), smtp_ssl);
            Some(SmtpSettings {
                host,
                port,
                username: string(mail, "SmtpUserName"),
                ssl: smtp_ssl,
                tls: boolean("SmtpUseTls"),
                auth: boolean("SmtpUseAuth"),
                login: boolean("SmtpAuthLogin"),
                plain: boolean("SmtpAuthPlain"),
                xoauth2: boolean("SmtpAuthXoauth2"),
            })
        } else {
            None
        };
        result.push(Account {
            path: path.to_string(),
            name: string(account, "PresentationIdentity"),
            username: if username.is_empty() {
                email.clone()
            } else {
                username
            },
            email,
            host: string(mail, "ImapHost"),
            port: mail
                .get("ImapPort")
                .and_then(|v| v.get::<u32>())
                .and_then(|v| u16::try_from(v).ok())
                .unwrap_or(if ssl { 993 } else { 143 }),
            ssl,
            tls: boolean("ImapUseTls"),
            oauth2: interfaces.contains_key(OAUTH),
            smtp,
        });
    }
    result.sort_by(|a, b| a.email.cmp(&b.email));
    Ok(result)
}

fn smtp_endpoint(value: &str, ssl: bool) -> (String, u16) {
    let default_port = if ssl { 465 } else { 587 };
    if let Some((host, port)) = value.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
        && (!host.contains(':') || host.starts_with('['))
    {
        return (host.trim_matches(['[', ']']).to_owned(), port);
    }
    (value.trim_matches(['[', ']']).to_owned(), default_port)
}

pub fn token(account: &Account) -> Result<String> {
    let reply = call(&account.path, OAUTH, "GetAccessToken", None)?;
    let (token, _) = reply
        .get::<(String, i32)>()
        .context("Invalid OAuth2 response")?;
    Ok(token)
}

pub fn password(account: &Account) -> Result<String> {
    password_for(account, "imap-password")
}

pub fn smtp_password(account: &Account) -> Result<String> {
    password_for(account, "smtp-password")
}

fn password_for(account: &Account, kind: &str) -> Result<String> {
    use glib::variant::ToVariant;
    let reply = call(
        &account.path,
        "org.gnome.OnlineAccounts.PasswordBased",
        "GetPassword",
        Some(&(kind,).to_variant()),
    )?;
    let (password,) = reply
        .get::<(String,)>()
        .context("Invalid password response")?;
    Ok(password)
}
