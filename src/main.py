import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Gio
import dbus
import imaplib
import ssl
import threading
import base64
import os
import logging
from components.button import AppButton
try:
    from resources import load_resources
    load_resources()
except ImportError:
    pass
from components.container import Sidebar, NavigationList, ContentItem, ScrollContainer, ButtonContainer, ContentContainer
from components.ui import AppIcon, AppText

class MyWindow(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app)
        self.set_title("Online Accounts")
        self.set_default_size(1200, 800)

        # Setup logging
        logging.basicConfig(level=logging.DEBUG, format='%(asctime)s - %(levelname)s - %(message)s')

        # Register icon theme
        display = self.get_display()
        if display:
            icon_theme = Gtk.IconTheme.get_for_display(display)
            icon_theme.add_resource_path("/org/gtk/example/icons")

        self.toolbar_view = Adw.ToolbarView()
        self.toolbar_view.set_top_bar_style(Adw.ToolbarStyle.FLAT)

        self.unified_header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.unified_header.set_size_request(-1, 48)
        self.unified_header.add_css_class("unified-header")

        self.sidebar_header = Adw.HeaderBar()
        self.sidebar_header.set_title_widget(Gtk.Label(label="Accounts"))
        self.sidebar_header.set_show_end_title_buttons(False)
        self.sidebar_header.set_size_request(350, -1)
        self.sidebar_header.add_css_class("sidebar-header")

        self.window_title = Adw.WindowTitle()
        self.window_title.set_title("Online Accounts")
        self.window_title.set_subtitle("Select an account")

        self.content_header = Adw.HeaderBar()
        self.content_header.set_title_widget(self.window_title)
        self.content_header.set_centering_policy(Adw.CenteringPolicy.STRICT)
        self.content_header.set_hexpand(True)
        self.content_header.add_css_class("content-header")

        self.unified_header.append(self.sidebar_header)
        self.unified_header.append(self.content_header)

        self.toolbar_view.add_top_bar(self.unified_header)

        self.paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self.paned.set_position(350)
        self.paned.set_wide_handle(False)
        self.paned.connect("notify::position", self.on_paned_position_changed)

        css_provider = Gtk.CssProvider()
        css_file = os.path.join(os.path.dirname(__file__), 'style.css')
        css_provider.load_from_path(css_file)
        Gtk.StyleContext.add_provider_for_display(
            self.get_display(),
            css_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )

        self.content_area = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=20)
        self.content_area.set_margin_top(30)
        self.content_area.set_margin_bottom(30)
        self.content_area.set_margin_start(30)
        self.content_area.set_margin_end(30)
        self.content_area.set_vexpand(True)
        self.content_area.set_hexpand(True)

        self.empty_state = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=15)
        self.empty_state.set_halign(Gtk.Align.CENTER)
        self.empty_state.set_valign(Gtk.Align.CENTER)

        empty_icon = Gtk.Image.new_from_icon_name("mail-unread-symbolic")
        empty_icon.set_pixel_size(64)
        empty_icon.set_opacity(0.5)

        empty_label = Gtk.Label()
        empty_label.set_markup("<span size='large'>Select an account to view details</span>")
        empty_label.set_opacity(0.7)

        self.empty_state.append(empty_icon)
        self.empty_state.append(empty_label)

        self.content_area.append(self.empty_state)

        self.account_details = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=15)
        self.account_details.set_visible(False)
        self.content_area.append(self.account_details)

        content_scroll = Gtk.ScrolledWindow()
        content_scroll.set_vexpand(True)
        content_scroll.set_child(self.content_area)

        sidebar = Sidebar()

        sidebar_scroll = ScrollContainer()

        self.sidebar_list = NavigationList()
        self.sidebar_list.connect("row-selected", self.on_account_selected)

        sidebar_scroll.set_child(self.sidebar_list)
        sidebar.append(sidebar_scroll)

        self.paned.set_start_child(sidebar)
        self.paned.set_end_child(content_scroll)
        self.paned.set_resize_start_child(True)
        self.paned.set_shrink_start_child(False)

        self.toolbar_view.set_content(self.paned)

        self.set_content(self.toolbar_view)

        self.accounts_data = []

        # Track expanded state of folders (nested structure)
        self.expanded_folders = {}

        self.load_accounts()

        self.account_folders = {}

        self.selected_account_button = None
        self.selected_folder_button = None

    def on_account_button_clicked(self, button, account_row):
        if self.selected_account_button:
            self.selected_account_button.set_selected(False)
        if self.selected_folder_button:
            self.selected_folder_button.set_selected(False)

        self.selected_account_button = button
        self.selected_folder_button = None
        button.set_selected(True)

        self.on_account_selected(self.sidebar_list, account_row)



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
        if account_row.expanded:
            self.collapse_account(account_row)
        else:
            self.expand_account(account_row)

    def on_folder_button_clicked(self, button, folder_row):
        if folder_row.has_children:
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

            self.on_account_selected(self.sidebar_list, folder_row)

    def get_oauth2_token(self, account_path):
        try:
            logging.info(f"Getting OAuth2 token for account: {account_path}")
            bus = dbus.SessionBus()
            goa_proxy = bus.get_object('org.gnome.OnlineAccounts', account_path)

            # Ensure credentials are fresh before getting token
            account_interface = dbus.Interface(goa_proxy, 'org.gnome.OnlineAccounts.Account')
            try:
                expires_in = account_interface.EnsureCredentials()
                logging.info(f"Ensured credentials, expires in: {expires_in} seconds")
            except Exception as e:
                logging.warning(f"Could not ensure credentials: {e}")

            oauth2_interface = dbus.Interface(goa_proxy, 'org.gnome.OnlineAccounts.OAuth2Based')
            token_info = oauth2_interface.GetAccessToken()
            if token_info:
                logging.info(f"Successfully got OAuth2 token, expires in: {token_info[1]} seconds")
                return token_info[0]
            else:
                logging.warning("OAuth2 token is empty")
                return None
        except Exception as e:
            logging.error(f"Failed to get OAuth2 token: {e}")
            return None

    def get_mail_settings(self, account_path):
        try:
            logging.info(f"Getting mail settings for account: {account_path}")
            bus = dbus.SessionBus()
            goa_proxy = bus.get_object('org.gnome.OnlineAccounts', account_path)
            props_interface = dbus.Interface(goa_proxy, 'org.freedesktop.DBus.Properties')
            mail_props = props_interface.GetAll('org.gnome.OnlineAccounts.Mail')
            logging.info(f"Mail settings: {dict(mail_props)}")
            return mail_props
        except Exception as e:
            logging.error(f"Failed to get mail settings: {e}")
            return {}

    def authenticate_imap_oauth2(self, mail, email, token):
        logging.info(f"Attempting OAuth2 authentication for {email}")
        auth_string = 'user=' + email + '\1auth=Bearer ' + token + '\1\1'
        logging.debug(f"Auth string before encoding: {repr(auth_string)}")

        auth_string_bytes = auth_string.encode('utf-8')
        logging.debug(f"OAuth2 auth string bytes length: {len(auth_string_bytes)}")

        try:
            def auth_callback(challenge):
                logging.debug(f"OAuth2 challenge: {challenge}")
                return auth_string_bytes

            mail.authenticate('XOAUTH2', auth_callback)
            logging.info("OAuth2 authentication successful")
        except Exception as e:
            logging.error(f"OAuth2 authentication failed: {e}")
            raise



    def fetch_imap_folders(self, account_data, callback):
        def fetch_folders():
            try:
                email = account_data["email"]

                # Get mail settings from GNOME Online Accounts
                mail_settings = self.get_mail_settings(account_data["path"])

                if not mail_settings:
                    error_msg = "Error: Could not get mail settings"
                    logging.error(f"No mail settings for account: {account_data['path']}")
                    self.account_folders[account_data["path"]] = [error_msg]
                    GLib.idle_add(callback, [error_msg])
                    return

                server = mail_settings.get('ImapHost', 'imap.gmail.com')
                use_ssl = mail_settings.get('ImapUseSsl', True)
                username = mail_settings.get('ImapUserName', email)

                # Check if account has OAuth2 interface
                auth_xoauth2 = account_data.get("has_oauth2", False)

                logging.info(f"Connecting to IMAP server: {server}, SSL: {use_ssl}, OAuth2: {auth_xoauth2}")

                port = 993 if use_ssl else 143

                try:
                    context = ssl.create_default_context()
                    logging.info(f"Connecting to {server}:{port}")

                    if use_ssl:
                        mail = imaplib.IMAP4_SSL(server, port, ssl_context=context)
                    else:
                        mail = imaplib.IMAP4(server, port)

                    logging.info("IMAP connection established")
                    authenticated = False

                    # Try OAuth2 authentication
                    if auth_xoauth2:
                        logging.info("Account supports OAuth2, attempting authentication")
                        token = self.get_oauth2_token(account_data["path"])
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

                    # List folders
                    logging.info("Listing IMAP folders")
                    status, folder_list = mail.list()
                    logging.info(f"IMAP LIST status: {status}, found {len(folder_list) if folder_list else 0} folders")
                    mail.logout()

                    if status == 'OK':
                        folders = []
                        for folder_info in folder_list:
                            if isinstance(folder_info, bytes):
                                folder_info = folder_info.decode('utf-8')

                            # Parse folder name from IMAP LIST response
                            parts = folder_info.split('"')
                            if len(parts) >= 3:
                                folder_name = parts[-2]
                                if folder_name and folder_name not in folders:
                                    folders.append(folder_name)

                        if not folders:
                            folders = ["INBOX"]

                        # Sort folders by importance
                        folders.sort(key=lambda x: (
                            0 if x.upper() == 'INBOX' else
                            1 if any(sent in x.upper() for sent in ['SENT', 'OUTBOX']) else
                            2 if any(draft in x.upper() for draft in ['DRAFT', 'DRAFTS']) else
                            3 if any(trash in x.upper() for trash in ['TRASH', 'DELETED', 'BIN']) else
                            4 if any(spam in x.upper() for spam in ['SPAM', 'JUNK']) else
                            5
                        ))
                    else:
                        error_msg = f"Error: Failed to list folders - {status}"
                        folders = [error_msg]

                except Exception as e:
                    logging.error(f"IMAP connection failed: {e}")
                    if "Authentication" in str(e) or "login" in str(e).lower() or "AUTHENTICATE" in str(e):
                        error_msg = "Error: Authentication failed - check account credentials"
                    elif "timeout" in str(e).lower() or "connection" in str(e).lower():
                        error_msg = "Error: Connection failed - check network"
                    elif "SSL" in str(e) or "certificate" in str(e).lower():
                        error_msg = "Error: SSL/TLS connection failed"
                    else:
                        error_msg = f"Error: IMAP connection failed"
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
        account_row.expanded = True
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
        loading_row = Gtk.ListBoxRow()
        loading_row.is_loading = True
        loading_row.parent_account = account_row.account_data

        loading_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        loading_box.set_margin_top(8)
        loading_box.set_margin_bottom(8)
        loading_box.set_margin_start(32)
        loading_box.set_margin_end(12)

        spinner = Gtk.Spinner()
        spinner.start()
        loading_box.append(spinner)

        loading_label = Gtk.Label(label="Loading folders...")
        loading_label.set_halign(Gtk.Align.START)
        loading_label.add_css_class("dim-label")
        loading_box.append(loading_label)

        loading_row.set_child(loading_box)

        account_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == account_row:
                account_index = i
                break

        self.sidebar_list.insert(loading_row, account_index + 1)

    def remove_loading_row(self, account_row):
        for row in self.get_sidebar_rows():
            if hasattr(row, 'is_loading') and row.is_loading and row.parent_account == account_row.account_data:
                self.sidebar_list.remove(row)
                break

    def organize_folders_hierarchy(self, folders):
        """Organize folders into a true nested hierarchy"""
        root_folders = {}

        for folder in folders:
            if folder.startswith("Error:"):
                root_folders[folder] = {'name': folder, 'full_path': folder, 'children': {}, 'is_error': True}
                continue

            # Split folder path into parts
            parts = []
            if folder.startswith("[") and "]" in folder:
                # Handle format like [Gmail]/Sent
                bracket_part = folder.split("]")[0] + "]"
                remaining = folder[len(bracket_part):]
                parts.append(bracket_part)
                if remaining.startswith("/"):
                    remaining = remaining[1:]
                if remaining:
                    parts.extend(remaining.split("/"))
            else:
                parts = folder.split("/")

            # Build nested structure
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

        # Organize folders into hierarchy
        organized_folders = self.organize_folders_hierarchy(folders)
        current_index = 0

        # Add folders recursively
        current_index = self.add_folder_level(organized_folders, account_row, account_index + 1, current_index, 0)

    def add_folder_level(self, folder_dict, account_row, insert_position, current_index, level):
        """Recursively add folders at the current level"""
        for folder_name, folder_data in sorted(folder_dict.items()):
            # Skip INBOX folder at root level
            if level == 0 and folder_data['full_path'].upper() == 'INBOX':
                continue
            if folder_data['is_error']:
                error_row = ContentItem()
                error_row.is_folder = True
                error_row.folder_name = folder_data['name']
                error_row.parent_account = account_row.account_data

                button_container = ButtonContainer()
                button_container.set_margin_start(45 * (level + 1))

                error_button = AppButton(variant="primary", expandable=True)
                error_box = ContentContainer()

                error_icon = AppIcon("dialog-error-symbolic")
                error_text = AppText(folder_data['name'])
                error_box.append(error_icon)
                error_box.append(error_text)

                error_button.set_child(error_box)
                button_container.append(error_button)

                error_row.set_child(button_container)
                error_row.main_box = button_container
                error_row.folder_button = error_button

                self.sidebar_list.insert(error_row, insert_position + current_index)
                current_index += 1
            else:
                folder_row = ContentItem()
                folder_row.is_folder = True
                folder_row.folder_name = folder_data['name']
                folder_row.full_path = folder_data['full_path']
                folder_row.parent_account = account_row.account_data
                folder_row.has_children = len(folder_data['children']) > 0
                folder_row.children_data = folder_data['children']
                folder_row.level = level

                icon_name = self.get_folder_icon(folder_data['full_path'])

                button_container = ButtonContainer()
                button_container.set_margin_start(45 * (level + 1))

                folder_button = AppButton(variant="primary", expandable=True)
                folder_box = ContentContainer()

                if folder_row.has_children:
                    account_key = account_row.account_data["email"]
                    folder_key = f"{account_key}:{folder_data['full_path']}"
                    is_expanded = self.expanded_folders.get(folder_key, False)
                    arrow_icon = AppIcon("pan-end-symbolic" if not is_expanded else "pan-down-symbolic")
                    folder_box.append(arrow_icon)
                else:
                    folder_icon = AppIcon(icon_name)
                    folder_box.append(folder_icon)

                folder_text = AppText(folder_data['name'])
                folder_box.append(folder_text)

                folder_button.set_child(folder_box)
                folder_button.connect("clicked", self.on_folder_button_clicked, folder_row)
                button_container.append(folder_button)

                folder_row.set_child(button_container)
                folder_row.main_box = button_container
                folder_row.folder_button = folder_button

                self.sidebar_list.insert(folder_row, insert_position + current_index)
                current_index += 1

                # Add children if folder is expanded
                if folder_row.has_children:
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

        # Update the arrow icon
        if folder_row.has_children:
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-down-symbolic")

        # Find the position of this folder row
        folder_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == folder_row:
                folder_index = i
                break

        # Add children after the parent folder
        account_row = None
        for row in self.get_sidebar_rows():
            if hasattr(row, 'account_data') and row.account_data["email"] == account_key:
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

        # Update the arrow icon
        if folder_row.has_children:
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-end-symbolic")

        # Remove all descendant rows for this folder
        self.remove_folder_descendants(folder_row)

    def remove_folder_descendants(self, parent_folder_row):
        """Remove all descendants of a folder recursively"""
        rows_to_remove = []
        parent_level = parent_folder_row.level
        account_key = parent_folder_row.parent_account["email"]

        # Find all rows that are descendants of this folder
        found_parent = False
        for row in self.get_sidebar_rows():
            if row == parent_folder_row:
                found_parent = True
                continue

            if found_parent and hasattr(row, 'level') and hasattr(row, 'parent_account'):
                if row.parent_account["email"] == account_key:
                    if row.level > parent_level:
                        rows_to_remove.append(row)
                        # Also mark this folder as collapsed if it was expanded
                        if hasattr(row, 'full_path'):
                            descendant_key = f"{account_key}:{row.full_path}"
                            if descendant_key in self.expanded_folders:
                                self.expanded_folders[descendant_key] = False
                    else:
                        # We've reached a folder at the same level or higher, stop
                        break

        for row in rows_to_remove:
            self.sidebar_list.remove(row)

    def collapse_account(self, account_row):
        account_row.expanded = False
        account_row.expand_button.set_icon_name("pan-end-symbolic")

        # Reset folder expansion state for this account
        account_email = account_row.account_data["email"]
        keys_to_remove = []
        for key in self.expanded_folders:
            if key.startswith(f"{account_email}:"):
                keys_to_remove.append(key)

        for key in keys_to_remove:
            del self.expanded_folders[key]

        # Remove all folder rows for this account
        rows_to_remove = []
        for row in self.get_sidebar_rows():
            if hasattr(row, 'parent_account') and row.parent_account["email"] == account_email:
                rows_to_remove.append(row)

        for row in rows_to_remove:
            self.sidebar_list.remove(row)

    def get_sidebar_rows(self):
        rows = []
        row = self.sidebar_list.get_first_child()
        while row:
            rows.append(row)
            row = row.get_next_sibling()
        return rows

    def on_paned_position_changed(self, paned, param):
        position = paned.get_position()
        self.sidebar_header.set_size_request(position, -1)

    def on_account_selected(self, listbox, row):
        if row is None:
            self.empty_state.set_visible(True)
            self.account_details.set_visible(False)
            self.window_title.set_subtitle("Select an account")
            return

        if hasattr(row, 'is_folder') and row.is_folder:
            account_data = row.parent_account
            folder_name = row.folder_name
            folder_full_path = getattr(row, 'full_path', folder_name)

            self.window_title.set_title(f"{account_data['provider']} - {folder_full_path}")
            self.window_title.set_subtitle(account_data["account_name"])

            child = self.account_details.get_first_child()
            while child:
                self.account_details.remove(child)
                child = self.account_details.get_first_child()

            name_label = Gtk.Label()
            name_label.set_markup(f"<span size='xx-large' weight='bold'>{folder_full_path}</span>")
            name_label.set_halign(Gtk.Align.START)
            name_label.set_margin_bottom(15)
            self.account_details.append(name_label)

            account_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            account_label = Gtk.Label(label="Account:")
            account_label.set_halign(Gtk.Align.START)
            account_label.add_css_class("dim-label")
            account_value = Gtk.Label(label=account_data["account_name"])
            account_value.set_halign(Gtk.Align.START)
            account_box.append(account_label)
            account_box.append(account_value)
            self.account_details.append(account_box)

            folder_info_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            folder_info_label = Gtk.Label(label="Folder:")
            folder_info_label.set_halign(Gtk.Align.START)
            folder_info_label.add_css_class("dim-label")
            folder_info_value = Gtk.Label(label=folder_full_path)
            folder_info_value.set_halign(Gtk.Align.START)
            folder_info_box.append(folder_info_label)
            folder_info_box.append(folder_info_value)
            self.account_details.append(folder_info_box)

            self.empty_state.set_visible(False)
            self.account_details.set_visible(True)
            return

        if not hasattr(row, 'account_data'):
            return

        account_data = row.account_data

        self.window_title.set_title(account_data["provider"])
        self.window_title.set_subtitle(account_data["account_name"])

        child = self.account_details.get_first_child()
        while child:
            self.account_details.remove(child)
            child = self.account_details.get_first_child()

        name_label = Gtk.Label()
        name_label.set_markup(f"<span size='xx-large' weight='bold'>{account_data['account_name']}</span>")
        name_label.set_halign(Gtk.Align.START)
        name_label.set_margin_bottom(15)
        self.account_details.append(name_label)

        provider_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        provider_label = Gtk.Label(label="Provider:")
        provider_label.set_halign(Gtk.Align.START)
        provider_label.add_css_class("dim-label")
        provider_value = Gtk.Label(label=account_data["provider"])
        provider_value.set_halign(Gtk.Align.START)
        provider_box.append(provider_label)
        provider_box.append(provider_value)
        self.account_details.append(provider_box)

        email_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        email_label = Gtk.Label(label="Email:")
        email_label.set_halign(Gtk.Align.START)
        email_label.add_css_class("dim-label")
        email_value = Gtk.Label(label=account_data["email"])
        email_value.set_halign(Gtk.Align.START)
        email_box.append(email_label)
        email_box.append(email_value)
        self.account_details.append(email_box)

        self.empty_state.set_visible(False)
        self.account_details.set_visible(True)

    def load_accounts(self):
        try:
            bus = dbus.SessionBus()

            goa_proxy = bus.get_object('org.gnome.OnlineAccounts',
                                      '/org/gnome/OnlineAccounts')

            obj_manager = dbus.Interface(goa_proxy,
                                        'org.freedesktop.DBus.ObjectManager')

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

                        has_oauth2 = 'org.gnome.OnlineAccounts.OAuth2Based' in managed_objects[path]

                        self.accounts_data.append({
                            "path": path,
                            "account_name": account_name,
                            "provider": provider,
                            "email": email_address,
                            "has_oauth2": has_oauth2
                        })

                        account_row = Gtk.ListBoxRow()
                        account_row.account_data = self.accounts_data[-1]
                        account_row.expanded = False

                        button_container = ButtonContainer()

                        expand_button = AppButton(variant="expand")
                        expand_button.set_icon_name("pan-end-symbolic")
                        expand_button.connect("clicked", self.on_expand_clicked, account_row)
                        button_container.append(expand_button)

                        account_button = AppButton(variant="primary", expandable=True)

                        account_box = ContentContainer()
                        account_icon = AppIcon("mail-unread-symbolic")
                        account_text = AppText(email_address)
                        account_box.append(account_icon)
                        account_box.append(account_text)

                        account_button.set_child(account_box)
                        account_button.connect("clicked", self.on_account_button_clicked, account_row)
                        button_container.append(account_button)

                        account_row.set_child(button_container)
                        account_row.main_box = button_container
                        account_row.expand_button = expand_button
                        account_row.account_button = account_button
                        self.sidebar_list.append(account_row)

            if not found_accounts:
                no_accounts_row = Gtk.ListBoxRow()
                no_accounts_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
                no_accounts_box.set_margin_top(20)

                no_accounts = Gtk.Label(label="No email accounts found")
                no_accounts.add_css_class("dim-label")
                no_accounts_box.append(no_accounts)

                no_accounts_row.set_child(no_accounts_box)
                self.sidebar_list.append(no_accounts_row)

        except Exception as e:
            error_row = Gtk.ListBoxRow()
            error_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            error_box.set_margin_top(20)

            error_label = Gtk.Label(label=f"Error loading accounts: {str(e)}")
            error_label.add_css_class("error")
            error_box.append(error_label)

            error_row.set_child(error_box)
            self.sidebar_list.append(error_row)

class MyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="org.example.OnlineAccountsList",
                         flags=Gio.ApplicationFlags.FLAGS_NONE)

    def do_activate(self):
        win = MyWindow(self)
        win.present()

app = MyApp()
app.run()
