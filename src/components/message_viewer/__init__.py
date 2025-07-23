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
        
        self.widget = Adw.PreferencesGroup()
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        self.widget.add_css_class("message-viewer-root")
        
        
        self.main_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.main_container.set_vexpand(True)
        self.main_container.set_hexpand(True)
        
        self.content_container = ContentContainer(
            spacing=20,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="message-viewer-content",
            
            h_fill=True,
            w_fill=True,
        )
        
        self.scroll_window = Gtk.ScrolledWindow()
        self.scroll_window.set_vexpand(True)
        self.scroll_window.set_hexpand(True)
        self.scroll_window.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.scroll_window.add_css_class("message-viewer-scroll")
        self.scroll_window.set_child(self.content_container.widget)
        
        self.main_container.append(self.scroll_window)
        self.widget.add(self.main_container)
        
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
        
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
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
        """Display all messages in a thread"""
        logging.info(f"MessageViewer: Displaying thread with {len(thread.messages)} messages")
        
        
        self.hide_all_states()
        
        
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        
        self.current_state = "content"
        
        self.content_container.widget.set_valign(Gtk.Align.FILL)
        self.content_container.widget.set_halign(Gtk.Align.FILL)
        
        
        if self.content_header:
            self.content_header.set_message_subject(thread.get_display_subject())
        
        
        self.thread_message_cards = {}
        
        
        for i, message in enumerate(thread.messages):
            logging.debug(f"MessageViewer: Creating card for message {i+1}: UID={message.get('uid')}, has_body={self.message_has_body(message)}")
            
            message_card = self.create_message_card(message, is_in_thread=True, thread_position=i+1, thread_total=len(thread.messages))
            self.content_container.widget.append(message_card)
            
            
            message_uid = message.get("uid")
            if message_uid:
                self.thread_message_cards[message_uid] = message_card
            
            
            if not self.message_has_body(message):
                logging.debug(f"MessageViewer: Message UID {message_uid} needs body, trying database first")
                self.fetch_message_body_smart(message, thread)
            else:
                logging.debug(f"MessageViewer: Message UID {message_uid} already has body")
        
        logging.info(f"MessageViewer: Thread display complete, {len(thread.messages)} cards created")

    def create_message_card(self, message, is_in_thread=False, thread_position=None, thread_total=None):
        """Create a message card for display"""
        message_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        message_card.add_css_class("message-card")
        if is_in_thread:
            message_card.add_css_class("message-card-in-thread")
        
        
        card_header = self.create_message_card_header(message)
        message_card.append(card_header)
        
        
        loading = not self.message_has_body(message)
        card_body = self.create_message_card_body(message, loading=loading)
        message_card.append(card_body)
        
        return message_card

    def fetch_message_body_for_thread(self, message, thread):
        """Fetch message body for a message in a thread"""
        if not self.current_account_data:
            logging.warning("MessageViewer: No account data available for body fetch")
            return
        
        message_uid = message.get("uid")
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
        
        if not all([message_uid, folder, account_id]):
            logging.warning(f"MessageViewer: Missing required fields for database fetch: UID={message_uid}, folder={folder}, account_id={account_id}")
            
            if thread:
                self.fetch_message_body_for_thread(message, thread)
            else:
                self.fetch_message_body_from_imap_direct(message)
            return
        
        try:
            
            db_message = self.storage.get_message_by_uid(message_uid, folder, account_id)
            
            if db_message and self.message_has_body(db_message):
                logging.info(f"MessageViewer: Found body in database for UID {message_uid}")
                
                message["body"] = db_message.get("body", "")
                message["body_html"] = db_message.get("body_html", "")
                
                
                if thread and message_uid in getattr(self, 'thread_message_cards', {}):
                    self.update_message_card_body(message_uid, message)
                elif not thread:
                    
                    self.display_message(message)
            else:
                logging.debug(f"MessageViewer: No body found in database for UID {message_uid}, fetching from IMAP")
                
                if thread:
                    self.fetch_message_body_for_thread(message, thread)
                else:
                    self.fetch_message_body_from_imap_direct(message)
                    
        except Exception as e:
            logging.error(f"MessageViewer: Error fetching from database for UID {message_uid}: {e}")
            
            if thread:
                self.fetch_message_body_for_thread(message, thread)
            else:
                self.fetch_message_body_from_imap_direct(message)

    def fetch_message_body_from_imap_direct(self, message):
        """Fetch message body directly from IMAP (for single messages)"""
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
        
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        
        
        self.hide_all_states()
        self.current_state = "content"
        
        self.content_container.widget.set_valign(Gtk.Align.FILL)
        self.content_container.widget.set_halign(Gtk.Align.FILL)
        
        
        if self.content_header:
            self.content_header.set_message_subject(message.get("subject", "(No Subject)"))
        
        
        message_card = self.create_message_card(message)
        self.content_container.widget.append(message_card)
    
    def create_message_card_header(self, message):
        """Create the card header with avatar, sender email, and shortened date"""
        header_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        header_container.add_css_class("message-card-header")
        header_container.set_spacing(12)
        
        
        sender_info = message.get("sender", {})
        sender_name = sender_info.get("name", "")
        sender_email = sender_info.get("email", "")
        
        
        initials = self.get_initials(sender_name, sender_email)
        avatar = Adw.Avatar.new(32, initials, True)
        avatar.add_css_class("message-avatar")
        avatar.set_valign(Gtk.Align.CENTER)
        header_container.append(avatar)
        
        
        sender_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        sender_container.set_hexpand(True)
        sender_container.set_spacing(2)
        sender_container.set_valign(Gtk.Align.CENTER)
        
        display_name = sender_name if sender_name else sender_email
        name_label = Gtk.Label(label=display_name)
        name_label.set_halign(Gtk.Align.START)
        name_label.set_valign(Gtk.Align.CENTER)
        name_label.add_css_class("message-card-sender")
        sender_container.append(name_label)
        
        if sender_name and sender_email:
            email_label = Gtk.Label(label=sender_email)
            email_label.set_halign(Gtk.Align.START)
            email_label.set_valign(Gtk.Align.CENTER)
            email_label.add_css_class("message-card-email")
            sender_container.append(email_label)
        
        header_container.append(sender_container)
        
        
        date_str = message.get("date", "")
        if date_str:
            short_date = self.get_shortened_date(date_str)
            date_label = Gtk.Label(label=short_date)
            date_label.add_css_class("message-card-date")
            date_label.set_halign(Gtk.Align.END)
            date_label.set_valign(Gtk.Align.CENTER)
            header_container.append(date_label)
        
        return header_container
       
    def create_message_card_body(self, message, loading=False):
        """Create the card body with message content or loading state"""
        body_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        body_container.add_css_class("message-card-body")
        
        
        body_text = message.get("body", "")
        body_html = message.get("body_html", "")
        
        if body_html or body_text:
            html_viewer = HtmlViewer()
            if body_html:
                html_viewer.load_html(body_html)
            elif body_text:
                html_viewer.load_plain_text(body_text)
            body_container.append(html_viewer.widget)
        elif loading:
            
            loading_container = self.create_message_body_loading_state()
            body_container.append(loading_container)
        else:
            
            no_body_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            no_body_container.set_margin_top(12)
            no_body_container.set_margin_bottom(12)
            no_body_container.set_margin_start(12)
            no_body_container.set_margin_end(12)
            no_body_container.add_css_class("message-body")
            
            no_body_label = AppText(
                text="(No message body)",
                class_names="message-no-body",
                halign=Gtk.Align.CENTER,
            )
            no_body_label.set_opacity(0.6)
            no_body_container.append(no_body_label.widget)
            body_container.append(no_body_container)
        
        return body_container

    def create_message_body_loading_state(self):
        """Create loading state for message body"""
        loading_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        loading_container.set_margin_top(12)
        loading_container.set_margin_bottom(12)
        loading_container.set_margin_start(12)
        loading_container.set_margin_end(12)
        loading_container.set_spacing(8)
        loading_container.set_halign(Gtk.Align.CENTER)
        loading_container.add_css_class("message-body-loading")
        
        
        spinner = Gtk.Spinner()
        spinner.set_size_request(16, 16)
        spinner.start()
        loading_container.append(spinner)
        
        
        loading_label = AppText(
            text="Loading...",
            class_names="message-body-loading-text",
            halign=Gtk.Align.CENTER,
        )
        loading_label.set_opacity(0.7)
        loading_container.append(loading_label.widget)
        
        return loading_container
    
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
        
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        
        
        if self.content_header:
            self.content_header.set_message_subject(None)
        
        
        self.content_container.widget.set_valign(Gtk.Align.CENTER)
        self.content_container.widget.set_halign(Gtk.Align.CENTER)
        
        
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
        
        self.content_container.widget.append(state_container)
        self.current_state = "empty"

    def show_loading_state(self):
        self.hide_all_states()
        self.content_container.widget.set_valign(Gtk.Align.CENTER)
        self.content_container.widget.set_halign(Gtk.Align.CENTER)
        
        
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-loading-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)
        state_container.set_hexpand(True)
        
        
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
        
        self.content_container.widget.append(state_container)
        self.current_state = "loading"

    def show_empty_state(self):
        self.show_select_message_state()
    
    def show_error_state(self, error_message=None, raw_body=None):
        self.hide_all_states()
        self.content_container.widget.set_valign(Gtk.Align.CENTER)
        self.content_container.widget.set_halign(Gtk.Align.CENTER)
        
        
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-error-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)
        state_container.set_hexpand(True)
        
        
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
        
        self.content_container.widget.append(state_container)
        self.current_state = "error"
    
    def hide_all_states(self):
        if self.current_state == "loading":
            self.main_container.remove(self.loading_state.widget)
        elif self.current_state == "error":
            self.main_container.remove(self.error_state.widget)
        self.current_state = None 

    def set_content_header(self, content_header):
        """Set the content header reference for updating title"""
        self.content_header = content_header 

    def update_message_card_body(self, message_uid, message):
        """Update the body content of a specific message card"""
        if not hasattr(self, 'thread_message_cards') or message_uid not in self.thread_message_cards:
            logging.warning(f"MessageViewer: No message card found for UID {message_uid} to update")
            return
            
        message_card = self.thread_message_cards[message_uid]
        logging.debug(f"MessageViewer: Updating body for message card UID {message_uid}")
        
        
        
        
        body_child = None
        child = message_card.get_first_child()
        while child:
            if child.has_css_class("message-card-body"):
                body_child = child
                break
            child = child.get_next_sibling()
        
        if body_child:
            logging.debug(f"MessageViewer: Found existing body child for UID {message_uid}, replacing it")
            
            message_card.remove(body_child)
            
            
            
            new_body = self.create_message_card_body(message)
            message_card.append(new_body)
            logging.info(f"MessageViewer: Successfully updated message card body for UID {message_uid}")
        else:
            logging.warning(f"MessageViewer: Could not find body child in message card for UID {message_uid}")
            
            child = message_card.get_first_child()
            child_count = 0
            while child:
                child_classes = []
                
                try:
                    css_classes = child.get_css_classes()
                    child_classes = list(css_classes)
                except:
                    child_classes = ["unknown"]
                logging.debug(f"MessageViewer: Child {child_count} classes: {child_classes}")
                child = child.get_next_sibling()
                child_count += 1 