from utils.toolkit import Gtk, Adw, GLib
from components.ui import AppIcon, AppText
from components.button import AppButton

from .states import MessageListStates
from .loader import MessageLoader
from .renderer import MessageRenderer
from .search import MessageSearch
from .sync_handler import MessageSyncHandler

import logging

class MessageList:
    def __init__(self, storage, imap_backend):
        self.storage = storage
        self.imap_backend = imap_backend
        self.current_folder = None
        self.current_account_data = None
        self.messages = []
        self.message_selected_callback = None
        self.header = None

        self.widget = Adw.PreferencesGroup()
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        self.widget.add_css_class("message-list-root")

        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.list_box.connect("row-selected", self.on_message_selected)
        self.list_box.add_css_class("boxed-list")

        self.widget.add(self.list_box)

        self.states = MessageListStates(self.widget)
        self.loader = MessageLoader(storage, imap_backend)
        self.renderer = MessageRenderer(self.list_box)
        self.search = MessageSearch()
        self.sync_handler = MessageSyncHandler(storage)

        self.loader.connect_callbacks(self.on_messages_loaded, self.on_messages_error)
        self.sync_handler.connect_messages_loaded_callback(self.on_messages_loaded)

        self.states.show_empty()

    def set_folder(self, folder):
        logging.debug(f"MessageList: Setting folder to {folder}")
        
        self.clear_list()
        self.messages = []
        
        self.current_folder = folder
        self.loader.set_folder(folder)
        self.sync_handler.set_folder(folder)

        if folder:
            logging.debug(f"MessageList: Loading messages for folder {folder}")
            self.load_messages()
        else:
            logging.debug("MessageList: No folder selected, showing empty state")
            self.states.show_empty()

    def set_account_data(self, account_data):
        logging.debug(f"MessageList: Setting account_data to {account_data}")

        self.current_account_data = account_data
        self.loader.set_account_data(account_data)
        self.sync_handler.set_account_data(account_data)
        self.renderer.set_storage_and_account(self.storage, account_data)

    def load_messages(self, force_refresh=False):
        if not self.current_folder:
            logging.debug("MessageList: No current folder, cannot load messages")
            return

        if not self.current_account_data:
            logging.debug("MessageList: No current account_data, cannot load messages")
            return

        logging.info(
            f"MessageList: Loading messages for folder {self.current_folder}, force_refresh={force_refresh}"
        )

        self.loader.load_messages(force_refresh)

    def on_messages_loaded(self, messages):
        logging.info(
            f"MessageList: Messages loaded successfully, count: {len(messages)}"
        )
        
        if self.messages and len(self.messages) == len(messages):
            old_uids = {msg.get('uid') for msg in self.messages}
            new_uids = {msg.get('uid') for msg in messages}
            if old_uids == new_uids:
                logging.debug("MessageList: Message UIDs unchanged, skipping re-render")
                self.messages = messages  
                if self.header:
                    GLib.idle_add(self.header.set_loading, False)
                return
        
        self.messages = messages

        if self.header:
            GLib.idle_add(self.header.set_loading, False)

        self.apply_search_filter()

    def on_messages_error(self, error_message):
        logging.error(f"MessageList: Error loading messages: {error_message}")
        
        if self.header:
            GLib.idle_add(self.header.set_loading, False)
        
        self.states.show_error(error_message)

    def on_message_row_selected(self, message_or_thread):
        if self.message_selected_callback:
            self.message_selected_callback(message_or_thread)

    def clear_list(self):
        self.renderer.clear_list()

    def on_message_selected(self, list_box, row):
        self.renderer.handle_selection(list_box, row, self.message_selected_callback)

    def connect_message_selected(self, callback):
        self.message_selected_callback = callback

    def get_selected_message(self):
        return self.renderer.get_selected_message()

    def set_threading_enabled(self, enabled):
        self.renderer.set_threading_enabled(enabled)
        if self.messages:
            current_messages = self.search.get_filtered_messages() if self.search.has_search_text() else self.messages
            self.renderer.render_message_list(
                current_messages, 
                grouped=not self.search.has_search_text(), 
                preserve_selection=True,
                on_row_selected_callback=self.on_message_row_selected
            )

    def cleanup(self):
        self.sync_handler.cleanup()

    def set_header(self, header):
        self.header = header
        self.loader.set_header(header)
        if header:
            header.connect_search(self.on_search_changed)

    def refresh_messages(self):
        logging.info("MessageList: Force refreshing messages from IMAP")
        self.load_messages(force_refresh=True)

    def refresh_message_display(self):
        """Refresh the UI display of messages without reloading data"""
        logging.debug("MessageList: Refreshing message display UI")
        if self.messages:
            self.apply_search_filter()
        
    def on_search_changed(self, search_text):
        self.search.set_search_text(search_text)
        self.apply_search_filter()
        
    def apply_search_filter(self):
        if not self.messages:
            return
            
        filtered_messages, should_group = self.search.apply_filter(self.messages)
        
        if not filtered_messages:
            self.states.show_empty()
        else:
            self.states.show_list()
            self.renderer.render_message_list(
                filtered_messages, 
                grouped=should_group, 
                preserve_selection=not self.search.has_search_text(),
                on_row_selected_callback=self.on_message_row_selected
            )

    def sync_folder_manually(self, folder_name: str):
        self.sync_handler.sync_folder_manually(folder_name)
