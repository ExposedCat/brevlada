from utils.toolkit import GLib
from utils.sync_service import SyncService
import logging

class MessageSyncHandler:
    def __init__(self, storage):
        self.storage = storage
        self.current_folder = None
        self.current_account_data = None
        
        self.sync_service = SyncService(storage, sync_interval=300)
        self.sync_service.add_sync_callback(self.on_sync_event)
        self.sync_service.start()
        
        self.messages_loaded_callback = None

    def set_folder(self, folder):
        self.current_folder = folder
        if self.sync_service:
            self.sync_service.set_current_folder(folder)

    def set_account_data(self, account_data):
        if self.current_account_data:
            self.sync_service.unregister_account(self.current_account_data["email"])

        self.current_account_data = account_data

        if account_data:
            self.sync_service.register_account(account_data)

    def connect_messages_loaded_callback(self, callback):
        self.messages_loaded_callback = callback

    def sync_folder_manually(self, folder_name: str):
        if not self.current_account_data:
            logging.warning("MessageSyncHandler: No account data available for manual sync")
            return

        logging.info(f"MessageSyncHandler: Manual sync requested for folder {folder_name}")
        self.sync_service.sync_folder(
            self.current_account_data, folder_name, force=True
        )

    def on_sync_event(self, event_type: str, account_id: str, folder_name: str, data):
        if event_type == "folder_discovery_complete":
            logging.info(
                f"MessageSyncHandler: Folder discovery completed for {account_id}, found {len(data)} folders"
            )
        elif event_type == "folder_discovery_error":
            logging.error(
                f"MessageSyncHandler: Folder discovery failed for {account_id}: {data}"
            )
        elif event_type == "sync_complete":
            if (
                self.current_account_data and 
                account_id == self.current_account_data.get("email")
                and folder_name == self.current_folder
            ):
                logging.info(
                    f"MessageSyncHandler: Sync completed for {folder_name}, {data} messages"
                )
                try:
                    messages = self.storage.get_messages(folder_name, account_id)
                    if self.messages_loaded_callback:
                        GLib.idle_add(self.messages_loaded_callback, messages)
                except Exception as e:
                    logging.error(
                        f"MessageSyncHandler: Error reloading messages after sync: {e}"
                    )
        elif event_type == "sync_error":
            if (
                self.current_account_data and 
                account_id == self.current_account_data.get("email")
                and folder_name == self.current_folder
            ):
                logging.error(f"MessageSyncHandler: Sync error for {folder_name}: {data}")

    def cleanup(self):
        if self.sync_service:
            self.sync_service.stop()
            logging.info("MessageSyncHandler: Sync service stopped") 