import imaplib
import ssl
import threading
import time
import logging
import signal
from typing import Optional, Dict, Any, Callable
from utils.toolkit import GLib


class IMAPConnection:
    """Represents a single IMAP connection with authentication"""

    def __init__(self, account_data: Dict[str, Any], mail_settings: Dict[str, Any]):
        self.account_data = account_data
        self.mail_settings = mail_settings
        self.connection: Optional[imaplib.IMAP4_SSL] = None
        self.last_used = 0
        self.is_authenticated = False
        self.lock = threading.Lock()
        # Handle dbus.String objects by converting to regular strings
        self.email = str(account_data.get("email", "unknown"))

    def connect(self) -> bool:
        """Establish connection to IMAP server"""
        with self.lock:
            try:
                if self.connection:
                    return True

                server = str(self.mail_settings.get("imap_host", "imap.gmail.com"))
                port = int(self.mail_settings.get("imap_port", 993))
                use_ssl = bool(self.mail_settings.get("imap_use_ssl", True))
                use_tls = bool(self.mail_settings.get("imap_use_tls", True))

                logging.info(
                    f"Connecting to IMAP server: {server}:{port} for {self.email}"
                )

                if use_ssl:
                    context = ssl.create_default_context()
                    context.check_hostname = False
                    context.verify_mode = ssl.CERT_NONE
                    self.connection = imaplib.IMAP4_SSL(
                        server, port, ssl_context=context
                    )
                else:
                    self.connection = imaplib.IMAP4(server, port)
                    if use_tls:
                        self.connection.starttls()

                logging.info(f"Successfully connected to IMAP server for {self.email}")
                return True

            except Exception as e:
                logging.error(f"Failed to connect to IMAP server for {self.email}: {e}")
                self.connection = None
                return False

    def authenticate(self) -> bool:
        """Authenticate the connection"""
        with self.lock:
            if not self.connection:
                if not self.connect():
                    return False

            if self.is_authenticated:
                return True

            try:
                email = str(self.account_data["email"])
                username = str(self.mail_settings.get("imap_username", email))
                auth_xoauth2 = self.account_data.get("has_oauth2", False)

                if not auth_xoauth2:
                    logging.warning(f"Account {email} does not support OAuth2")
                    return False

                # Get OAuth2 token
                token = self._get_oauth2_token()
                if not token:
                    logging.error(f"Failed to get OAuth2 token for {email}")
                    return False

                # Authenticate
                auth_string = f"user={username}\x01auth=Bearer {token}\x01\x01"

                def auth_callback(response):
                    return auth_string.encode("utf-8")

                self.connection.authenticate("XOAUTH2", auth_callback)
                self.is_authenticated = True
                logging.info(f"OAuth2 authentication successful for {email}")
                return True

            except Exception as e:
                logging.error(f"OAuth2 authentication failed for {self.email}: {e}")
                self.is_authenticated = False
                return False

    def _get_oauth2_token(self) -> Optional[str]:
        """Get OAuth2 token from GNOME Online Accounts"""
        try:
            import dbus

            bus = dbus.SessionBus()
            account_obj = bus.get_object(
                "org.gnome.OnlineAccounts", self.account_data["path"]
            )
            oauth2_props = dbus.Interface(
                account_obj, "org.gnome.OnlineAccounts.OAuth2Based"
            )
            access_token = oauth2_props.GetAccessToken()
            return access_token[0] if access_token else None
        except Exception as e:
            logging.error(f"Error getting OAuth2 token for {self.email}: {e}")
            return None

    def execute_command(self, command: str, *args) -> tuple:
        """Execute an IMAP command with automatic reconnection"""
        with self.lock:
            max_retries = 3
            for attempt in range(max_retries):
                try:
                    if not self.connection or not self.is_authenticated:
                        if not self.connect() or not self.authenticate():
                            raise Exception(
                                "Failed to establish authenticated connection"
                            )

                    self.last_used = time.time()

                    # Use the appropriate IMAP method based on command
                    if command == "LIST":
                        result = self.connection.list(*args)
                    elif command == "SELECT":
                        result = self.connection.select(*args)
                    elif command == "FETCH":
                        result = self.connection.fetch(*args)
                    elif command == "SEARCH":
                        result = self.connection.search(*args)
                    elif command == "STORE":
                        result = self.connection.store(*args)
                    elif command == "COPY":
                        result = self.connection.copy(*args)
                    elif command == "EXPUNGE":
                        result = self.connection.expunge()
                    elif command == "CLOSE":
                        result = self.connection.close()
                    elif command == "LOGOUT":
                        result = self.connection.logout()
                    else:
                        # For other commands, use _simple_command
                        result = self.connection._simple_command(command, *args)

                    return result

                except (imaplib.IMAP4.abort, ConnectionError, OSError) as e:
                    logging.warning(
                        f"IMAP connection error for {self.email} (attempt {attempt + 1}): {e}"
                    )
                    self._reset_connection()
                    if attempt == max_retries - 1:
                        raise
                    time.sleep(1)  # Brief delay before retry

                except Exception as e:
                    logging.error(
                        f"Unexpected error executing IMAP command for {self.email}: {e}"
                    )
                    # Don't retry on authentication errors
                    if "Failed to establish authenticated connection" in str(e):
                        raise
                    # For other errors, try to reset connection and retry
                    self._reset_connection()
                    if attempt == max_retries - 1:
                        raise
                    time.sleep(1)  # Brief delay before retry

    def _reset_connection(self):
        """Reset connection state"""
        try:
            if self.connection:
                self.connection.logout()
        except:
            pass
        self.connection = None
        self.is_authenticated = False

    def logout(self):
        """Close the connection"""
        with self.lock:
            self._reset_connection()

    def is_idle(self, timeout_seconds: int = 300) -> bool:
        """Check if connection has been idle for too long"""
        return time.time() - self.last_used > timeout_seconds


