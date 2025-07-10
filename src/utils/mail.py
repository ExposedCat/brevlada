import dbus
import imaplib
import ssl
import threading
import logging
from utils.toolkit import GLib


def get_oauth2_token(account_data):
    try:
        bus = dbus.SessionBus()
        account_obj = bus.get_object("org.gnome.OnlineAccounts", account_data["path"])
        oauth2_props = dbus.Interface(
            account_obj, "org.gnome.OnlineAccounts.OAuth2Based"
        )
        access_token = oauth2_props.GetAccessToken()
        return access_token[0] if access_token else None
    except Exception as e:
        logging.error(f"Error getting OAuth2 token: {e}")
        return None


def get_mail_settings(account_data):
    try:
        bus = dbus.SessionBus()
        account_obj = bus.get_object("org.gnome.OnlineAccounts", account_data["path"])
        mail_props = dbus.Interface(account_obj, "org.freedesktop.DBus.Properties")

        mail_properties = mail_props.GetAll("org.gnome.OnlineAccounts.Mail")

        return {
            "email": mail_properties.get("EmailAddress", ""),
            "imap_host": mail_properties.get("ImapHost", "imap.gmail.com"),
            "imap_port": mail_properties.get("ImapPort", 993),
            "imap_username": mail_properties.get("ImapUserName", ""),
            "imap_use_ssl": mail_properties.get("ImapUseSsl", True),
            "imap_use_tls": mail_properties.get("ImapUseTls", True),
            "imap_accept_ssl_errors": mail_properties.get("ImapAcceptSslErrors", False),
        }
    except Exception:
        logging.error("Error getting mail settings")
        return None


def authenticate_imap_oauth2(mail, username, token):
    auth_string = f"user={username}\x01auth=Bearer {token}\x01\x01"

    def auth_callback(response):
        return auth_string.encode("utf-8")

    try:
        mail.authenticate("XOAUTH2", auth_callback)
    except Exception:
        raise


def connect_to_imap_server(mail_settings):
    server = mail_settings.get("imap_host", "imap.gmail.com")
    port = mail_settings.get("imap_port", 993)
    use_ssl = mail_settings.get("imap_use_ssl", True)

    logging.info(f"Connecting to IMAP server: {server}, SSL: {use_ssl}")

    if use_ssl:
        context = ssl.create_default_context()
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        mail = imaplib.IMAP4_SSL(server, port, ssl_context=context)
    else:
        mail = imaplib.IMAP4(server, port)
        if mail_settings.get("imap_use_tls", True):
            mail.starttls()

    return mail


def authenticate_imap(mail, account_data, mail_settings):
    email = account_data["email"]
    username = mail_settings.get("imap_username", email)
    auth_xoauth2 = account_data.get("has_oauth2", False)

    if not auth_xoauth2:
        logging.warning("Account does not support OAuth2")
        return False

    logging.info("Account supports OAuth2, attempting authentication")
    token = get_oauth2_token(account_data)
    if not token:
        logging.error("Failed to get OAuth2 token")
        return False

    try:
        authenticate_imap_oauth2(mail, username, token)
        logging.info("OAuth2 authentication successful")
        return True
    except Exception as e:
        logging.error(f"OAuth2 authentication failed: {e}")
        return False


def parse_folder_line(folder_line):
    if isinstance(folder_line, bytes):
        folder_line = folder_line.decode("utf-8")
    elif isinstance(folder_line, tuple):
        folder_line = folder_line[0].decode("utf-8") if folder_line[0] else ""
    else:
        folder_line = str(folder_line)

    if '"' in folder_line:
        parts = folder_line.split('"')
        if len(parts) >= 3:
            return parts[-2]
    else:
        parts = folder_line.split()
        if len(parts) >= 3:
            return " ".join(parts[2:])

    return None


def get_folders_from_imap(mail, email):
    try:
        status, folder_list = mail.list()
        if status != "OK":
            return ["Error: Failed to list folders"]

        folders = []
        for folder_line in folder_list:
            if folder_line:
                folder_name = parse_folder_line(folder_line)
                if folder_name:
                    folders.append(folder_name)

        if folders:
            folders.sort()
            logging.info(f"Found {len(folders)} folders for {email}")
        else:
            folders = ["Error: No folders found"]
            logging.warning(f"No folders found for {email}")

        return folders

    except Exception as e:
        logging.error(f"Error fetching folders: {e}")
        if (
            "Authentication" in str(e)
            or "login" in str(e).lower()
            or "AUTHENTICATE" in str(e)
        ):
            return ["Error: Authentication failed - check account credentials"]
        elif "timeout" in str(e).lower() or "connection" in str(e).lower():
            return ["Error: Connection failed - check network"]
        elif "SSL" in str(e) or "certificate" in str(e).lower():
            return ["Error: SSL/TLS connection failed"]
        else:
            return ["Error: IMAP connection failed"]


def fetch_imap_folders(account_data, callback):
    def fetch_folders():
        try:
            email = account_data["email"]

            mail_settings = get_mail_settings(account_data)
            if not mail_settings:
                error_msg = "Error: Could not get mail settings"
                logging.error(f"No mail settings for account: {account_data['path']}")
                GLib.idle_add(callback, [error_msg])
                return

            mail = connect_to_imap_server(mail_settings)

            if not authenticate_imap(mail, account_data, mail_settings):
                error_msg = "Error: Authentication failed - OAuth2 required"
                logging.error(f"Authentication failed for {email}")
                GLib.idle_add(callback, [error_msg])
                return

            folders = get_folders_from_imap(mail, email)
            mail.logout()

            GLib.idle_add(callback, folders)

        except Exception as e:
            logging.error(f"Failed to fetch folders: {e}")
            error_msg = "Error: Failed to connect to mail server"
            GLib.idle_add(callback, [error_msg])

    thread = threading.Thread(target=fetch_folders)
    thread.daemon = True
    thread.start()
