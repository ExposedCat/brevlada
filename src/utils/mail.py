import dbus
import imaplib
import ssl
import threading
import logging
import time
from utils.toolkit import GLib


def get_oauth2_token(account_data):
    try:
        logging.debug(f"Attempting to get OAuth2 token for account: {account_data.get('email', 'unknown')}")
        bus = dbus.SessionBus()
        logging.debug("Connected to D-Bus session bus")
        account_obj = bus.get_object("org.gnome.OnlineAccounts", account_data["path"])
        logging.debug(f"Got account object from path: {account_data['path']}")
        oauth2_props = dbus.Interface(
            account_obj, "org.gnome.OnlineAccounts.OAuth2Based"
        )
        logging.debug("Created OAuth2 interface")
        access_token = oauth2_props.GetAccessToken()
        logging.debug(f"Retrieved access token, length: {len(access_token[0]) if access_token and access_token[0] else 0}")
        return access_token[0] if access_token else None
    except Exception as e:
        logging.error(f"Error getting OAuth2 token for {account_data.get('email', 'unknown')}: {e}")
        logging.debug(f"Full account data: {account_data}")
        return None


def get_mail_settings(account_data):
    try:
        logging.debug(f"Getting mail settings for account: {account_data.get('email', 'unknown')}")
        bus = dbus.SessionBus()
        account_obj = bus.get_object("org.gnome.OnlineAccounts", account_data["path"])
        mail_props = dbus.Interface(account_obj, "org.freedesktop.DBus.Properties")

        mail_properties = mail_props.GetAll("org.gnome.OnlineAccounts.Mail")
        logging.debug(f"Retrieved mail properties: {dict(mail_properties)}")

        settings = {
            "email": mail_properties.get("EmailAddress", ""),
            "imap_host": mail_properties.get("ImapHost", "imap.gmail.com"),
            "imap_port": mail_properties.get("ImapPort", 993),
            "imap_username": mail_properties.get("ImapUserName", ""),
            "imap_use_ssl": mail_properties.get("ImapUseSsl", True),
            "imap_use_tls": mail_properties.get("ImapUseTls", True),
            "imap_accept_ssl_errors": mail_properties.get("ImapAcceptSslErrors", False),
        }
        logging.debug(f"Processed mail settings: {settings}")
        return settings
    except Exception as e:
        logging.error(f"Error getting mail settings for {account_data.get('email', 'unknown')}: {e}")
        logging.debug(f"Account path: {account_data.get('path', 'unknown')}")
        return None


def authenticate_imap_oauth2(mail, username, token):
    logging.debug(f"Authenticating with OAuth2 for user: {username}")
    auth_string = f"user={username}\x01auth=Bearer {token}\x01\x01"
    logging.debug(f"Auth string length: {len(auth_string)}")

    def auth_callback(response):
        logging.debug(f"Auth callback called with response: {response}")
        return auth_string.encode("utf-8")

    try:
        logging.debug("Attempting XOAUTH2 authentication...")
        mail.authenticate("XOAUTH2", auth_callback)
        logging.debug("XOAUTH2 authentication successful")
    except Exception as e:
        logging.error(f"XOAUTH2 authentication failed: {e}")
        raise


def connect_to_imap_server(mail_settings):
    server = mail_settings.get("imap_host", "imap.gmail.com")
    port = mail_settings.get("imap_port", 993)
    use_ssl = mail_settings.get("imap_use_ssl", True)
    use_tls = mail_settings.get("imap_use_tls", True)

    logging.info(f"Connecting to IMAP server: {server}:{port}, SSL: {use_ssl}, TLS: {use_tls}")
    logging.debug(f"Full mail settings: {mail_settings}")

    try:
        if use_ssl:
            logging.debug("Creating SSL context...")
            context = ssl.create_default_context()
            context.check_hostname = False
            context.verify_mode = ssl.CERT_NONE
            logging.debug("Connecting with SSL...")
            mail = imaplib.IMAP4_SSL(server, port, ssl_context=context)
            logging.debug("SSL connection established")
        else:
            logging.debug("Connecting without SSL...")
            mail = imaplib.IMAP4(server, port)
            if use_tls:
                logging.debug("Starting TLS...")
                mail.starttls()
                logging.debug("TLS started")
            logging.debug("Plain connection established")

        logging.info(f"Successfully connected to IMAP server: {server}:{port}")
        return mail
    except Exception as e:
        logging.error(f"Failed to connect to IMAP server {server}:{port}: {e}")
        raise


