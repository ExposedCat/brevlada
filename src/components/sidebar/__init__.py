from utils.toolkit import Gtk, GLib
import dbus
import imaplib
import ssl
import threading
import logging
from components.button import AppButton
from components.container import NavigationList, ContentItem, ScrollContainer, ButtonContainer, ContentContainer
from components.ui import AppIcon, AppText

class AccountsSidebar(Gtk.Box):
    def __init__(self, class_names=None, **kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, **kwargs)
        self.add_css_class("sidebar")

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

        self.sidebar_scroll = ScrollContainer(class_names="sidebar-scroll")
        self.sidebar_list = NavigationList(class_names="main-navigation")

        self.sidebar_scroll.set_child(self.sidebar_list)
        self.append(self.sidebar_scroll)

        self.accounts_data = []
        self.account_folders = {}
        self.expanded_folders = {}
        self.selected_account_button = None
        self.selected_folder_button = None
        self.selection_callback = None

        self.load_accounts()

    def connect_row_selected(self, callback):
        self.sidebar_list.connect("row-selected", callback)
        self.selection_callback = callback

    def get_folder_icon(self, folder_name):
        folder_upper = folder_name.upper()

        if folder_upper == "INBOX":
            return "mail-unread-symbolic"
        elif "SENT MAIL" in folder_upper or "SENT" in folder_upper or "ITEMS" in folder_upper:
            return "mail-send-symbolic"
        elif "DRAFT" in folder_upper:
            return "document-edit-symbolic"
        elif "TRASH" in folder_upper or "DELETED" in folder_upper:
            return "user-trash-symbolic"
        elif "SPAM" in folder_upper or "JUNK" in folder_upper or "BULK" in folder_upper:
            return "mail-mark-junk-symbolic"
        elif "ALL MAIL" in folder_upper:
            return "mail-unread-symbolic"
        elif "ARCHIVE" in folder_upper:
            return "shoe-box-symbolic"
        elif "IMPORTANT" in folder_upper:
            return "mail-mark-important-symbolic"
        elif "STARRED" in folder_upper:
            return "starred-symbolic"
        else:
            return "folder-symbolic"

    def on_expand_clicked(self, button, account_row):
        if getattr(account_row, 'expanded', False):
            self.collapse_account(account_row)
        else:
            self.expand_account(account_row)

    def on_account_button_clicked(self, button, account_row):
        if self.selected_account_button:
            self.selected_account_button.set_selected(False)
        if self.selected_folder_button:
            self.selected_folder_button.set_selected(False)

        self.selected_account_button = button
        self.selected_folder_button = None
        button.set_selected(True)

        self.sidebar_list.select_row(account_row)
        if self.selection_callback:
            self.selection_callback(self.sidebar_list, account_row)

    def on_folder_button_clicked(self, button, folder_row):
        if getattr(folder_row, 'has_children', False):
            account_key = folder_row.parent_account["email"]
            folder_key = f"{account_key}:{folder_row.full_path}"

            if folder_key in self.expanded_folders and self.expanded_folders[folder_key]:
                self.collapse_folder(folder_row)
            else:
                self.expand_folder(folder_row)
        else:
            if self.selected_account_button:
                self.selected_account_button.set_selected(False)
            if self.selected_folder_button:
                self.selected_folder_button.set_selected(False)

            self.selected_folder_button = button
            self.selected_account_button = None
            button.set_selected(True)

            if self.selection_callback:
                self.selection_callback(self.sidebar_list, folder_row)

    def get_oauth2_token(self, account_data):
        try:
            bus = dbus.SessionBus()
            account_obj = bus.get_object('org.gnome.OnlineAccounts', account_data['path'])
            oauth2_props = dbus.Interface(account_obj, 'org.gnome.OnlineAccounts.OAuth2Based')
            access_token = oauth2_props.GetAccessToken()
            return access_token[0] if access_token else None
        except Exception as e:
            logging.error(f"Error getting OAuth2 token: {e}")
            return None

    def get_mail_settings(self, account_data):
        try:
            bus = dbus.SessionBus()
            account_obj = bus.get_object('org.gnome.OnlineAccounts', account_data['path'])
            mail_props = dbus.Interface(account_obj, 'org.freedesktop.DBus.Properties')

            mail_properties = mail_props.GetAll('org.gnome.OnlineAccounts.Mail')

            return {
                'email': mail_properties.get('EmailAddress', ''),
                'imap_host': mail_properties.get('ImapHost', 'imap.gmail.com'),
                'imap_port': mail_properties.get('ImapPort', 993),
                'imap_username': mail_properties.get('ImapUserName', ''),
                'imap_use_ssl': mail_properties.get('ImapUseSsl', True),
                'imap_use_tls': mail_properties.get('ImapUseTls', True),
                'imap_accept_ssl_errors': mail_properties.get('ImapAcceptSslErrors', False),
            }
        except Exception:
            logging.error("Error getting mail settings")
            return None

    def authenticate_imap_oauth2(self, mail, username, token):
        auth_string = f'user={username}\x01auth=Bearer {token}\x01\x01'

        def auth_callback(response):
            return auth_string.encode('utf-8')

        try:
            mail.authenticate('XOAUTH2', auth_callback)
        except Exception as e:
            raise

    def fetch_imap_folders(self, account_data, callback):
        def fetch_folders():
            try:
                email = account_data["email"]

                mail_settings = self.get_mail_settings(account_data)

                if not mail_settings:
                    error_msg = "Error: Could not get mail settings"
                    logging.error(f"No mail settings for account: {account_data['path']}")
                    self.account_folders[account_data["path"]] = [error_msg]
                    GLib.idle_add(callback, [error_msg])
                    return

                server = mail_settings.get('imap_host', 'imap.gmail.com')
                port = mail_settings.get('imap_port', 993)
                use_ssl = mail_settings.get('imap_use_ssl', True)
                username = mail_settings.get('imap_username', email)

                auth_xoauth2 = account_data.get("has_oauth2", False)

                logging.info(f"Connecting to IMAP server: {server}, SSL: {use_ssl}, OAuth2: {auth_xoauth2}")

                folders = []
                authenticated = False

                if use_ssl:
                    context = ssl.create_default_context()
                    context.check_hostname = False
                    context.verify_mode = ssl.CERT_NONE
                    mail = imaplib.IMAP4_SSL(server, port, ssl_context=context)
                else:
                    mail = imaplib.IMAP4(server, port)
                    if mail_settings.get('imap_use_tls', True):
                        mail.starttls()

                if auth_xoauth2:
                    logging.info("Account supports OAuth2, attempting authentication")
                    token = self.get_oauth2_token(account_data)
                    if token:
                        try:
                            self.authenticate_imap_oauth2(mail, username, token)
                            authenticated = True
                            logging.info("OAuth2 authentication successful")
                        except Exception as e:
                            logging.error(f"OAuth2 authentication failed: {e}")
                    else:
                        logging.error("Failed to get OAuth2 token")
                else:
                    logging.warning("Account does not support OAuth2")

                if not authenticated:
                    error_msg = "Error: Authentication failed - OAuth2 required"
                    logging.error(f"Authentication failed for {email}")
                    self.account_folders[account_data["path"]] = [error_msg]
                    GLib.idle_add(callback, [error_msg])
                    return

                try:
                    status, folder_list = mail.list()
                    if status == 'OK':
                        for folder_line in folder_list:
                            if folder_line:
                                if isinstance(folder_line, bytes):
                                    folder_line = folder_line.decode('utf-8')
                                elif isinstance(folder_line, tuple):
                                    folder_line = folder_line[0].decode('utf-8') if folder_line[0] else ''
                                else:
                                    folder_line = str(folder_line)

                                if '"' in folder_line:
                                    parts = folder_line.split('"')
                                    if len(parts) >= 3:
                                        folder_name = parts[-2]
                                        folders.append(folder_name)
                                else:
                                    parts = folder_line.split()
                                    if len(parts) >= 3:
                                        folder_name = ' '.join(parts[2:])
                                        folders.append(folder_name)

                    if folders:
                        folders.sort()
                        logging.info(f"Found {len(folders)} folders for {email}")
                    else:
                        folders = ["Error: No folders found"]
                        logging.warning(f"No folders found for {email}")

                    mail.logout()

                except Exception as e:
                    logging.error(f"Error fetching folders: {e}")
                    if "Authentication" in str(e) or "login" in str(e).lower() or "AUTHENTICATE" in str(e):
                        error_msg = "Error: Authentication failed - check account credentials"
                    elif "timeout" in str(e).lower() or "connection" in str(e).lower():
                        error_msg = "Error: Connection failed - check network"
                    elif "SSL" in str(e) or "certificate" in str(e).lower():
                        error_msg = "Error: SSL/TLS connection failed"
                    else:
                        error_msg = "Error: IMAP connection failed"

                    folders = [error_msg]

                self.account_folders[account_data["path"]] = folders
                GLib.idle_add(callback, folders)

            except Exception as e:
                logging.error(f"Failed to fetch folders: {e}")
                error_msg = "Error: Failed to connect to mail server"
                folders = [error_msg]
                self.account_folders[account_data["path"]] = folders
                GLib.idle_add(callback, folders)

        thread = threading.Thread(target=fetch_folders)
        thread.daemon = True
        thread.start()

    def expand_account(self, account_row):
        setattr(account_row, 'expanded', True)
        account_row.expand_button.set_icon_name("pan-down-symbolic")

        account_path = account_row.account_data["path"]
        if account_path in self.account_folders:
            self.add_folder_rows(account_row, self.account_folders[account_path])
        else:
            self.add_loading_row(account_row)

            def on_folders_fetched(folders):
                self.remove_loading_row(account_row)
                self.add_folder_rows(account_row, folders)

            self.fetch_imap_folders(account_row.account_data, on_folders_fetched)

    def add_loading_row(self, account_row):
        loading_row = ContentItem(class_names="loading-row")
        setattr(loading_row, 'is_loading', True)
        setattr(loading_row, 'parent_account', account_row.account_data)

        loading_container = ButtonContainer(
            spacing=10,
            class_names="loading-container"
        )
        loading_container.set_margin_top(8)
        loading_container.set_margin_bottom(8)
        loading_container.set_margin_start(32)
        loading_container.set_margin_end(12)

        loading_spinner = Gtk.Spinner()
        loading_spinner.start()
        loading_container.append(loading_spinner)

        loading_text = AppText(
            text="Loading folders...",
            class_names=["loading-text", "dim-label"]
        )
        loading_container.append(loading_text)

        loading_row.set_child(loading_container)

        account_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == account_row:
                account_index = i
                break

        self.sidebar_list.insert(loading_row, account_index + 1)

    def remove_loading_row(self, account_row):
        for row in self.get_sidebar_rows():
            if hasattr(row, 'is_loading') and hasattr(row, 'parent_account') and getattr(row, 'parent_account') == account_row.account_data:
                self.sidebar_list.remove(row)
                break

    def organize_folders_hierarchy(self, folders):
        root_folders = {}

        for folder in folders:
            if folder.startswith("Error:"):
                root_folders[folder] = {'name': folder, 'full_path': folder, 'children': {}, 'is_error': True}
                continue

            parts = []
            if folder.startswith("[") and "]" in folder:
                bracket_part = folder.split("]")[0] + "]"
                remaining = folder[len(bracket_part):]
                parts.append(bracket_part)
                if remaining.startswith("/"):
                    remaining = remaining[1:]
                if remaining:
                    parts.extend(remaining.split("/"))
            else:
                parts = folder.split("/")

            current_level = root_folders
            current_path = ""

            for i, part in enumerate(parts):
                if i == 0:
                    current_path = part
                else:
                    current_path += "/" + part

                if part not in current_level:
                    current_level[part] = {
                        'name': part,
                        'full_path': current_path,
                        'children': {},
                        'is_error': False
                    }

                current_level = current_level[part]['children']

        return root_folders

    def add_folder_rows(self, account_row, folders):
        account_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == account_row:
                account_index = i
                break

        organized_folders = self.organize_folders_hierarchy(folders)
        current_index = 0

        current_index = self.add_folder_level(organized_folders, account_row, account_index + 1, current_index, 0)

    def add_folder_level(self, folder_dict, account_row, insert_position, current_index, level):
        for folder_name, folder_data in sorted(folder_dict.items()):
            if level == 0 and folder_data['full_path'].upper() == 'INBOX':
                continue
            if folder_data['is_error']:
                error_row = ContentItem(class_names="error-folder-item")
                setattr(error_row, 'is_folder', True)
                setattr(error_row, 'folder_name', folder_data['name'])
                setattr(error_row, 'parent_account', account_row.account_data)

                error_container = ButtonContainer(class_names="error-container")
                error_container.set_margin_start(45 * (level + 1))

                error_button = AppButton(
                    variant="primary",
                    expandable=True,
                    class_names=["error-button"]
                )
                error_box = ContentContainer(class_names="error-content")

                error_icon = AppIcon("dialog-error-symbolic", class_names="error-icon")
                error_text = AppText(folder_data['name'], class_names="error-text")
                error_box.append(error_icon)
                error_box.append(error_text)

                error_button.set_child(error_box)
                error_container.append(error_button)

                error_row.set_child(error_container)
                setattr(error_row, 'main_box', error_container)
                setattr(error_row, 'folder_button', error_button)

                self.sidebar_list.insert(error_row, insert_position + current_index)
                current_index += 1
            else:
                folder_row = ContentItem(class_names="folder-item")
                setattr(folder_row, 'is_folder', True)
                setattr(folder_row, 'folder_name', folder_data['name'])
                setattr(folder_row, 'full_path', folder_data['full_path'])
                setattr(folder_row, 'parent_account', account_row.account_data)
                setattr(folder_row, 'has_children', len(folder_data['children']) > 0)
                setattr(folder_row, 'children_data', folder_data['children'])
                setattr(folder_row, 'level', level)

                icon_name = self.get_folder_icon(folder_data['full_path'])

                folder_container = ButtonContainer(class_names="folder-container")
                folder_container.set_margin_start(45 * (level + 1))

                folder_button = AppButton(
                    variant="primary",
                    expandable=True,
                    class_names=["folder-button"]
                )
                folder_box = ContentContainer(class_names="folder-content")

                if getattr(folder_row, 'has_children'):
                    account_key = account_row.account_data["email"]
                    folder_key = f"{account_key}:{folder_data['full_path']}"
                    is_expanded = self.expanded_folders.get(folder_key, False)
                    arrow_icon = AppIcon("pan-end-symbolic" if not is_expanded else "pan-down-symbolic", class_names="arrow-icon")
                    folder_box.append(arrow_icon)
                else:
                    folder_icon = AppIcon(icon_name, class_names="folder-icon")
                    folder_box.append(folder_icon)

                folder_text = AppText(folder_data['name'], class_names="folder-text")
                folder_box.append(folder_text)

                folder_button.set_child(folder_box)
                folder_button.connect("clicked", self.on_folder_button_clicked, folder_row)
                folder_container.append(folder_button)

                folder_row.set_child(folder_container)
                setattr(folder_row, 'main_box', folder_container)
                setattr(folder_row, 'folder_button', folder_button)

                self.sidebar_list.insert(folder_row, insert_position + current_index)
                current_index += 1

                if getattr(folder_row, 'has_children'):
                    account_key = account_row.account_data["email"]
                    folder_key = f"{account_key}:{folder_data['full_path']}"
                    if self.expanded_folders.get(folder_key, False):
                        current_index = self.add_folder_level(
                            folder_data['children'],
                            account_row,
                            insert_position,
                            current_index,
                            level + 1
                        )

        return current_index

    def expand_folder(self, folder_row):
        account_key = folder_row.parent_account["email"]
        folder_key = f"{account_key}:{folder_row.full_path}"

        self.expanded_folders[folder_key] = True

        if getattr(folder_row, 'has_children'):
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-down-symbolic")

        folder_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == folder_row:
                folder_index = i
                break

        account_row = None
        for row in self.get_sidebar_rows():
            if hasattr(row, 'account_data') and getattr(row, 'account_data')["email"] == account_key:
                account_row = row
                break

        if account_row:
            self.add_folder_level(
                folder_row.children_data,
                account_row,
                folder_index + 1,
                0,
                folder_row.level + 1
            )

    def collapse_folder(self, folder_row):
        account_key = folder_row.parent_account["email"]
        folder_key = f"{account_key}:{folder_row.full_path}"

        self.expanded_folders[folder_key] = False

        if getattr(folder_row, 'has_children'):
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-end-symbolic")

        self.remove_folder_descendants(folder_row)

    def remove_folder_descendants(self, parent_folder_row):
        rows_to_remove = []
        parent_level = parent_folder_row.level
        account_key = parent_folder_row.parent_account["email"]

        found_parent = False
        for row in self.get_sidebar_rows():
            if row == parent_folder_row:
                found_parent = True
                continue

            if found_parent and hasattr(row, 'level') and hasattr(row, 'parent_account'):
                if getattr(row, 'parent_account')["email"] == account_key:
                    if getattr(row, 'level') > parent_level:
                        rows_to_remove.append(row)
                        if hasattr(row, 'full_path'):
                            descendant_key = f"{account_key}:{row.full_path}"
                            if descendant_key in self.expanded_folders:
                                self.expanded_folders[descendant_key] = False
                    else:
                        break

        for row in rows_to_remove:
            self.sidebar_list.remove(row)

    def collapse_account(self, account_row):
        setattr(account_row, 'expanded', False)
        account_row.expand_button.set_icon_name("pan-end-symbolic")

        account_email = account_row.account_data["email"]
        keys_to_remove = []
        for key in self.expanded_folders:
            if key.startswith(f"{account_email}:"):
                keys_to_remove.append(key)

        for key in keys_to_remove:
            del self.expanded_folders[key]

        rows_to_remove = []
        for row in self.get_sidebar_rows():
            if hasattr(row, 'parent_account') and getattr(row, 'parent_account')["email"] == account_email:
                rows_to_remove.append(row)

        for row in rows_to_remove:
            self.sidebar_list.remove(row)

    def get_sidebar_rows(self):
        rows = []
        index = 0
        while True:
            row = self.sidebar_list.get_row_at_index(index)
            if row is None:
                break
            rows.append(row)
            index += 1
        return rows

    def load_accounts(self):
        try:
            bus = dbus.SessionBus()
            goa_proxy = bus.get_object('org.gnome.OnlineAccounts', '/org/gnome/OnlineAccounts')

            obj_manager = dbus.Interface(goa_proxy, 'org.freedesktop.DBus.ObjectManager')

            managed_objects = obj_manager.GetManagedObjects()

            found_accounts = False
            self.accounts_data = []

            for path, interfaces in managed_objects.items():
                if 'org.gnome.OnlineAccounts.Account' in interfaces:
                    account = interfaces['org.gnome.OnlineAccounts.Account']

                    if path in managed_objects and 'org.gnome.OnlineAccounts.Mail' in managed_objects[path]:
                        found_accounts = True

                        account_name = account.get('PresentationIdentity', 'Unknown')
                        provider = account.get('ProviderName', 'Unknown')

                        mail = managed_objects[path]['org.gnome.OnlineAccounts.Mail']
                        email_address = mail.get('EmailAddress', 'No email address')

                        if (account_name == 'Unknown' or not account_name) and email_address != 'No email address':
                            account_name = email_address

                        has_oauth2 = 'org.gnome.OnlineAccounts.OAuth2Based' in managed_objects[path]

                        account_data = {
                            "path": path,
                            "account_name": account_name,
                            "provider": provider,
                            "email": email_address,
                            "has_oauth2": has_oauth2
                        }

                        self.accounts_data.append(account_data)

                        account_row = ContentItem(class_names="account-item")
                        setattr(account_row, 'account_data', account_data)
                        setattr(account_row, 'expanded', False)

                        account_container = ButtonContainer(
                            spacing=6,
                            class_names="account-container"
                        )

                        expand_button = AppButton(
                            variant="expand",
                            class_names=["expand-button", "account-expand"]
                        )
                        expand_button.set_icon_name("pan-end-symbolic")
                        expand_button.connect("clicked", self.on_expand_clicked, account_row)

                        account_button = AppButton(
                            variant="primary",
                            expandable=True,
                            class_names=["account-button"]
                        )

                        account_box = ContentContainer(
                            class_names="account-content"
                        )
                        account_icon = AppIcon(
                            "mail-unread-symbolic",
                            class_names="account-icon"
                        )
                        account_text = AppText(
                            text=account_name,
                            class_names="account-text"
                        )
                        account_box.append(account_icon)
                        account_box.append(account_text)

                        account_button.set_child(account_box)
                        account_button.connect("clicked", self.on_account_button_clicked, account_row)

                        account_container.append(expand_button)
                        account_container.append(account_button)

                        account_row.set_child(account_container)
                        setattr(account_row, 'main_box', account_container)
                        setattr(account_row, 'expand_button', expand_button)
                        setattr(account_row, 'account_button', account_button)

                        account_row.set_selectable(True)

                        self.sidebar_list.append(account_row)

            if not found_accounts:
                no_accounts_row = ContentItem(class_names="no-accounts-item")
                no_accounts_container = ContentContainer(
                    class_names="no-accounts-container"
                )
                no_accounts_container.set_orientation(Gtk.Orientation.VERTICAL)
                no_accounts_container.set_margin_top(20)

                no_accounts_text = AppText(
                    text="No email accounts found",
                    class_names=["dim-label"],
                    expandable=False
                )
                no_accounts_container.append(no_accounts_text)

                no_accounts_row.set_child(no_accounts_container)
                self.sidebar_list.append(no_accounts_row)

        except Exception as e:
            error_row = ContentItem(class_names="error-item")
            error_container = ContentContainer(
                class_names="error-container"
            )
            error_container.set_orientation(Gtk.Orientation.VERTICAL)
            error_container.set_margin_top(20)

            error_text = AppText(
                text=f"Error loading accounts: {str(e)}",
                class_names=["error"],
                expandable=False
            )
            error_container.append(error_text)

            error_row.set_child(error_container)
            self.sidebar_list.append(error_row)
