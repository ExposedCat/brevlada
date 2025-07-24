from utils.toolkit import Gtk, Adw, GLib
from components.ui import AppIcon, AppText
from components.button import AppButton
from components.container import ContentContainer, ScrollContainer
from components.html_viewer import HtmlViewer

import logging
import threading
from utils.mail import fetch_message_body_from_imap
from theme import THEME_MARGIN_MEDIUM, THEME_MARGIN_LARGE

class MessageViewer:
    def __init__(self, storage, imap_backend):
        self.storage = storage
        self.imap_backend = imap_backend
        self.current_message = None
        self.current_account_data = None
        self.body_fetch_id = 0
        self.current_state = None
        self.content_header = None
        self.is_showing_thread = False
        self.expanded_messages = {}  
        
        self.widget = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        self.widget.add_css_class("message-viewer-root")
        
        self.message_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.message_container.set_vexpand(False)
        self.message_container.set_hexpand(True)
        self.message_container.add_css_class("message-container")
        
        self.scroll_window = Gtk.ScrolledWindow()
        self.scroll_window.set_vexpand(True)
        self.scroll_window.set_hexpand(True)
        self.scroll_window.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.scroll_window.set_child(self.message_container)
        
        self.widget.append(self.scroll_window)
        
        self.loading_state = self.create_loading_state()
        self.error_state = self.create_error_state()
        
        self.show_empty_state()
    
    def create_loading_state(self):
        loading_spinner = Gtk.Spinner()
        loading_spinner.set_size_request(32, 32)
        loading_spinner.start()
        
        loading_label = AppText(
            text="Loading message content...",
            class_names="message-viewer-loading-text",
            halign=Gtk.Align.CENTER,
        )
        
        return ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER,
            valign=Gtk.Align.CENTER,
            class_names="message-viewer-loading-state",
            children=[loading_spinner, loading_label.widget],
        )
    
    def create_error_state(self):
        error_icon = AppIcon("dialog-error-symbolic")
        error_icon.set_pixel_size(48)
        error_icon.set_opacity(0.8)
        
        error_label = AppText(
            text="Failed to load message content",
            class_names="message-viewer-error-text",
            halign=Gtk.Align.CENTER,
        )
        error_label.set_markup(
            "<span size='large' foreground='red'>Failed to load message content</span>"
        )
        
        return ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER,
            valign=Gtk.Align.CENTER,
            class_names="message-viewer-error-state",
            children=[error_icon.widget, error_label.widget],
        )
    
    def set_account_data(self, account_data):
        self.current_account_data = account_data
    
    def show_message(self, message_or_thread):
        
        
        while self.message_container.get_first_child():
            self.message_container.remove(self.message_container.get_first_child())
        if not message_or_thread:
            self.show_select_message_state()
            return
        
        self.current_message = message_or_thread
        self.show_loading_state()
        
        
        if hasattr(message_or_thread, 'messages') and hasattr(message_or_thread, 'get_unread_count'):
            logging.info(f"MessageViewer: Received thread with {len(message_or_thread.messages)} messages")
        else:
            logging.info(f"MessageViewer: Received single message: {message_or_thread.get('subject', 'No subject')}")
        
        
        if hasattr(message_or_thread, 'messages') and hasattr(message_or_thread, 'get_unread_count'):
            
            self.display_thread(message_or_thread)
        else:
            
            if self.message_has_body(message_or_thread):
                self.display_message(message_or_thread)
            else:
                self.fetch_message_body(message_or_thread)

    def message_has_body(self, message):
        """Check if message has a non-empty body"""
        body_text = message.get("body", "")
        body_html = message.get("body_html", "")
        return (body_text and body_text.strip()) or (body_html and body_html.strip())

    def fetch_message_body(self, message):
        if not self.current_account_data:
            logging.error("MessageViewer: No account data set in MessageViewer")
            self.show_error_state("No account data set")
            return
        
        
        self.fetch_message_body_smart(message)
    
    def display_thread(self, thread):
        logging.info(f"MessageViewer: Displaying thread with {len(thread.messages)} messages")
        while self.message_container.get_first_child():
            self.message_container.remove(self.message_container.get_first_child())
        self.current_state = "content"
        self.is_showing_thread = len(thread.messages) > 1
        self.current_thread_total = len(thread.messages)  
        if self.content_header:
            self.content_header.set_message_subject(thread.get_display_subject())
        self.thread_message_cards = {}
        self.expanded_messages.clear()  
        
        for i, message in enumerate(thread.messages):
            logging.debug(f"MessageViewer: Creating card for message {i+1}: UID={message.get('uid')}, has_body={self.message_has_body(message)}")
            
            message_uid = message.get("uid")
            
            is_unread = not message.get("is_read", True)  
            if is_unread and message_uid:
                self.expanded_messages[message_uid] = True
                logging.debug(f"MessageViewer: Auto-expanding unread message UID {message_uid}")
            message_row = self.create_simple_message_row(message, is_in_thread=True, thread_total=len(thread.messages))
            self.message_container.append(message_row)
            message_uid = message.get("uid")
            if message_uid:
                self.thread_message_cards[message_uid] = message_row
            if not self.message_has_body(message):
                logging.debug(f"MessageViewer: Message UID {message_uid} needs body, trying database first")
                self.fetch_message_body_smart(message, thread)
            else:
                logging.debug(f"MessageViewer: Message UID {message_uid} already has body")
        logging.info(f"MessageViewer: Thread display complete, {len(thread.messages)} cards created")

    def create_simple_message_row(self, message, is_in_thread=False, thread_total=None):
        message_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        message_container.add_css_class("message-row-widget")
        
        
        header_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        header_container.set_spacing(6)
        
        
        expand_button = None
        is_actual_thread = is_in_thread and thread_total and thread_total > 1
        if is_actual_thread:
            expand_button = AppButton(
                variant="expand",
                class_names=["expand-button", "message-expand"],
                h_fill=False,
                w_fill=False
            )
            expand_button.widget.set_size_request(32, 32)  
            expand_button.widget.set_margin_start(12)  
            expand_button.widget.set_margin_end(0)  
            expand_button.widget.set_margin_top(6)  
            expand_button.widget.set_margin_bottom(6)  
            expand_button.widget.set_valign(Gtk.Align.CENTER)  
            message_uid = message.get("uid")
            is_expanded = self.expanded_messages.get(message_uid, False)
            expand_button.set_icon_name("pan-down-symbolic" if is_expanded else "pan-end-symbolic")
            expand_button.connect("clicked", self.on_message_expand_clicked, message, message_container, expand_button)
            header_container.append(expand_button.widget)
        
        header_row = Adw.ActionRow()
        header_row.set_hexpand(True)  
        
        sender_info = message.get("sender", {})
        sender_name = sender_info.get("name", "")
        sender_email = sender_info.get("email", "")
        
        display_name = sender_name if sender_name else sender_email
        header_row.set_title(display_name)
        
        if sender_name and sender_email:
            header_row.set_subtitle(sender_email)
        
        date_str = message.get("date", "")
        if date_str:
            short_date = self.get_shortened_date(date_str)
            if short_date:
                date_label = Gtk.Label(label=short_date)
                date_label.add_css_class("message-date-suffix")
                date_label.set_halign(Gtk.Align.END)
                date_label.set_opacity(0.7)
                header_row.add_suffix(date_label)
        
        initials = self.get_initials(sender_name, sender_email)
        avatar = Adw.Avatar.new(32, initials, True)
        avatar.add_css_class("message-avatar")
        header_row.add_prefix(avatar)
        
        header_container.append(header_row)
        message_container.append(header_container)
        
        
        
        if not is_actual_thread:
            
            loading = not self.message_has_body(message)
            body_content = self.create_message_body_content(message, loading=loading)
            message_container.append(body_content)
        else:
            
            message_uid = message.get("uid")
            is_expanded = self.expanded_messages.get(message_uid, False)
            if is_expanded:
                loading = not self.message_has_body(message)
                body_content = self.create_message_body_content(message, loading=loading)
                message_container.append(body_content)
        
        return message_container
    
    def on_message_expand_clicked(self, button, message, message_container, expand_button):
        """Handle expand button click for threaded messages"""
        message_uid = message.get("uid")
        if not message_uid:
            return
            
        
        is_expanded = self.expanded_messages.get(message_uid, False)
        self.expanded_messages[message_uid] = not is_expanded
        
        
        if is_expanded:
            
            expand_button.set_icon_name("pan-end-symbolic")
            
            if message_container.get_last_child() != message_container.get_first_child():
                body_content = message_container.get_last_child()
                message_container.remove(body_content)
        else:
            
            expand_button.set_icon_name("pan-down-symbolic")
            
            if not self.message_has_body(message):
                logging.debug(f"MessageViewer: Message UID {message_uid} expanding but no body, fetching from IMAP")
                
                loading_content = self.create_message_body_content(message, loading=True)
                message_container.append(loading_content)
                
                self.fetch_message_body_smart(message, thread=True)
            else:
                loading = False
                body_content = self.create_message_body_content(message, loading=loading)
                message_container.append(body_content)

    def fetch_message_body_for_thread(self, message, thread):
        """Fetch message body for a message in a thread"""
        message_uid = message.get("uid")
        logging.debug(f"MessageViewer: STARTING IMAP FETCH for thread message UID {message_uid}")
        
        if not self.current_account_data:
            logging.warning("MessageViewer: No account data available for body fetch")
            return

        if not message_uid:
            logging.warning("MessageViewer: No UID found for message, skipping body fetch")
            return
            
        
        fetch_key = f"_fetching_body_{message_uid}"
        if hasattr(self, fetch_key) and getattr(self, fetch_key):
            logging.debug(f"MessageViewer: Already fetching body for UID {message_uid}, skipping")
            return
            
        logging.info(f"MessageViewer: Starting body fetch for message UID {message_uid}")
        setattr(self, fetch_key, True)
        
        
        message_fetch_id = f"fetch_{message_uid}_{self.body_fetch_id}"
        self.body_fetch_id += 1
        
        def on_body_fetched(error, message_body_data):
            logging.debug(f"MessageViewer: Body fetch callback for UID {message_uid}")
            
            
            setattr(self, fetch_key, False)
            
            if error:
                logging.error(f"MessageViewer: Error fetching thread message body for UID {message_uid}: {error}")
                return
            
            if message_body_data:
                logging.info(f"MessageViewer: Successfully fetched body for UID {message_uid}")
                
                if isinstance(message_body_data, dict):
                    message["body"] = message_body_data.get("text", "")
                    message["body_html"] = message_body_data.get("html", "")
                    body_text_len = len(message["body"]) if message["body"] else 0
                    body_html_len = len(message["body_html"]) if message["body_html"] else 0
                    logging.debug(f"MessageViewer: UID {message_uid} - text_length={body_text_len}, html_length={body_html_len}")
                else:
                    message["body"] = message_body_data
                    message["body_html"] = ""
                    body_text_len = len(message["body"]) if message["body"] else 0
                    logging.debug(f"MessageViewer: UID {message_uid} - body_length={body_text_len}")
                
                try:
                    self.storage.update_message_body(
                        message["uid"], 
                        message["folder"], 
                        message["account_id"], 
                        message["body"],
                        message.get("body_html", "")
                    )
                    logging.debug(f"MessageViewer: Stored body for UID {message_uid} in database")
                except Exception as e:
                    logging.error(f"MessageViewer: Error storing thread message body for UID {message_uid}: {e}")
                
                self.update_message_card_body(message_uid, message)
                logging.info(f"MessageViewer: Updated message card for UID {message_uid}")
            else:
                logging.warning(f"MessageViewer: No body data returned for UID {message_uid}")
        
        fetch_message_body_from_imap(
            self.current_account_data, 
            message["folder"], 
            message["uid"], 
            on_body_fetched
        )

    def fetch_message_body_smart(self, message, thread=None):
        """Smart body fetching: try database first, then IMAP if needed"""
        message_uid = message.get("uid")
        if not message_uid:
            logging.warning("MessageViewer: No UID found for message, skipping body fetch")
            return
        
        self.fetch_message_body_from_database(message, thread)

    def fetch_message_body_from_database(self, message, thread=None):
        """Try to fetch message body from database"""
        message_uid = message.get("uid")
        folder = message.get("folder")
        account_id = message.get("account_id")
        
        logging.debug(f"MessageViewer: Database fetch attempt - UID={message_uid}, folder={folder}, account_id={account_id}")
        
        if not all([message_uid, folder, account_id]):
            logging.warning(f"MessageViewer: Missing required fields for database fetch: UID={message_uid}, folder={folder}, account_id={account_id}")
            logging.debug(f"MessageViewer: IMAP FETCH REASON: Missing required database fields")
            
            if thread:
                self.fetch_message_body_for_thread(message, thread)
            else:
                self.fetch_message_body_from_imap_direct(message)
            return
        
        try:
            
            db_message = self.storage.get_message_by_uid(message_uid, folder, account_id)
            
            logging.debug(f"MessageViewer: Database lookup result for UID {message_uid}: found={db_message is not None}")
            if db_message:
                body_text = db_message.get("body", "")
                body_html = db_message.get("body_html", "")
                logging.debug(f"MessageViewer: Database body content - text_length={len(body_text) if body_text else 0}, html_length={len(body_html) if body_html else 0}")
                has_body = bool((body_text and body_text.strip()) or (body_html and body_html.strip()))
                logging.debug(f"MessageViewer: Database message has_body={has_body}")
            
            if db_message and self.message_has_body(db_message):
                logging.info(f"MessageViewer: Found body in database for UID {message_uid}")
                
                message["body"] = db_message.get("body", "")
                message["body_html"] = db_message.get("body_html", "")
                
                
                if thread and message_uid in getattr(self, 'thread_message_cards', {}):
                    self.update_message_card_body(message_uid, message)
                elif not thread:
                    
                    self.display_message(message)
            else:
                if not db_message:
                    logging.debug(f"MessageViewer: IMAP FETCH REASON: Message UID {message_uid} not found in database")
                else:
                    logging.debug(f"MessageViewer: IMAP FETCH REASON: Message UID {message_uid} found in database but has empty body (body_length={len(db_message.get('body', ''))}, html_length={len(db_message.get('body_html', ''))})")
                
                logging.debug(f"MessageViewer: No body found in database for UID {message_uid}, fetching from IMAP")
                
                if thread:
                    self.fetch_message_body_for_thread(message, thread)
                else:
                    self.fetch_message_body_from_imap_direct(message)
                    
        except Exception as e:
            logging.error(f"MessageViewer: Error fetching from database for UID {message_uid}: {e}")
            logging.debug(f"MessageViewer: IMAP FETCH REASON: Database error - {type(e).__name__}: {str(e)}")
            
            if thread:
                self.fetch_message_body_for_thread(message, thread)
            else:
                self.fetch_message_body_from_imap_direct(message)

    def fetch_message_body_from_imap_direct(self, message):
        """Fetch message body directly from IMAP (for single messages)"""
        message_uid = message.get("uid")
        logging.debug(f"MessageViewer: STARTING IMAP FETCH for single message UID {message_uid}")
        
        if not self.current_account_data:
            self.show_error_state("No account data set")
            return
        
        self.body_fetch_id += 1
        fetch_id = self.body_fetch_id
        
        logging.info(f"MessageViewer: Fetching body from IMAP for message UID {message.get('uid')}")
        
        def on_body_fetched(error, message_body_data):
            if fetch_id != self.body_fetch_id:
                logging.debug(f"MessageViewer: Body fetch operation {fetch_id} was cancelled, ignoring response")
                return
            
            if error:
                logging.error(f"MessageViewer: Error fetching message body: {error}")
                self.show_error_state(str(error))
                return
            
            if message_body_data:
                if isinstance(message_body_data, dict):
                    message["body"] = message_body_data.get("text", "")
                    message["body_html"] = message_body_data.get("html", "")
                else:
                    message["body"] = message_body_data
                    message["body_html"] = ""
                
                try:
                    self.storage.update_message_body(
                        message["uid"], 
                        message["folder"], 
                        message["account_id"], 
                        message["body"],
                        message.get("body_html", "")
                    )
                    logging.debug(f"MessageViewer: Stored message body in database")
                except Exception as e:
                    logging.error(f"MessageViewer: Error storing message body: {e}")
                
                self.display_message(message)
            else:
                logging.error("MessageViewer: No message body returned from IMAP fetch")
                self.show_error_state("No message body returned from IMAP fetch")
        
        fetch_message_body_from_imap(
            self.current_account_data, 
            message["folder"], 
            message["uid"], 
            on_body_fetched
        )

    def display_message(self, message):
        while self.message_container.get_first_child():
            self.message_container.remove(self.message_container.get_first_child())
        self.hide_all_states()
        self.current_state = "content"
        self.is_showing_thread = False
        self.current_thread_total = 1  
        if self.content_header:
            self.content_header.set_message_subject(message.get("subject", "(No Subject)"))
        message_row = self.create_simple_message_row(message, is_in_thread=False, thread_total=1)
        self.message_container.append(message_row)

       
    def create_message_body_content(self, message, loading=False):
        """Create the expandable body content for the expander row"""
        body_text = message.get("body", "")
        body_html = message.get("body_html", "")
        
        if body_html or body_text:
            
            html_viewer = HtmlViewer()
            if body_html:
                html_viewer.load_html(body_html)
            elif body_text:
                html_viewer.load_plain_text(body_text)
            return html_viewer.widget
        elif loading:
            
            loading_row = Adw.ActionRow()
            loading_row.set_title("Loading...")
            
            
            loading_spinner = Gtk.Spinner()
            loading_spinner.set_size_request(16, 16)
            loading_spinner.start()
            loading_row.add_suffix(loading_spinner)
            
            return loading_row
        else:
            
            empty_row = Adw.ActionRow()
            empty_row.set_title("(No message body)")
            empty_row.add_css_class("dim-label")
            return empty_row

    
    def get_initials(self, name, email):
        """Get initials for avatar (max 2 characters)"""
        if name:
            
            words = name.split()
            if len(words) >= 2:
                return f"{words[0][0]}{words[1][0]}".upper()
            elif len(words) == 1:
                return words[0][:2].upper()
        
        if email:
            
            return email[:2].upper()
        
        return "??"
    
    def get_shortened_date(self, date_str):
        """Get shortened date for display with time"""
        try:
            from datetime import datetime
            import email.utils
            
            parsed_date = email.utils.parsedate_tz(date_str)
            if parsed_date:
                timestamp = email.utils.mktime_tz(parsed_date)
                date_obj = datetime.fromtimestamp(timestamp)
                
                now = datetime.now()
                diff = now - date_obj
                
                time_str = date_obj.strftime("%H:%M")
                
                if diff.days == 0:
                    
                    return f"Today {time_str}"
                elif diff.days == 1:
                    
                    return f"Yesterday {time_str}"
                elif diff.days < 7:
                    
                    return f"{date_obj.strftime('%a')} {time_str}"
                elif diff.days < 365:
                    
                    return f"{date_obj.strftime('%b %d')} {time_str}"
                else:
                    
                    return f"{date_obj.strftime('%m/%d/%y')} {time_str}"
        except:
            pass
        
        return ""
      
    def show_select_message_state(self):
        self.hide_all_states()
        while self.message_container.get_first_child():
            self.message_container.remove(self.message_container.get_first_child())
        self.is_showing_thread = False
        if self.content_header:
            self.content_header.set_message_subject(None)
        
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-empty-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)  
        state_container.set_hexpand(True)
        
        icon = AppIcon("mail-unread-symbolic", class_names="message-viewer-empty-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)
        
        select_label = AppText(
            text="Select message to view",
            class_names="message-viewer-empty-text",
            halign=Gtk.Align.CENTER,
        )
        select_label.set_opacity(0.7)
        
        state_container.append(icon.widget)
        state_container.append(select_label.widget)
        
        self.message_container.append(state_container)
        self.current_state = "empty"

    def show_loading_state(self):
        self.hide_all_states()
        
        
        
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-loading-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        
        
        
        
        spinner = Gtk.Spinner()
        spinner.add_css_class("message-viewer-loading-spinner")
        spinner.set_size_request(32, 32)
        spinner.start()
        
        loading_label = AppText(
            text="Loading...",
            class_names="message-viewer-loading-text",
            margin_top=THEME_MARGIN_LARGE,
            halign=Gtk.Align.CENTER,
        )
        loading_label.set_opacity(0.7)
        
        state_container.append(spinner)
        state_container.append(loading_label.widget)
        
        self.message_container.append(state_container) 
        self.current_state = "loading"

    def show_empty_state(self):
        self.show_select_message_state()
    
    def show_error_state(self, error_message=None, raw_body=None):
        self.hide_all_states()
        
        
        
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-error-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        
        
        
        
        icon = AppIcon("dialog-error-symbolic", class_names="message-viewer-error-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)
        
        if error_message:
            logging.error(f"MessageViewer: Showing error state: {error_message}")
            if raw_body is not None:
                logging.error(f"MessageViewer: Raw message body (truncated): {raw_body[:1000]!r}")
            error_label = AppText(
                text=f"{error_message}",
                class_names="message-viewer-error-text",
                halign=Gtk.Align.CENTER,
            )
            error_label.set_opacity(0.7)
        else:
            error_label = AppText(
                text="Failed to load message content.",
                class_names="message-viewer-error-text",
                halign=Gtk.Align.CENTER,
            )
            error_label.set_opacity(0.7)
        
        state_container.append(icon.widget)
        state_container.append(error_label.widget)
        
        self.message_container.append(state_container) 
        self.current_state = "error"
    
    def hide_all_states(self):
        if self.current_state == "loading":
            self.message_container.remove(self.loading_state.widget) 
        elif self.current_state == "error":
            self.message_container.remove(self.error_state.widget) 
        self.current_state = None 

    def set_content_header(self, content_header):
        """Set the content header reference for updating title"""
        self.content_header = content_header 

    def update_message_card_body(self, message_uid, message):
        """Update the body content of a specific message card"""
        if not hasattr(self, 'thread_message_cards') or message_uid not in self.thread_message_cards:
            logging.warning(f"MessageViewer: No message card found for UID {message_uid} to update")
            return
            
        
        is_expanded = self.expanded_messages.get(message_uid, False)
        
        
        if self.is_showing_thread and not is_expanded:
            logging.debug(f"MessageViewer: Message UID {message_uid} is collapsed, body loaded but UI not updated")
            return
            
        old_card = self.thread_message_cards[message_uid]
        
        
        previous_sibling = None
        child = self.message_container.get_first_child()
        while child and child != old_card:
            previous_sibling = child
            child = child.get_next_sibling()
        
        
        self.message_container.remove(old_card)
        
        
        new_card = self.create_simple_message_row(message, is_in_thread=self.is_showing_thread, thread_total=self.current_thread_total)
        
        
        if previous_sibling:
            self.message_container.insert_child_after(new_card, previous_sibling)
        else:
            
            self.message_container.prepend(new_card)
        
        
        self.thread_message_cards[message_uid] = new_card
        
        logging.info(f"MessageViewer: Successfully updated message card body for UID {message_uid}") 