# Connection cache: account_email -> (connection, last_used_time)
_connection_cache = {}
_connection_cache_lock = threading.Lock()

def logout_and_remove_from_cache(mail, account_data):
    """Logout and remove connection from cache"""
    email = str(account_data.get('email', 'unknown'))
    
    try:
        mail.logout()
    except Exception as e:
        logging.warning(f"Error during logout for {email}: {e}")
    
    # Remove from cache
    with _connection_cache_lock:
        if email in _connection_cache:
            del _connection_cache[email]
            logging.debug(f"Removed connection from cache for {email}")

def cleanup_all_connections():
    """Close all cached connections - call this on app shutdown"""
    with _connection_cache_lock:
        connection_count = len(_connection_cache)
        if connection_count > 0:
            logging.debug(f"Cache contents before cleanup: {list(_connection_cache.keys())}")
        
        for email, (connection, _) in _connection_cache.items():
            try:
                logging.debug(f"Closing cached connection for {email}")
                connection.logout()
            except Exception as e:
                logging.warning(f"Error closing cached connection for {email}: {e}")
        
        _connection_cache.clear()
        logging.info(f"Cleaned up {connection_count} cached connections")



def connect_and_authenticate(mail_settings, account_data):
    """Connect to IMAP server and authenticate in one step, with connection caching"""
    email = str(account_data.get('email', 'unknown'))
    
    with _connection_cache_lock:
        # Check if we have a cached connection
        if email in _connection_cache:
            cached_connection, last_used = _connection_cache[email]
            current_time = time.time()
            
            # Check if connection is still fresh (less than 5 minutes old)
            if current_time - last_used < 300:  # 5 minutes
                logging.debug(f"Reusing cached connection for {email}")
                _connection_cache[email] = (cached_connection, current_time)
                return cached_connection
            else:
                # Connection is too old, remove it
                logging.debug(f"Removing stale cached connection for {email}")
                try:
                    cached_connection.logout()
                except:
                    pass
                del _connection_cache[email]
    
    # No cached connection, create new one
    try:
        logging.debug(f"Creating new connection for {email}")
        mail = connect_to_imap_server(mail_settings)
        
        # Authenticate
        if not authenticate_imap(mail, account_data, mail_settings):
            # If authentication fails, close connection and return None
            try:
                mail.logout()
            except:
                pass
            return None
        
        # Cache the successful connection
        with _connection_cache_lock:
            _connection_cache[email] = (mail, time.time())
            logging.debug(f"Cached new connection for {email}")
            
        return mail
        
    except Exception as e:
        logging.error(f"Failed to connect and authenticate: {e}")
        return None


def authenticate_imap(mail, account_data, mail_settings):
    email = account_data["email"]
    username = mail_settings.get("imap_username", email)
    auth_xoauth2 = account_data.get("has_oauth2", False)

    logging.debug(f"Authenticating IMAP for email: {email}, username: {username}")
    logging.debug(f"OAuth2 available: {auth_xoauth2}")

    if not auth_xoauth2:
        logging.warning(f"Account {email} does not support OAuth2")
        return False

    logging.info(f"Account {email} supports OAuth2, attempting authentication")
    token = get_oauth2_token(account_data)
    if not token:
        logging.error(f"Failed to get OAuth2 token for {email}")
        return False

    try:
        authenticate_imap_oauth2(mail, username, token)
        logging.info(f"OAuth2 authentication successful for {email}")
        return True
    except Exception as e:
        logging.error(f"OAuth2 authentication failed for {email}: {e}")
        logging.debug(f"Authentication exception details: {type(e).__name__}: {str(e)}")
        return False


