from utils.toolkit import Gtk, Adw, GLib
from components.ui import AppIcon, AppText
from components.button import AppButton

from utils.thread_grouping import group_messages_into_threads
from .message_row import MessageRow

import threading
import logging
from utils.mail import fetch_messages_from_folder
from utils.sync_service import SyncService


class MessageList:
    def __init__(self, storage, imap_backend):
        self.storage = storage
        self.imap_backend = imap_backend
        self.current_folder = None
        self.current_account_data = None
        self.messages = []
        self.threads = []
        self.message_selected_callback = None
        self.threading_enabled = True
        self.message_row_instances = {}
        self.header = None

        # Initialize sync service
        self.sync_service = SyncService(storage, sync_interval=300)
        self.sync_service.add_sync_callback(self.on_sync_event)
        self.sync_service.start()

        self.widget = Adw.PreferencesGroup()
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        self.widget.set_margin_top(6)
        self.widget.set_margin_bottom(6)
        self.widget.set_margin_start(6)
        self.widget.set_margin_end(6)

        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.list_box.connect("row-activated", self.on_message_selected)
        self.list_box.add_css_class("boxed-list")

        self.widget.add(self.list_box)

        self.empty_state = self.create_empty_state()
        self.loading_state = self.create_loading_state()
        self.error_state = self.create_error_state()

        self.show_empty_state()

    def create_empty_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-empty-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)

        icon = AppIcon("mail-unread-symbolic", class_names="message-list-empty-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)

        text = AppText(
            "No messages in this folder",
            halign=Gtk.Align.CENTER,
            class_names="message-list-empty-text",
        )
        text.set_opacity(0.7)

        container.append(icon.widget)
        container.append(text.widget)

        return container

    def create_loading_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-loading-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)

        spinner = Gtk.Spinner()
        spinner.add_css_class("message-list-loading-spinner")
        spinner.set_spinning(True)

        text = AppText(
            "Loading messages...",
            halign=Gtk.Align.CENTER,
            class_names="message-list-loading-text",
        )

        container.append(spinner)
        container.append(text.widget)

        return container

    def create_error_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-error-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)

        icon = AppIcon("dialog-error-symbolic", class_names="message-list-error-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)

        text = AppText(
            "Failed to load messages",
            halign=Gtk.Align.CENTER,
            class_names="message-list-error-text",
        )
        text.set_opacity(0.7)

        retry_button = AppButton(class_names="message-list-retry-button")
        retry_button.widget.set_label("Retry")
        retry_button.connect("clicked", self.on_retry_clicked)

        container.append(icon.widget)
        container.append(text.widget)
        container.append(retry_button.widget)

        return container

    def set_folder(self, folder):
        logging.debug(f"MessageList: Setting folder to {folder}")
        self.current_folder = folder

        # Update sync service with current folder
        if self.sync_service:
            self.sync_service.set_current_folder(folder)

        if folder:
            logging.debug(f"MessageList: Loading messages for folder {folder}")
            self.load_messages()
        else:
            logging.debug("MessageList: No folder selected, showing empty state")
            self.show_empty_state()

    def set_account_data(self, account_data):
        logging.debug(f"MessageList: Setting account_data to {account_data}")

        # Unregister previous account if any
        if self.current_account_data:
            self.sync_service.unregister_account(self.current_account_data["email"])

        self.current_account_data = account_data

        # Register new account with sync service if we have account data
        if account_data:
            self.sync_service.register_account(account_data)

    def load_messages(self, force_refresh=False):
        if not self.current_folder:
            logging.debug("MessageList: No current folder, cannot load messages")
            return

        if not self.current_account_data:
            logging.debug("MessageList: No current account_data, cannot load messages")
            return

        account_id = self.current_account_data["email"]
        logging.info(
            f"MessageList: Loading messages for folder {self.current_folder}, account_id {account_id}, force_refresh={force_refresh}"
        )

        def fetch_messages():
            try:
                if not force_refresh:
                    logging.debug(
                        f"MessageList: Fetching messages from storage for folder {self.current_folder}, account_id {account_id}"
                    )
                    messages = self.storage.get_messages(
                        self.current_folder, account_id
                    )
                    logging.debug(
                        f"MessageList: Retrieved {len(messages)} messages from storage"
                    )

                    if not messages:
                        logging.debug(
                            "MessageList: No messages in storage, fetching from IMAP"
                        )
                        # Show loading state in header
                        if self.header:
                            GLib.idle_add(self.header.set_loading, True)
                        self.fetch_from_imap()
                        return

                    # Show cached messages immediately
                    GLib.idle_add(self.on_messages_loaded, messages)

                    # Show loading state in header for background refresh
                    if self.header:
                        GLib.idle_add(self.header.set_loading, True)

                    # Trigger background refresh
                    self.fetch_from_imap()
                else:
                    logging.debug(
                        "MessageList: Force refresh requested, fetching from IMAP"
                    )
                    # Show loading state in header
                    if self.header:
                        GLib.idle_add(self.header.set_loading, True)
                    self.fetch_from_imap()
            except Exception as e:
                logging.error(
                    f"MessageList: Error fetching messages for folder {self.current_folder}: {e}"
                )
                logging.debug(
                    f"MessageList: Exception details: {type(e).__name__}: {str(e)}"
                )
                GLib.idle_add(self.on_messages_error, str(e))

        logging.debug(
            f"MessageList: Starting thread to fetch messages for folder {self.current_folder}"
        )
        thread = threading.Thread(target=fetch_messages)
        thread.daemon = True
        thread.start()

    def on_messages_loaded(self, messages):
        logging.info(
            f"MessageList: Messages loaded successfully, count: {len(messages)}"
        )
        logging.debug(f"MessageList: Threading enabled: {self.threading_enabled}")
        self.messages = messages

        # Hide loading state in header
        if self.header:
            GLib.idle_add(self.header.set_loading, False)

        if self.threading_enabled:
            logging.debug("MessageList: Grouping messages into threads")
            self.threads = group_messages_into_threads(messages)
            logging.debug(f"MessageList: Created {len(self.threads)} threads")
            self.populate_threaded_list()
        else:
            logging.debug("MessageList: Populating message list (no threading)")
            self.populate_message_list()

    def on_messages_error(self, error_message):
        logging.error(f"MessageList: Error loading messages: {error_message}")
        # Hide loading state in header
        if self.header:
            GLib.idle_add(self.header.set_loading, False)
        self.show_error_state()

    def populate_message_list(self):
        logging.debug("MessageList: Populating message list")
        self.clear_list()

        if not self.messages:
            logging.debug("MessageList: No messages to display, showing empty state")
            self.show_empty_state()
            return

        logging.debug(f"MessageList: Displaying {len(self.messages)} messages")
        self.show_message_list()

        for i, message in enumerate(self.messages):
            logging.debug(
                f"MessageList: Creating message row {i+1}/{len(self.messages)}"
            )
            message_row = MessageRow(message)
            message_row.connect_selected(self.on_message_row_selected)
            self.message_row_instances[message_row.widget] = message_row
            self.list_box.append(message_row.widget)

        logging.debug("MessageList: Message list population complete")

    def populate_threaded_list(self):
        logging.debug("MessageList: Populating threaded list")
        self.clear_list()

        if not self.threads:
            logging.debug("MessageList: No threads to display, showing empty state")
            self.show_empty_state()
            return

        logging.debug(f"MessageList: Displaying {len(self.threads)} threads")
        self.show_message_list()

        for i, thread in enumerate(self.threads):
            logging.debug(f"MessageList: Creating thread row {i+1}/{len(self.threads)}")
            thread_row = MessageRow(thread)
            thread_row.connect_selected(self.on_message_row_selected)
            self.message_row_instances[thread_row.widget] = thread_row
            self.list_box.append(thread_row.widget)

        logging.debug("MessageList: Threaded list population complete")

    def fetch_from_imap(self):
        if not self.current_account_data:
            logging.error("MessageList: No account_data available for IMAP fetch")
            GLib.idle_add(self.on_messages_error, "No account selected")
            return

        account_data = self.current_account_data

        logging.debug(
            f"MessageList: Fetching messages from IMAP for folder {self.current_folder}"
        )

        def on_imap_response(error, messages):
            if error:
                logging.error(f"MessageList: IMAP fetch error: {error}")
                GLib.idle_add(self.on_messages_error, error)
                return

            if messages:
                logging.debug(
                    f"MessageList: Received {len(messages)} messages from IMAP"
                )
                try:
                    self.storage.store_messages(
                        messages, self.current_folder, account_data["email"]
                    )
                    logging.debug(
                        f"MessageList: Stored {len(messages)} messages in database"
                    )
                except Exception as e:
                    logging.error(f"MessageList: Error storing messages: {e}")

                GLib.idle_add(self.on_messages_loaded, messages)
            else:
                logging.debug("MessageList: No messages received from IMAP")
                GLib.idle_add(self.on_messages_loaded, [])

        fetch_messages_from_folder(account_data, self.current_folder, on_imap_response)

    def on_message_row_selected(self, message):
        if self.message_selected_callback:
            self.message_selected_callback(message)

    def clear_list(self):
        self.message_row_instances.clear()
        while True:
            row = self.list_box.get_first_child()
            if row is None:
                break
            self.list_box.remove(row)

    def on_message_selected(self, list_box, row):
        pass

    def on_retry_clicked(self, button):
        logging.debug("MessageList: Retry button clicked, reloading messages")
        self.load_messages()

    def show_empty_state(self):
        self.hide_all_states()
        self.widget.add(self.empty_state)

    def show_loading_state(self):
        self.hide_all_states()
        self.widget.add(self.loading_state)

    def show_message_list(self):
        self.hide_all_states()

    def show_error_state(self):
        self.hide_all_states()
        self.widget.add(self.error_state)

    def hide_all_states(self):
        if self.empty_state.get_parent():
            self.widget.remove(self.empty_state)
        if self.loading_state.get_parent():
            self.widget.remove(self.loading_state)
        if self.error_state.get_parent():
            self.widget.remove(self.error_state)

    def connect_message_selected(self, callback):
        self.message_selected_callback = callback

    def get_selected_message(self):
        selected_row = self.list_box.get_selected_row()
        if selected_row:
            if selected_row in self.message_row_instances:
                message_row = self.message_row_instances[selected_row]
                if message_row.is_thread:
                    # For threads, return the latest message
                    return (
                        message_row.message_or_thread.messages[-1]
                        if message_row.message_or_thread.messages
                        else None
                    )
                else:
                    return message_row.message_or_thread
        return None

    def set_threading_enabled(self, enabled):
        self.threading_enabled = enabled
        if self.messages:
            if enabled:
                self.threads = group_messages_into_threads(self.messages)
                self.populate_threaded_list()
            else:
                self.populate_message_list()

    def cleanup(self):
        if self.sync_service:
            self.sync_service.stop()
            logging.info("MessageList: Sync service stopped")

    def set_header(self, header):
        self.header = header

    def refresh_messages(self):
        logging.info("MessageList: Force refreshing messages from IMAP")
        self.load_messages(force_refresh=True)

    def sync_folder_manually(self, folder_name: str):
        if not self.current_account_data:
            logging.warning("MessageList: No account data available for manual sync")
            return

        logging.info(f"MessageList: Manual sync requested for folder {folder_name}")
        self.sync_service.sync_folder(
            self.current_account_data, folder_name, force=True
        )

    def on_sync_event(self, event_type: str, account_id: str, folder_name: str, data):
        if event_type == "folder_discovery_complete":
            logging.info(
                f"MessageList: Folder discovery completed for {account_id}, found {len(data)} folders"
            )
        elif event_type == "folder_discovery_error":
            logging.error(
                f"MessageList: Folder discovery failed for {account_id}: {data}"
            )
        elif event_type == "sync_complete":
            if (
                account_id == self.current_account_data.get("email")
                and folder_name == self.current_folder
            ):
                logging.info(
                    f"MessageList: Sync completed for {folder_name}, {data} messages"
                )
                try:
                    messages = self.storage.get_messages(folder_name, account_id)
                    GLib.idle_add(self.on_messages_loaded, messages)
                except Exception as e:
                    logging.error(
                        f"MessageList: Error reloading messages after sync: {e}"
                    )
        elif event_type == "sync_error":
            if (
                account_id == self.current_account_data.get("email")
                and folder_name == self.current_folder
            ):
                logging.error(f"MessageList: Sync error for {folder_name}: {data}")
