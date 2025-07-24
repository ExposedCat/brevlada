from utils.toolkit import Gtk
from utils.thread_grouping import group_messages_into_threads
from .message_row import MessageRow
import logging

class MessageRenderer:
    def __init__(self, list_box):
        self.list_box = list_box
        self.message_row_instances = {}
        self.threading_enabled = True
        self._restoring_selection = False

    def clear_list(self):
        self.message_row_instances.clear()
        while True:
            row = self.list_box.get_first_child()
            if row is None:
                break
            self.list_box.remove(row)

    def render_message_list(self, messages, grouped=True, preserve_selection=True, on_row_selected_callback=None):
        logging.debug(f"MessageRenderer: Rendering {len(messages)} messages, grouped={grouped}, preserve_selection={preserve_selection}")
        
        currently_selected = None
        if preserve_selection:
            currently_selected = self._get_currently_selected()
            if currently_selected:
                if hasattr(currently_selected, 'messages'):
                    selected_id = f"thread_{currently_selected.subject}"
                    logging.debug(f"MessageRenderer: Preserving thread selection: {selected_id}")
                else:
                    selected_id = f"message_{currently_selected.get('uid')}"
                    logging.debug(f"MessageRenderer: Preserving message selection: {selected_id}")
        
        self.clear_list()

        if not messages:
            logging.debug("MessageRenderer: No messages to render")
            return

        logging.debug(f"MessageRenderer: Displaying {len(messages)} messages")

        if grouped and self.threading_enabled:
            logging.debug("MessageRenderer: Grouping messages into threads")
            threads = group_messages_into_threads(messages)
            logging.debug(f"MessageRenderer: Created {len(threads)} threads")
            
            for i, thread in enumerate(threads):
                logging.debug(f"MessageRenderer: Creating thread row {i+1}/{len(threads)} with {len(thread.messages)} messages")
                thread_row = MessageRow(thread)
                if on_row_selected_callback:
                    thread_row.connect_selected(on_row_selected_callback)
                self.message_row_instances[thread_row.widget] = thread_row
                self.list_box.append(thread_row.widget)
                
                if preserve_selection and currently_selected and hasattr(currently_selected, 'messages'):
                    if thread.subject == currently_selected.subject:
                        logging.debug(f"MessageRenderer: Restoring selection to thread: {thread.subject}")
                        self._restoring_selection = True
                        self.list_box.select_row(thread_row.widget)
                        self._restoring_selection = False
        else:
            logging.debug("MessageRenderer: Rendering messages without grouping")
            for i, message in enumerate(messages):
                logging.debug(f"MessageRenderer: Creating message row {i+1}/{len(messages)}")
                message_row = MessageRow(message)
                if on_row_selected_callback:
                    message_row.connect_selected(on_row_selected_callback)
                self.message_row_instances[message_row.widget] = message_row
                self.list_box.append(message_row.widget)
                
                if preserve_selection and currently_selected and not hasattr(currently_selected, 'messages'):
                    if message.get('uid') == currently_selected.get('uid'):
                        logging.debug(f"MessageRenderer: Restoring selection to message UID: {message.get('uid')}")
                        self._restoring_selection = True
                        self.list_box.select_row(message_row.widget)
                        self._restoring_selection = False

        logging.debug("MessageRenderer: Message list rendering complete")

    def get_selected_message(self):
        selected_row = self.list_box.get_selected_row()
        if selected_row:
            if selected_row in self.message_row_instances:
                message_row = self.message_row_instances[selected_row]
                if message_row.is_thread:
                    return message_row.message_or_thread
                else:
                    return message_row.message_or_thread
        return None

    def _get_currently_selected(self):
        return self.get_selected_message()

    def is_restoring_selection(self):
        return self._restoring_selection

    def set_threading_enabled(self, enabled):
        self.threading_enabled = enabled

    def handle_selection(self, list_box, row, callback):
        if self._restoring_selection:
            logging.debug("MessageRenderer: Ignoring selection change during restoration")
            return
            
        if row is not None and row in self.message_row_instances:
            message_row = self.message_row_instances[row]
            if message_row.is_thread:
                message_or_thread = message_row.message_or_thread
            else:
                message_or_thread = message_row.message_or_thread
            if callback:
                callback(message_or_thread) 