def parse_folder_line(folder_line):
    logging.debug(f"Parsing folder line: {folder_line} (type: {type(folder_line)})")
    logging.debug(f"Raw folder line repr: {repr(folder_line)}")

    if isinstance(folder_line, bytes):
        folder_line = folder_line.decode("utf-8")
        logging.debug(f"Decoded bytes to: {folder_line}")
    elif isinstance(folder_line, tuple):
        logging.debug(f"Tuple content: {folder_line}")
        folder_line = folder_line[0].decode("utf-8") if folder_line[0] else ""
        logging.debug(f"Extracted from tuple: {folder_line}")
    else:
        folder_line = str(folder_line)
        logging.debug(f"Converted to string: {folder_line}")

    logging.debug(f"Processing folder line: {repr(folder_line)}")

    if '"' in folder_line:
        parts = folder_line.split('"')
        logging.debug(f"Split by quotes: {parts}")
        if len(parts) >= 3:
            result = parts[-2]
            logging.debug(f"Extracted quoted folder name: {repr(result)}")
            return result
    else:
        parts = folder_line.split()
        logging.debug(f"Split by spaces: {parts}")
        if len(parts) >= 3:
            result = " ".join(parts[2:])
            logging.debug(f"Extracted unquoted folder name: {repr(result)}")
            return result

    logging.debug("Could not parse folder name")
    return None


def get_folders_from_imap(mail, email):
    try:
        logging.debug(f"Listing folders for {email}")
        status, folder_list = mail.list()
        logging.debug(f"IMAP LIST command returned: status={status}, count={len(folder_list) if folder_list else 0}")

        if status != "OK":
            logging.error(f"IMAP LIST command failed with status: {status}")
            return ["Error: Failed to list folders"]

        logging.debug(f"Raw folder list: {folder_list}")
        logging.debug(f"Raw folder list repr: {[repr(f) for f in folder_list]}")
        folders = []
        for i, folder_line in enumerate(folder_list):
            logging.debug(f"Processing folder line {i}: {folder_line}")
            logging.debug(f"Folder line {i} repr: {repr(folder_line)}")
            if folder_line:
                folder_name = parse_folder_line(folder_line)
                if folder_name:
                    folders.append(folder_name)
                    logging.debug(f"Added folder: {repr(folder_name)}")
                else:
                    logging.debug(f"Skipped unparseable folder line: {repr(folder_line)}")

        if folders:
            folders.sort()
            logging.info(f"Found {len(folders)} folders for {email}: {folders}")
        else:
            folders = ["Error: No folders found"]
            logging.warning(f"No folders found for {email}")

        return folders

    except Exception as e:
        logging.error(f"Error fetching folders for {email}: {e}")
        logging.debug(f"Exception details: {type(e).__name__}: {str(e)}")
        if (
            "Authentication" in str(e)
            or "login" in str(e).lower()
            or "AUTHENTICATE" in str(e)
        ):
            return ["Error: Authentication failed - check account credentials"]
        elif "timeout" in str(e).lower() or "connection" in str(e).lower():
            return ["Error: Connection failed - check network"]
        elif "SSL" in str(e) or "certificate" in str(e).lower():
            return ["Error: SSL/TLS connection failed"]
        else:
            return ["Error: IMAP connection failed"]


def fetch_imap_folders(account_data, callback):
    def fetch_folders():
        try:
            email = account_data["email"]
            logging.info(f"Starting to fetch folders for account: {email}")
            logging.debug(f"Account data: {account_data}")

            mail_settings = get_mail_settings(account_data)
            if not mail_settings:
                error_msg = "Error: Could not get mail settings"
                logging.error(f"No mail settings for account: {account_data['path']}")
                GLib.idle_add(callback, [error_msg])
                return

            logging.debug(f"Connecting and authenticating to IMAP server for {email}")
            mail = connect_and_authenticate(mail_settings, account_data)
            
            if not mail:
                error_msg = "Error: Authentication failed - OAuth2 required"
                logging.error(f"Authentication failed for {email}")
                GLib.idle_add(callback, [error_msg])
                return

            logging.debug(f"Getting folders from IMAP for {email}")
            folders = get_folders_from_imap(mail, email)

            logging.debug(f"Operation completed for {email}, connection remains in cache")

            logging.info(f"Successfully fetched {len(folders)} folders for {email}")
            GLib.idle_add(callback, folders)

        except Exception as e:
            logging.error(f"Failed to fetch folders for {account_data.get('email', 'unknown')}: {e}")
            logging.debug(f"Exception details: {type(e).__name__}: {str(e)}")
            error_msg = "Error: Failed to connect to mail server"
            GLib.idle_add(callback, [error_msg])

    logging.debug(f"Starting thread to fetch folders for {account_data.get('email', 'unknown')}")
    thread = threading.Thread(target=fetch_folders)
    thread.daemon = True
    thread.start()


