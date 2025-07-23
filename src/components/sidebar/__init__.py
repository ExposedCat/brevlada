from utils.toolkit import Gtk
import dbus
import logging
from components.button import AppButton
from components.container import (
    NavigationList,
    ContentItem,
    ScrollContainer,
    ButtonContainer,
    ContentContainer,
)
from components.ui import AppIcon, AppText, LoadingIcon
from utils.mail import fetch_imap_folders
from theme import THEME_MARGIN_LARGE, THEME_INDENT_STEP

class AccountsSidebar:
    def __init__(self, class_names=None, **kwargs):
        self.widget = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, **kwargs)
        self.widget.add_css_class("sidebar")

        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

        self.sidebar_list = NavigationList(class_names="main-navigation")
        self.sidebar_scroll = ScrollContainer(
            class_names="sidebar-scroll", children=self.sidebar_list.widget
        )
        self.widget.append(self.sidebar_scroll.widget)

        self.accounts_data = []
        self.account_folders = {}
        self.expanded_folders = {}
        self.selected_account_button = None
        self.selected_folder_button = None
        self.selection_callback = None
        self.loading_accounts = set()

        self.load_accounts()

    def connect_row_selected(self, callback):
        self.sidebar_list.widget.connect("row-selected", callback)
        self.selection_callback = callback
        

    def get_folder_icon(self, folder_name):
        folder_upper = folder_name.upper()

        if folder_upper == "INBOX":
            return "mail-unread-symbolic"
        elif (
            "SENT MAIL" in folder_upper
            or "SENT" in folder_upper
            or "ITEMS" in folder_upper
        ):
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
        if getattr(account_row, "expanded", False):
            self.collapse_account(account_row)
        else:
            self.expand_account(account_row)

    def on_account_button_clicked(self, button, account_row):
        if self.selected_account_button:
            self.selected_account_button.set_selected(False)
        if self.selected_folder_button:
            self.selected_folder_button.set_selected(False)

        self.selected_account_button = account_row.account_button
        self.selected_folder_button = None
        account_row.account_button.set_selected(True)

        self.sidebar_list.widget.select_row(account_row.widget)
        if self.selection_callback:
            self.selection_callback(self.sidebar_list, account_row)

    def on_folder_button_clicked(self, button, folder_row):
        if getattr(folder_row, "has_children", False):
            account_key = folder_row.parent_account["email"]
            folder_key = f"{account_key}:{folder_row.full_path}"

            if (
                folder_key in self.expanded_folders
                and self.expanded_folders[folder_key]
            ):
                self.collapse_folder(folder_row)
            else:
                self.expand_folder(folder_row)
        else:
            if self.selected_account_button:
                self.selected_account_button.set_selected(False)
            if self.selected_folder_button:
                self.selected_folder_button.set_selected(False)

            self.selected_folder_button = folder_row.folder_button
            self.selected_account_button = None
            folder_row.folder_button.set_selected(True)

            if self.selection_callback:
                self.selection_callback(self.sidebar_list, folder_row)

    def expand_account(self, account_row):
        setattr(account_row, "expanded", True)
        account_row.expand_button.set_icon_name("pan-down-symbolic")

        account_path = account_row.account_data["path"]
        account_email = account_row.account_data["email"]

        if account_path in self.account_folders:
            self.add_folder_rows(account_row, self.account_folders[account_path])
        else:
            self.loading_accounts.add(account_email)
            from utils.toolkit import GLib

            GLib.idle_add(self.update_account_icon, account_row, True)

            def on_folders_fetched(folders):
                self.account_folders[account_path] = folders
                self.add_folder_rows(account_row, folders)

                self.loading_accounts.discard(account_email)
                from utils.toolkit import GLib

                GLib.idle_add(self.update_account_icon, account_row, False)

            fetch_imap_folders(account_row.account_data, on_folders_fetched)

    def update_account_icon(self, account_row, loading=False):
        if hasattr(account_row, "account_icon_widget") and hasattr(
            account_row, "account_icon_container"
        ):
            if loading:
                loading_icon = LoadingIcon(size=16)
                loading_icon.start()
                account_row.account_icon_container.remove(
                    account_row.account_icon_widget
                )
                account_row.account_icon_container.prepend(loading_icon.widget)
                setattr(account_row, "loading_icon", loading_icon)
            else:
                if hasattr(account_row, "loading_icon"):
                    account_row.loading_icon.stop()
                    account_row.account_icon_container.remove(
                        account_row.loading_icon.widget
                    )
                    account_row.account_icon_container.prepend(
                        account_row.account_icon_widget
                    )
                    delattr(account_row, "loading_icon")

    def organize_folders_hierarchy(self, folders):
        root_folders = {}

        for folder in folders:
            if folder.startswith("Error:"):
                root_folders[folder] = {
                    "name": folder,
                    "full_path": folder,
                    "children": {},
                    "is_error": True,
                }
                continue

            parts = []
            if folder.startswith("[") and "]" in folder:
                bracket_part = folder.split("]")[0] + "]"
                remaining = folder[len(bracket_part) :]
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
                        "name": part,
                        "full_path": current_path,
                        "children": {},
                        "is_error": False,
                    }

                current_level = current_level[part]["children"]

        return root_folders

    def add_folder_rows(self, account_row, folders):
        account_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == account_row.widget:
                account_index = i
                break

        organized_folders = self.organize_folders_hierarchy(folders)
        current_index = 0

        current_index = self.add_folder_level(
            organized_folders, account_row, account_index + 1, current_index, 0
        )

    def add_folder_level(
        self, folder_dict, account_row, insert_position, current_index, level
    ):
        for folder_name, folder_data in sorted(folder_dict.items()):
            if level == 0 and folder_data["full_path"].upper() == "INBOX":
                continue
            if folder_data["is_error"]:
                logging.debug(f"Creating error folder: {folder_data['name']}")
                error_icon = AppIcon("dialog-error-symbolic", class_names="error-icon")
                error_text = AppText(folder_data["name"], class_names="error-text")

                error_box = ContentContainer(
                    class_names="error-content",
                    children=[error_icon.widget, error_text.widget],
                )

                error_button = AppButton(
                    variant="expand",
                    class_names=["error-button"],
                    children=error_box.widget,
                )
                error_container = ButtonContainer(
                    class_names="error-container",
                    children=[error_button.widget],
                    with_margin=True,
                    margin_start=THEME_INDENT_STEP * (level + 1),
                )

                error_row = ContentItem(
                    class_names="error-folder-item", children=error_container.widget
                )
                setattr(error_row, "is_folder", True)
                setattr(error_row, "folder_name", folder_data["name"])
                setattr(error_row, "parent_account", account_row.account_data)
                setattr(error_row, "main_box", error_container)
                setattr(error_row, "folder_button", error_button)
                setattr(error_row.widget, "main_box", error_container)
                setattr(error_row.widget, "folder_button", error_button)

                self.sidebar_list.widget.insert(
                    error_row.widget, insert_position + current_index
                )
                current_index += 1
            else:
                icon_name = self.get_folder_icon(folder_data["full_path"])
                folder_text = AppText(folder_data["name"], class_names="folder-text")

                account_key = account_row.account_data["email"]
                folder_key = f"{account_key}:{folder_data['full_path']}"
                is_expanded = self.expanded_folders.get(folder_key, False)
                has_children = bool(folder_data.get("children"))

                if has_children:
                    arrow_icon = AppIcon(
                        "pan-end-symbolic" if not is_expanded else "pan-down-symbolic",
                        class_names="arrow-icon",
                    )
                    folder_box = ContentContainer(
                        class_names="folder-content",
                        children=[arrow_icon.widget, folder_text.widget],
                    )
                else:
                    folder_icon = AppIcon(icon_name, class_names="folder-icon")
                    folder_box = ContentContainer(
                        class_names="folder-content",
                        children=[folder_icon.widget, folder_text.widget],
                    )

                folder_button = AppButton(
                    variant="expand",
                    h_fill=True,
                    class_names=["folder-button"],
                    children=folder_box.widget,
                )

                folder_container = ButtonContainer(
                    class_names="folder-container",
                    children=[folder_button.widget],
                    with_margin=True,
                    margin_start=THEME_INDENT_STEP * (level + 1),
                )

                folder_row = ContentItem(
                    class_names="folder-item", children=folder_container.widget
                )
                setattr(folder_row, "is_folder", True)
                setattr(folder_row, "folder_name", folder_data["name"])
                setattr(folder_row, "full_path", folder_data["full_path"])
                setattr(folder_row, "parent_account", account_row.account_data)
                setattr(folder_row, "level", level)
                setattr(folder_row, "children_data", folder_data.get("children", {}))
                setattr(folder_row, "has_children", has_children)
                setattr(folder_row, "main_box", folder_container)
                setattr(folder_row, "folder_button", folder_button)
                setattr(folder_row.widget, "main_box", folder_container)
                setattr(folder_row.widget, "folder_button", folder_button)
                setattr(folder_row.widget, "parent_account", account_row.account_data)
                setattr(folder_row.widget, "full_path", folder_data["full_path"])
                setattr(folder_row.widget, "level", level)
                setattr(folder_row.widget, "has_children", has_children)
                setattr(
                    folder_row.widget, "children_data", folder_data.get("children", {})
                )

                folder_button.connect(
                    "clicked", self.on_folder_button_clicked, folder_row
                )

                self.sidebar_list.widget.insert(
                    folder_row.widget, insert_position + current_index
                )
                current_index += 1

                if has_children:
                    account_key = account_row.account_data["email"]
                    folder_key = f"{account_key}:{folder_data['full_path']}"
                    if self.expanded_folders.get(folder_key, False):
                        current_index += self.add_folder_level(
                            folder_data["children"],
                            account_row,
                            insert_position + current_index,
                            0,
                            level + 1,
                        )

        return current_index

    def expand_folder(self, folder_row):
        account_key = folder_row.parent_account["email"]
        folder_key = f"{account_key}:{folder_row.full_path}"

        self.expanded_folders[folder_key] = True

        if getattr(folder_row, "has_children"):
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-down-symbolic")

        folder_index = 0
        for i, row in enumerate(self.get_sidebar_rows()):
            if row == folder_row.widget:
                folder_index = i
                break

        account_row = None
        for row in self.get_sidebar_rows():
            if (
                hasattr(row, "account_data")
                and getattr(row, "account_data")["email"] == account_key
            ):
                account_row = row
                break

        if account_row:
            self.add_folder_level(
                folder_row.children_data,
                account_row,
                folder_index + 1,
                0,
                folder_row.level + 1,
            )

    def collapse_folder(self, folder_row):
        account_key = folder_row.parent_account["email"]
        folder_key = f"{account_key}:{folder_row.full_path}"

        self.expanded_folders[folder_key] = False

        if getattr(folder_row, "has_children"):
            folder_box = folder_row.folder_button.get_child()
            arrow_icon = folder_box.get_first_child()
            arrow_icon.set_from_icon_name("pan-end-symbolic")

        self.remove_folder_descendants(folder_row)

    def remove_folder_descendants(self, parent_folder_row):
        rows_to_remove = []
        parent_level = parent_folder_row.level
        account_key = parent_folder_row.parent_account["email"]
        parent_full_path = parent_folder_row.full_path

        for row in self.get_sidebar_rows():
            if (
                hasattr(row, "level")
                and hasattr(row, "parent_account")
                and hasattr(row, "full_path")
            ):
                if (
                    getattr(row, "parent_account")["email"] == account_key
                    and getattr(row, "level") > parent_level
                    and getattr(row, "full_path").startswith(parent_full_path + "/")
                ):
                    rows_to_remove.append(row)
                    descendant_key = f"{account_key}:{row.full_path}"
                    if descendant_key in self.expanded_folders:
                        self.expanded_folders[descendant_key] = False

        for row in rows_to_remove:
            self.sidebar_list.widget.remove(row)

    def collapse_account(self, account_row):
        setattr(account_row, "expanded", False)
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
            if (
                hasattr(row, "parent_account")
                and getattr(row, "parent_account")["email"] == account_email
            ):
                rows_to_remove.append(row)

        for row in rows_to_remove:
            self.sidebar_list.widget.remove(row)

    def get_sidebar_rows(self):
        rows = []
        index = 0
        while True:
            row = self.sidebar_list.widget.get_row_at_index(index)
            if row is None:
                break
            rows.append(row)
            index += 1
        return rows

    def load_accounts(self):
        try:
            bus = dbus.SessionBus()
            goa_proxy = bus.get_object(
                "org.gnome.OnlineAccounts", "/org/gnome/OnlineAccounts"
            )

            obj_manager = dbus.Interface(
                goa_proxy, "org.freedesktop.DBus.ObjectManager"
            )

            managed_objects = obj_manager.GetManagedObjects()

            found_accounts = False
            self.accounts_data = []

            for path, interfaces in managed_objects.items():
                if "org.gnome.OnlineAccounts.Account" in interfaces:
                    account = interfaces["org.gnome.OnlineAccounts.Account"]

                    if (
                        path in managed_objects
                        and "org.gnome.OnlineAccounts.Mail" in managed_objects[path]
                    ):
                        found_accounts = True

                        account_name = account.get("PresentationIdentity", "Unknown")
                        provider = account.get("ProviderName", "Unknown")

                        mail = managed_objects[path]["org.gnome.OnlineAccounts.Mail"]
                        email_address = mail.get("EmailAddress", "No email address")

                        if (
                            account_name == "Unknown" or not account_name
                        ) and email_address != "No email address":
                            account_name = email_address

                        has_oauth2 = (
                            "org.gnome.OnlineAccounts.OAuth2Based"
                            in managed_objects[path]
                        )

                        account_data = {
                            "path": path,
                            "account_name": account_name,
                            "provider": provider,
                            "email": email_address,
                            "has_oauth2": has_oauth2,
                        }

                        self.accounts_data.append(account_data)

                        expand_button = AppButton(
                            variant="expand",
                            class_names=["expand-button", "account-expand"],
                        )
                        expand_button.set_icon_name("pan-end-symbolic")

                        account_icon = AppIcon(
                            "mail-unread-symbolic", class_names="account-icon"
                        )
                        account_box = ContentContainer(
                            spacing=6,
                            class_names="account-content",
                            children=[
                                account_icon.widget,
                                AppText(
                                    text=account_data["account_name"],
                                    class_names=["account-text"],
                                ).widget,
                            ],
                        )

                        account_button = AppButton(
                            variant="primary",
                            h_fill=True,
                            class_names=["account-button"],
                            children=account_box.widget,
                        )

                        account_container = ButtonContainer(
                            class_names="account-container",
                            spacing=6,
                            children=[expand_button.widget, account_button.widget],
                        )

                        account_row = ContentItem(
                            class_names="account-item",
                            children=account_container.widget,
                        )
                        setattr(account_row, "account_data", account_data)

                        expand_button.connect(
                            "clicked", self.on_expand_clicked, account_row
                        )
                        account_button.connect(
                            "clicked", self.on_account_button_clicked, account_row
                        )
                        setattr(account_row, "main_box", account_container)
                        setattr(account_row, "expand_button", expand_button)
                        setattr(account_row, "account_button", account_button)
                        setattr(account_row, "account_icon_widget", account_icon.widget)
                        setattr(
                            account_row, "account_icon_container", account_box.widget
                        )
                        setattr(account_row.widget, "main_box", account_container)
                        setattr(account_row.widget, "expand_button", expand_button)
                        setattr(account_row.widget, "account_button", account_button)
                        setattr(account_row.widget, "account_data", account_data)

                        account_row.widget.set_selectable(True)

                        self.sidebar_list.widget.append(account_row.widget)

            if not found_accounts:
                no_accounts_container = ContentContainer(
                    spacing=6,
                    orientation=Gtk.Orientation.VERTICAL,
                    class_names="no-accounts-content",
                    children=[
                        AppText(
                            text="No accounts found",
                            class_names=["no-accounts-text", "dim-label"],
                        ).widget
                    ],
                    margin_top=THEME_MARGIN_LARGE,
                )

                no_accounts_row = ContentItem(
                    class_names="no-accounts-item",
                    children=no_accounts_container.widget,
                )
                self.sidebar_list.widget.append(no_accounts_row.widget)

        except Exception as e:
            error_container = ContentContainer(
                spacing=6,
                orientation=Gtk.Orientation.VERTICAL,
                class_names="error-content",
                children=[
                    AppText(
                        text=f"Error loading accounts: {str(e)}",
                        class_names=["error-text", "dim-label"],
                    ).widget
                ],
                margin_top=THEME_MARGIN_LARGE,
            )

            error_row = ContentItem(
                class_names="error-item", children=error_container.widget
            )
            self.sidebar_list.widget.append(error_row.widget)
