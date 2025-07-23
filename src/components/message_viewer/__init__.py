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
        self.content_header = None  # Will be set by main window
        
        self.widget = Adw.PreferencesGroup()
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        self.widget.add_css_class("message-viewer-root")
        
        # Main container for all content
        self.main_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.main_container.set_vexpand(True)
        self.main_container.set_hexpand(True)
        
        self.content_container = ContentContainer(
            spacing=20,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="message-viewer-content",
            # margin=20,  # Remove outer margin
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
    
    def show_message(self, message):
        # Always clear previous content
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        if not message:
            self.show_select_message_state()
            return
        
        self.current_message = message
        self.show_loading_state()
        
        # Check if we have the message body in storage
        if message.get("body") and message["body"].strip():
            self.display_message(message)
        else:
            # Fetch message body from IMAP
            self.fetch_message_body(message)
    
    def fetch_message_body(self, message):
        if not self.current_account_data:
            logging.error("No account data set in MessageViewer")
            self.show_error_state("No account data set")
            return
        
        # Increment fetch ID to cancel any ongoing operations
        self.body_fetch_id += 1
        fetch_id = self.body_fetch_id
        
        logging.info(f"MessageViewer: Fetching body for message UID {message.get('uid')}")
        
        def on_body_fetched(error, message_body_data):
            # Check if this fetch operation has been cancelled
            if fetch_id != self.body_fetch_id:
                logging.debug(f"MessageViewer: Body fetch operation {fetch_id} was cancelled, ignoring response")
                return
            
            if error:
                logging.error(f"MessageViewer: Error fetching message body: {error}")
                self.show_error_state(str(error))
                return
            
            if message_body_data:
                # Update message with body content
                if isinstance(message_body_data, dict):
                    message["body"] = message_body_data.get("text", "")
                    message["body_html"] = message_body_data.get("html", "")
                else:
                    # Fallback for old format
                    message["body"] = message_body_data
                    message["body_html"] = ""
                
                # Store updated message in database
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
        # Clear current content
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        
        # Show message content
        self.hide_all_states()
        self.current_state = "content"
        # Set alignment to fill for message content
        self.content_container.widget.set_valign(Gtk.Align.FILL)
        self.content_container.widget.set_halign(Gtk.Align.FILL)
        
        # Update content header with subject
        if self.content_header:
            self.content_header.set_message_subject(message.get("subject", "(No Subject)"))
        
        # Create message card
        message_card = Adw.PreferencesGroup()
        message_card.add_css_class("message-card")
        
        # Card header with avatar, sender, and date
        card_header = self.create_message_card_header(message)
        message_card.add(card_header)
        
        # Card body with message content
        card_body = self.create_message_card_body(message)
        message_card.add(card_body)
        
        self.content_container.widget.append(message_card)
    
    def create_message_card_header(self, message):
        """Create the card header with avatar, sender email, and shortened date"""
        header_row = Adw.ActionRow()
        header_row.add_css_class("message-card-header")
        
        # Get sender info
        sender_info = message.get("sender", {})
        sender_name = sender_info.get("name", "")
        sender_email = sender_info.get("email", "")
        
        # Create avatar with initials (max 2 characters)
        initials = self.get_initials(sender_name, sender_email)
        avatar = Adw.Avatar.new(40, initials, True)
        avatar.add_css_class("message-avatar")
        header_row.add_prefix(avatar)
        
        # Set title and subtitle
        display_name = sender_name if sender_name else sender_email
        header_row.set_title(display_name)
        if sender_name and sender_email:
            header_row.set_subtitle(sender_email)
        
        # Add shortened date on the right
        date_str = message.get("date", "")
        if date_str:
            short_date = self.get_shortened_date(date_str)
            date_label = Gtk.Label(label=short_date)
            date_label.add_css_class("message-card-date")
            date_label.set_halign(Gtk.Align.END)
            header_row.add_suffix(date_label)
        
        return header_row
    
    def create_message_card_body(self, message):
        """Create the card body with message content"""
        body_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        body_container.add_css_class("message-card-body")
        body_container.set_margin_top(12)
        body_container.set_margin_bottom(12)
        body_container.set_margin_start(12)
        body_container.set_margin_end(12)
        
        # Message body
        body_text = message.get("body", "")
        body_html = message.get("body_html", "")
        
        if body_html:
            # Use HTML viewer for HTML content
            html_viewer = HtmlViewer(
                class_names="message-body",
                h_fill=True,
                w_fill=True,
            )
            html_viewer.load_html(body_html)
            # Round the HTML viewer at bottom to fit card
            html_viewer.widget.add_css_class("message-body-rounded")
            body_container.append(html_viewer.widget)
        elif body_text:
            # Use HTML viewer for plain text with proper formatting
            html_viewer = HtmlViewer(
                class_names="message-body",
                h_fill=True,
                w_fill=True,
            )
            html_viewer.load_plain_text(body_text)
            # Round the HTML viewer at bottom to fit card
            html_viewer.widget.add_css_class("message-body-rounded")
            body_container.append(html_viewer.widget)
        else:
            # No body message
            no_body_label = AppText(
                text="(No message body)",
                class_names="message-no-body",
                halign=Gtk.Align.CENTER,
            )
            no_body_label.set_opacity(0.6)
            body_container.append(no_body_label.widget)
        
        return body_container
    
    def get_initials(self, name, email):
        """Get initials for avatar (max 2 characters)"""
        if name:
            # Split name and take first letter of first two words
            words = name.split()
            if len(words) >= 2:
                return f"{words[0][0]}{words[1][0]}".upper()
            elif len(words) == 1:
                return words[0][:2].upper()
        
        if email:
            # Use first two characters of email
            return email[:2].upper()
        
        return "??"
    
    def get_shortened_date(self, date_str):
        """Get shortened date for display"""
        try:
            from datetime import datetime
            import email.utils
            
            parsed_date = email.utils.parsedate_tz(date_str)
            if parsed_date:
                timestamp = email.utils.mktime_tz(parsed_date)
                date_obj = datetime.fromtimestamp(timestamp)
                
                now = datetime.now()
                diff = now - date_obj
                
                if diff.days == 0:
                    # Today - show time
                    return date_obj.strftime("%H:%M")
                elif diff.days == 1:
                    # Yesterday
                    return "Yesterday"
                elif diff.days < 7:
                    # This week - show day name
                    return date_obj.strftime("%a")
                elif diff.days < 365:
                    # This year - show month and day
                    return date_obj.strftime("%b %d")
                else:
                    # Older - show year
                    return date_obj.strftime("%Y")
        except:
            pass
        
        return ""
    
    def show_select_message_state(self):
        self.hide_all_states()
        # Clear all children from the content container
        child = self.content_container.widget.get_first_child()
        while child:
            self.content_container.widget.remove(child)
            child = self.content_container.widget.get_first_child()
        
        # Clear content header
        if self.content_header:
            self.content_header.set_message_subject(None)
        
        # Set alignment to center for empty state
        self.content_container.widget.set_valign(Gtk.Align.CENTER)
        self.content_container.widget.set_halign(Gtk.Align.CENTER)
        
        # Create container with same structure as message list
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-empty-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)  # Ensure vertical centering
        state_container.set_hexpand(True)
        
        # Icon
        icon = AppIcon("mail-unread-symbolic", class_names="message-viewer-empty-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)
        
        # Text
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
        
        # Create container with same structure as message list
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-loading-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)
        state_container.set_hexpand(True)
        
        # Spinner
        spinner = Gtk.Spinner()
        spinner.add_css_class("message-viewer-loading-spinner")
        spinner.set_size_request(32, 32)
        spinner.start()
        
        # Text
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
        
        # Create container with same structure as message list
        state_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        state_container.add_css_class("message-viewer-error-state")
        state_container.set_halign(Gtk.Align.CENTER)
        state_container.set_valign(Gtk.Align.CENTER)
        state_container.set_spacing(12)
        state_container.set_vexpand(True)
        state_container.set_hexpand(True)
        
        # Icon
        icon = AppIcon("dialog-error-symbolic", class_names="message-viewer-error-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)
        
        # Text
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