def fetch_messages_from_folder(account_data, folder_name, callback, limit=50):
    """Fetch messages from specified folder"""
    logging.debug(f"Starting to fetch messages from folder {folder_name} for account {account_data.get('email', 'unknown')}")

    def fetch_messages():
        try:
            email = account_data["email"]
            logging.info(f"Fetching messages from folder '{folder_name}' for account: {email}")

            mail_settings = get_mail_settings(account_data)
            if not mail_settings:
                error_msg = "Error: Could not get mail settings"
                logging.error(f"No mail settings for account: {email}")
                GLib.idle_add(callback, error_msg, None)
                return

            logging.debug(f"Connecting and authenticating to IMAP server for message fetch: {email}")
            mail = connect_and_authenticate(mail_settings, account_data)
            
            if not mail:
                error_msg = "Error: Authentication failed - OAuth2 required"
                logging.error(f"Authentication failed for message fetch: {email}")
                GLib.idle_add(callback, error_msg, None)
                return

            # Select folder
            logging.debug(f"Selecting folder '{folder_name}' for {email}")
            logging.debug(f"Folder name bytes: {folder_name.encode('utf-8')}")
            logging.debug(f"Folder name repr: {repr(folder_name)}")
            try:
                # Properly quote folder names for IMAP, especially Gmail folders
                if ' ' in folder_name or '[' in folder_name or ']' in folder_name or '/' in folder_name:
                    # Quote folder names with special characters
                    quoted_folder = f'"{folder_name}"'
                    logging.debug(f"Calling mail.select() with quoted folder: {quoted_folder}")
                    status, data = mail.select(quoted_folder)
                else:
                    logging.debug(f"Calling mail.select() with folder: {folder_name}")
                    status, data = mail.select(folder_name)
                logging.debug(f"SELECT response: status={status}, data={data}")
                if status != 'OK':
                    error_msg = f"Error: Could not select folder '{folder_name}'"
                    logging.error(f"Could not select folder '{folder_name}': status={status}, data={data}")
                    logout_and_remove_from_cache(mail, account_data)
                    GLib.idle_add(callback, error_msg, None)
                    return

                total_messages = int(data[0]) if data and data[0] else 0
                logging.debug(f"Folder '{folder_name}' contains {total_messages} messages")

                if total_messages == 0:
                    logging.debug(f"No messages in folder '{folder_name}'")
                    GLib.idle_add(callback, None, [])
                    logout_and_remove_from_cache(mail, account_data)
                    return

                # Fetch recent messages
                start_msg = max(1, total_messages - limit + 1)
                msg_range = f"{start_msg}:{total_messages}"
                logging.debug(f"Fetching messages {msg_range} from folder '{folder_name}'")

                # Fetch basic headers for display
                status, data = mail.fetch(msg_range, '(ENVELOPE FLAGS UID BODY.PEEK[HEADER.FIELDS (DATE FROM TO CC SUBJECT MESSAGE-ID IN-REPLY-TO REFERENCES)])')
                if status != 'OK':
                    error_msg = "Error: Could not fetch message headers"
                    logging.error(f"Could not fetch message headers: {data}")
                    logout_and_remove_from_cache(mail, account_data)
                    GLib.idle_add(callback, error_msg, None)
                    return

                logging.debug(f"Parsing {len(data)} message responses")
                messages = parse_fetched_messages(data, email, folder_name)
                messages.reverse()  # Show newest first

                logging.info(f"Successfully fetched {len(messages)} messages from folder '{folder_name}'")
                GLib.idle_add(callback, None, messages)

            except Exception as e:
                logging.error(f"Error fetching messages from folder '{folder_name}': {e}")
                error_msg = f"Error: Could not access folder '{folder_name}'"
                GLib.idle_add(callback, error_msg, None)
            finally:
                logging.debug(f"Operation completed for {email}, connection remains in cache")

        except Exception as e:
            logging.error(f"Failed to fetch messages for {account_data.get('email', 'unknown')}: {e}")
            error_msg = "Error: Failed to connect to mail server"
            GLib.idle_add(callback, error_msg, None)

    logging.debug(f"Starting thread to fetch messages for {account_data.get('email', 'unknown')}")
    thread = threading.Thread(target=fetch_messages)
    thread.daemon = True
    thread.start()