class IMAPConnectionManager:
    """Manages IMAP connections for multiple accounts"""

    def __init__(self):
        self.connections: Dict[str, IMAPConnection] = {}
        self.lock = threading.Lock()
        self.cleanup_thread = None
        self.running = True
        self._start_cleanup_thread()

    def get_connection(
        self, account_data: Dict[str, Any], mail_settings: Dict[str, Any]
    ) -> IMAPConnection:
        """Get or create a connection for an account"""
        # Handle dbus.String objects by converting to regular strings
        email = str(account_data.get("email", "unknown"))

        with self.lock:
            if email not in self.connections:
                logging.debug(f"Creating new IMAP connection for {email}")
                self.connections[email] = IMAPConnection(account_data, mail_settings)
            else:
                connection = self.connections[email]
                # Check if connection is too old and needs refresh
                if connection.is_idle():
                    logging.debug(f"Connection for {email} is idle, refreshing")
                    connection.logout()
                    self.connections[email] = IMAPConnection(
                        account_data, mail_settings
                    )

            return self.connections[email]

    def close_connection(self, email: str):
        """Close a specific connection"""
        with self.lock:
            if email in self.connections:
                self.connections[email].logout()
                del self.connections[email]

    def close_all_connections(self):
        """Close all connections"""
        with self.lock:
            for connection in self.connections.values():
                connection.logout()
            self.connections.clear()

    def _start_cleanup_thread(self):
        """Start background thread to clean up idle connections"""

        def cleanup_loop():
            while self.running:
                try:
                    time.sleep(60)  # Check every minute
                    with self.lock:
                        emails_to_remove = []
                        for email, connection in self.connections.items():
                            if connection.is_idle(timeout_seconds=600):  # 10 minutes
                                logging.debug(
                                    f"Cleaning up idle connection for {email}"
                                )
                                connection.logout()
                                emails_to_remove.append(email)

                        for email in emails_to_remove:
                            del self.connections[email]

                except Exception as e:
                    logging.error(f"Error in connection cleanup thread: {e}")

        self.cleanup_thread = threading.Thread(target=cleanup_loop, daemon=True)
        self.cleanup_thread.start()

    def shutdown(self):
        """Shutdown the connection manager"""
        self.running = False
        self.close_all_connections()


# Global connection manager instance
_connection_manager = None


def get_connection_manager() -> IMAPConnectionManager:
    """Get the global connection manager instance"""
    global _connection_manager
    if _connection_manager is None:
        _connection_manager = IMAPConnectionManager()
    return _connection_manager


def shutdown_connection_manager():
    """Shutdown the global connection manager"""
    global _connection_manager
    if _connection_manager:
        _connection_manager.shutdown()
        _connection_manager = None
