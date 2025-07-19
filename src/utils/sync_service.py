import threading
import time
import logging
from typing import Dict, List, Optional, Callable
from utils.mail import fetch_messages_from_folder, fetch_imap_folders


class SyncService:
    """Background service for automatic message synchronization"""
    
    def __init__(self, storage, sync_interval: int = 300):  # 5 minutes default
        self.storage = storage
        self.sync_interval = sync_interval
        self.running = False
        self.sync_thread = None
        self.sync_callbacks: List[Callable] = []
        self.accounts_to_sync: Dict[str, Dict] = {}  # account_id -> account_data
        self.all_folders: Dict[str, List[str]] = {}  # account_id -> [all_folders]
        self.current_folder: Optional[str] = None  # currently opened folder
        self.folder_discovery_complete: Dict[str, bool] = {}  # account_id -> bool
        
    def start(self):
        """Start the background sync service"""
        if self.running:
            return
            
        self.running = True
        self.sync_thread = threading.Thread(target=self._sync_loop, daemon=True)
        self.sync_thread.start()
        logging.info("SyncService: Background sync service started")
        
    def stop(self):
        """Stop the background sync service"""
        self.running = False
        if self.sync_thread:
            self.sync_thread.join(timeout=5)
        logging.info("SyncService: Background sync service stopped")
        
    def add_sync_callback(self, callback: Callable):
        """Add callback to be called when sync events occur"""
        self.sync_callbacks.append(callback)
        
    def register_account(self, account_data: Dict):
        """Register an account for automatic sync and start folder discovery in background"""
        account_id = account_data['email']
        self.accounts_to_sync[account_id] = account_data
        self.folder_discovery_complete[account_id] = False
        
        # Start folder discovery in background thread
        logging.info(f"SyncService: Starting background folder discovery for {account_id}")
        discovery_thread = threading.Thread(
            target=self._discover_folders_background, 
            args=(account_data,),
            daemon=True
        )
        discovery_thread.start()
        
    def unregister_account(self, account_id: str):
        """Unregister an account from automatic sync"""
        if account_id in self.accounts_to_sync:
            del self.accounts_to_sync[account_id]
        if account_id in self.all_folders:
            del self.all_folders[account_id]
        if account_id in self.folder_discovery_complete:
            del self.folder_discovery_complete[account_id]
        logging.info(f"SyncService: Unregistered account {account_id}")
        
    def set_current_folder(self, folder_name: str):
        """Set the currently opened folder for priority syncing"""
        self.current_folder = folder_name
        logging.debug(f"SyncService: Current folder set to {folder_name}")
        
    def sync_folder(self, account_data: Dict, folder_name: str, force: bool = False):
        """Manually trigger sync for a specific folder"""
        account_id = account_data['email']
        logging.info(f"SyncService: Manual sync requested for {account_id} - {folder_name}")
        
        def on_sync_complete(error, messages):
            if error:
                logging.error(f"SyncService: Sync failed for {account_id} - {folder_name}: {error}")
                self._notify_callbacks("sync_error", account_id, folder_name, error)
            else:
                if messages:
                    # Update database with new messages and remove deleted ones
                    try:
                        self._update_messages_in_db(account_id, folder_name, messages)
                        logging.info(f"SyncService: Updated {len(messages)} messages for {account_id} - {folder_name}")
                        self._notify_callbacks("sync_complete", account_id, folder_name, len(messages))
                    except Exception as e:
                        logging.error(f"SyncService: Error updating messages: {e}")
                        self._notify_callbacks("sync_error", account_id, folder_name, str(e))
                else:
                    logging.debug(f"SyncService: No messages found for {account_id} - {folder_name}")
                    self._notify_callbacks("sync_complete", account_id, folder_name, 0)
        
        fetch_messages_from_folder(account_data, folder_name, on_sync_complete)
        
    def _discover_folders_background(self, account_data: Dict):
        """Discover all folders in background thread"""
        account_id = account_data['email']
        
        def on_folders_discovered(folders):
            if isinstance(folders, list) and folders and not folders[0].startswith("Error:"):
                self.all_folders[account_id] = folders
                self.folder_discovery_complete[account_id] = True
                logging.info(f"SyncService: Discovered {len(folders)} folders for {account_id}")
                self._notify_callbacks("folder_discovery_complete", account_id, "", folders)
            else:
                logging.error(f"SyncService: Failed to discover folders for {account_id}: {folders}")
                self._notify_callbacks("folder_discovery_error", account_id, "", folders)
        
        fetch_imap_folders(account_data, on_folders_discovered)
        
    def _update_messages_in_db(self, account_id: str, folder_name: str, new_messages: List):
        """Update database: add new messages, keep existing, remove deleted ones"""
        try:
            # Get existing message UIDs from database
            existing_messages = self.storage.get_messages(folder_name, account_id)
            existing_uids = {msg['uid'] for msg in existing_messages}
            
            # Get new message UIDs
            new_uids = {msg['uid'] for msg in new_messages}
            
            # Find messages to remove (in DB but not in IMAP)
            uids_to_remove = existing_uids - new_uids
            
            # Remove deleted messages from database
            if uids_to_remove:
                logging.info(f"SyncService: Removing {len(uids_to_remove)} deleted messages from {folder_name}")
                self._remove_messages_from_db(account_id, folder_name, uids_to_remove)
            
            # Store new/updated messages
            if new_messages:
                self.storage.store_messages(new_messages, folder_name, account_id)
                
            logging.info(f"SyncService: DB update complete for {folder_name}: {len(new_messages)} messages, removed {len(uids_to_remove)}")
            
        except Exception as e:
            logging.error(f"SyncService: Error updating database for {folder_name}: {e}")
            raise
            
    def _remove_messages_from_db(self, account_id: str, folder_name: str, uids_to_remove: set):
        """Remove specific messages from database"""
        try:
            with self.storage.get_connection() as conn:
                for uid in uids_to_remove:
                    conn.execute("""
                        DELETE FROM messages 
                        WHERE uid = ? AND folder = ? AND account_id = ?
                    """, (uid, folder_name, account_id))
                    
                    # Also remove associated attachments
                    conn.execute("""
                        DELETE FROM attachments 
                        WHERE message_uid = ? AND folder = ? AND account_id = ?
                    """, (uid, folder_name, account_id))
                    
        except Exception as e:
            logging.error(f"SyncService: Error removing messages from DB: {e}")
            raise
        
    def _sync_loop(self):
        """Main sync loop that runs in background thread"""
        logging.info("SyncService: Starting background sync loop")
        
        while self.running:
            try:
                # Only sync accounts that have completed folder discovery
                for account_id, account_data in self.accounts_to_sync.items():
                    if not self.running:
                        break
                        
                    if not self.folder_discovery_complete.get(account_id, False):
                        continue  # Skip accounts still discovering folders
                    
                    # Priority sync: INBOX and current folder
                    folders_to_sync = ["INBOX"]
                    if self.current_folder and self.current_folder != "INBOX":
                        folders_to_sync.append(self.current_folder)
                    
                    for folder_name in folders_to_sync:
                        if not self.running:
                            break
                            
                        # Check if folder exists for this account
                        if account_id in self.all_folders and folder_name in self.all_folders[account_id]:
                            logging.debug(f"SyncService: Periodic sync for {account_id} - {folder_name}")
                            self.sync_folder(account_data, folder_name, force=False)
                            time.sleep(2)  # Small delay between folder syncs
                
                # Wait for next sync cycle
                for _ in range(self.sync_interval):
                    if not self.running:
                        break
                    time.sleep(1)
                    
            except Exception as e:
                logging.error(f"SyncService: Error in sync loop: {e}")
                time.sleep(60)  # Wait a minute before retrying
                
        logging.info("SyncService: Background sync loop stopped")
        
    def _notify_callbacks(self, event_type: str, account_id: str, folder_name: str, data):
        """Notify all registered callbacks of sync events"""
        for callback in self.sync_callbacks:
            try:
                callback(event_type, account_id, folder_name, data)
            except Exception as e:
                logging.error(f"SyncService: Error in sync callback: {e}")
                
    def get_sync_status(self, account_id: str, folder_name: str) -> Dict:
        """Get sync status for a specific folder"""
        return {
            'account_id': account_id,
            'folder_name': folder_name,
            'last_sync': time.time(),
            'status': 'active' if self.running else 'stopped',
            'folder_discovery_complete': self.folder_discovery_complete.get(account_id, False)
        }
        
    def get_all_folders(self, account_id: str) -> List[str]:
        """Get all folders for an account"""
        return self.all_folders.get(account_id, [])
        
    def is_folder_discovery_complete(self, account_id: str) -> bool:
        """Check if folder discovery is complete for an account"""
        return self.folder_discovery_complete.get(account_id, False) 