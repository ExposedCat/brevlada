from utils.toolkit import GLib
import threading
import logging
from utils.mail import fetch_messages_from_folder

class MessageLoader:
    def __init__(self, storage, imap_backend):
        self.storage = storage
        self.imap_backend = imap_backend
        self.current_fetch_id = 0
        self.current_folder = None
        self.current_account_data = None
        self.header = None
        
        self.messages_loaded_callback = None
        self.messages_error_callback = None

    def set_folder(self, folder):
        self.current_folder = folder
        self.current_fetch_id += 1

    def set_account_data(self, account_data):
        self.current_account_data = account_data

    def set_header(self, header):
        self.header = header

    def connect_callbacks(self, messages_loaded_callback, messages_error_callback):
        self.messages_loaded_callback = messages_loaded_callback
        self.messages_error_callback = messages_error_callback

    def load_messages(self, force_refresh=False):
        if not self.current_folder:
            logging.debug("MessageLoader: No current folder, cannot load messages")
            return

        if not self.current_account_data:
            logging.debug("MessageLoader: No current account_data, cannot load messages")
            return

        self.current_fetch_id += 1
        fetch_id = self.current_fetch_id

        account_id = self.current_account_data["email"]
        logging.info(
            f"MessageLoader: Loading messages for folder {self.current_folder}, account_id {account_id}, force_refresh={force_refresh}, fetch_id={fetch_id}"
        )

        def fetch_messages():
            try:
                if not force_refresh:
                    logging.debug(
                        f"MessageLoader: Fetching messages from storage for folder {self.current_folder}, account_id {account_id}"
                    )
                    messages = self.storage.get_messages(
                        self.current_folder, account_id
                    )
                    logging.debug(
                        f"MessageLoader: Retrieved {len(messages)} messages from storage"
                    )

                    if not messages:
                        logging.debug(
                            "MessageLoader: No messages in storage, fetching from IMAP"
                        )
                        if self.header:
                            GLib.idle_add(self.header.set_loading, True)
                        self._fetch_from_imap(fetch_id)
                        return
                    else:
                        logging.info(f"MessageLoader: Using {len(messages)} messages from storage, skipping IMAP")
                        if self.messages_loaded_callback:
                            GLib.idle_add(self.messages_loaded_callback, messages)
                        return
                else:
                    logging.debug(
                        "MessageLoader: Force refresh requested, fetching from IMAP"
                    )
                    
                    if self.header:
                        GLib.idle_add(self.header.set_loading, True)
                    self._fetch_from_imap(fetch_id)
            except Exception as e:
                logging.error(
                    f"MessageLoader: Error fetching messages for folder {self.current_folder}: {e}"
                )
                logging.debug(
                    f"MessageLoader: Exception details: {type(e).__name__}: {str(e)}"
                )
                if self.messages_error_callback:
                    GLib.idle_add(self.messages_error_callback, str(e))

        logging.debug(
            f"MessageLoader: Starting thread to fetch messages for folder {self.current_folder}"
        )
        thread = threading.Thread(target=fetch_messages)
        thread.daemon = True
        thread.start()

    def _fetch_from_imap(self, fetch_id):
        if not self.current_account_data:
            logging.error("MessageLoader: No account_data available for IMAP fetch")
            if self.messages_error_callback:
                GLib.idle_add(self.messages_error_callback, "No account selected")
            return

        account_data = self.current_account_data

        logging.debug(
            f"MessageLoader: Fetching messages from IMAP for folder {self.current_folder}, fetch_id={fetch_id}"
        )

        def on_imap_response(error, messages):
            if fetch_id != self.current_fetch_id:
                logging.debug(f"MessageLoader: Fetch operation {fetch_id} was cancelled, ignoring response")
                return
            if error:
                logging.error(f"MessageLoader: IMAP fetch error: {error}")
                if self.messages_error_callback:
                    GLib.idle_add(self.messages_error_callback, error)
                return

            if messages:
                logging.debug(
                    f"MessageLoader: Received {len(messages)} messages from IMAP"
                )
                try:
                    self.storage.store_messages(
                        messages, self.current_folder, account_data["email"]
                    )
                    logging.debug(
                        f"MessageLoader: Stored {len(messages)} messages in database"
                    )
                except Exception as e:
                    logging.error(f"MessageLoader: Error storing messages: {e}")

                if self.messages_loaded_callback:
                    GLib.idle_add(self.messages_loaded_callback, messages)
            else:
                logging.debug("MessageLoader: No messages received from IMAP")
                if self.messages_loaded_callback:
                    GLib.idle_add(self.messages_loaded_callback, [])
            
            if self.header:
                GLib.idle_add(self.header.set_loading, False)

        fetch_messages_from_folder(account_data, self.current_folder, on_imap_response) 