def parse_fetched_messages(fetch_data, account_email, folder_name):
    """Parse fetched message data into Message objects"""
    import email

    logging.debug(f"Parsing fetched messages for folder '{folder_name}'")
    messages = []
    current_uid = None
    current_flags = []

    for item in fetch_data:
        if isinstance(item, tuple) and len(item) >= 2:
            # Parse message info
            msg_info = item[0].decode('utf-8') if isinstance(item[0], bytes) else str(item[0])
            msg_data = item[1]

            logging.debug(f"Processing message item: {msg_info}")

            # Extract UID and FLAGS
            if 'UID' in msg_info:
                uid_start = msg_info.find('UID ') + 4
                uid_end = msg_info.find(' ', uid_start)
                if uid_end == -1:
                    uid_end = msg_info.find(')', uid_start)
                current_uid = int(msg_info[uid_start:uid_end])
                logging.debug(f"Found UID: {current_uid}")

            if 'FLAGS' in msg_info:
                flags_start = msg_info.find('FLAGS (') + 7
                flags_end = msg_info.find(')', flags_start)
                flags_str = msg_info[flags_start:flags_end]
                current_flags = [flag.strip() for flag in flags_str.split()]
                logging.debug(f"Found FLAGS: {current_flags}")

            # Parse headers
            if msg_data:
                try:
                    header_text = msg_data.decode('utf-8') if isinstance(msg_data, bytes) else msg_data
                    msg_obj = email.message_from_string(header_text)

                    # Extract basic message info
                    subject = decode_header_value(msg_obj.get('Subject', ''))
                    from_header = decode_header_value(msg_obj.get('From', ''))
                    to_header = decode_header_value(msg_obj.get('To', ''))
                    date_header = msg_obj.get('Date', '')
                    message_id = msg_obj.get('Message-ID', '')

                    # Parse sender info
                    sender_name = ""
                    sender_email = ""
                    if from_header:
                        if '<' in from_header and '>' in from_header:
                            sender_name = from_header.split('<')[0].strip().strip('"')
                            sender_email = from_header.split('<')[1].split('>')[0].strip()
                        else:
                            sender_email = from_header.strip()

                    # Create message dict
                    message = {
                        'uid': current_uid,
                        'folder': folder_name,
                        'account_id': account_email,
                        'message_id': message_id,
                        'subject': subject,
                        'sender': {
                            'name': sender_name,
                            'email': sender_email
                        },
                        'recipients': [to_header] if to_header else [],
                        'cc': [],
                        'bcc': [],
                        'reply_to': [],
                        'date': date_header,
                        'flags': current_flags,
                        'is_read': '\\Seen' in current_flags,
                        'is_flagged': '\\Flagged' in current_flags,
                        'is_deleted': '\\Deleted' in current_flags,
                        'is_draft': '\\Draft' in current_flags,
                        'is_answered': '\\Answered' in current_flags,
                        'has_attachments': False,
                        'body': '',
                        'body_html': '',
                        'headers': dict(msg_obj.items()),
                        'envelope': {},
                        'bodystructure': {},
                        'thread_subject': subject,
                        'thread_references': [],
                        'in_reply_to': msg_obj.get('In-Reply-To', ''),
                        'references': msg_obj.get('References', ''),
                        'attachments': []
                    }

                    messages.append(message)
                    logging.debug(f"Created message object for UID {current_uid}: {subject}")

                except Exception as e:
                    logging.error(f"Error parsing message UID {current_uid}: {e}")
                    continue

    logging.debug(f"Successfully parsed {len(messages)} messages")
    return messages


def decode_header_value(header_value):
    """Decode email header value"""
    if not header_value:
        return ""

    try:
        from email.header import decode_header
        decoded_fragments = decode_header(header_value)
        decoded_header = ""
        for fragment, encoding in decoded_fragments:
            if isinstance(fragment, bytes):
                if encoding:
                    decoded_header += fragment.decode(encoding)
                else:
                    decoded_header += fragment.decode('utf-8', errors='replace')
            else:
                decoded_header += fragment
        return decoded_header
    except Exception as e:
        logging.debug(f"Error decoding header '{header_value}': {e}")
        return header_value
