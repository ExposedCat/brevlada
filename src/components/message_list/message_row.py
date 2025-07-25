from utils.toolkit import Gtk, Pango
from components.ui import AppIcon, AppText
from theme import MESSAGE_ROW_ICON_GAP, THEME_ROW_GAP, THEME_ROW_VERTICAL_GAP
from components.container import ContentContainer
import logging

class MessageRow:
    def __init__(self, message_or_thread):
        self.message_or_thread = message_or_thread
        self.selected_callback = None
        self.read_changed_callback = None
        self.storage = None
        self.current_account_data = None

        
        self.is_thread = hasattr(message_or_thread, "messages") and hasattr(
            message_or_thread, "get_unread_count"
        )

        self.widget = Gtk.ListBoxRow()
        self.widget.set_activatable(True)
        self.widget.set_selectable(True)
        self.widget.add_css_class("message-row")

        self.container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=THEME_ROW_GAP)
        self.container.add_css_class("message-row-container")

        
        self.left_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.left_container.set_spacing(MESSAGE_ROW_ICON_GAP)
        self.left_container.add_css_class("message-row-left")

        self.read_indicator = self.create_read_indicator()
        self.left_container.append(self.read_indicator)

        
        self.content_container = ContentContainer(
            spacing=THEME_ROW_VERTICAL_GAP,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="message-row-content",
            children=None
        ).widget

        
        sender_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=THEME_ROW_GAP)
        sender_container.add_css_class("message-row-sender-container")
        if self.is_thread and self.get_message_count() > 1:
            self.thread_count_badge = self.create_thread_count_badge()
            sender_container.append(self.thread_count_badge)
        
        
        sender_classes = ["heading"]
        if not self.get_is_read():
            sender_classes.append("message-row-sender-unread")
        
        self.sender_label = AppText(self.get_display_sender(), class_names=sender_classes)
        sender_container.append(self.sender_label.widget)

        self.content_container.append(sender_container)

        self.subject_label = AppText(
            self.get_display_subject(), class_names="dim-label"
        )
        self.subject_label.widget.add_css_class("message-row-subject-label")
        self.content_container.append(self.subject_label.widget)

        self.left_container.append(self.content_container)
        self.container.append(self.left_container)

        
        self.right_container = ContentContainer(
            spacing=THEME_ROW_VERTICAL_GAP,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="message-row-right",
            children=None
        ).widget

        
        self.date_label = AppText(
            self.get_display_date(),
            halign=Gtk.Align.END,
            class_names="message-row-date",
        )
        self.right_container.append(self.date_label.widget)

        
        self.icons_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=THEME_ROW_GAP)
        self.icons_container.set_halign(Gtk.Align.END)
        self.icons_container.add_css_class("message-row-icons")
        if self.get_is_flagged():
            self.flag_indicator = self.create_flag_indicator()
            self.flag_indicator.add_css_class("message-row-flag-icon")
            self.icons_container.append(self.flag_indicator)
        self.right_container.append(self.icons_container)
        self.container.append(self.right_container)

        self.widget.set_child(self.container)
        self.widget.connect("activate", self.on_activated)

    def create_read_indicator(self):
        if self.get_is_read():
            indicator = AppIcon("mail-read-symbolic")
        else:
            indicator = AppIcon("mail-unread-symbolic")
        return indicator.widget

    def create_attachment_indicator(self):
        attachment_icon = AppIcon("mail-attachment-symbolic")
        return attachment_icon.widget

    def create_flag_indicator(self):
        return AppIcon("starred-symbolic").widget

    def on_activated(self, row):
        if self.selected_callback:
            if self.is_thread:
                self.selected_callback(self.message_or_thread)
            else:
                self.selected_callback(self.message_or_thread)

    def get_is_read(self):
        if self.is_thread:
            return self.message_or_thread.get_unread_count() == 0
        elif isinstance(self.message_or_thread, dict):
            return self.message_or_thread.get("is_read", False)
        else:
            return self.message_or_thread.is_read

    def get_is_flagged(self):
        if self.is_thread:
            return self.message_or_thread.is_flagged
        elif isinstance(self.message_or_thread, dict):
            return self.message_or_thread.get("is_flagged", False)
        else:
            return self.message_or_thread.is_flagged

    def get_display_sender(self):
        if self.is_thread:
            return self.message_or_thread.get_display_sender()
        elif isinstance(self.message_or_thread, dict):
            sender = self.message_or_thread.get("sender", {})
            if sender.get("name"):
                return sender["name"]
            elif sender.get("email"):
                return sender["email"]
            else:
                return "Unknown Sender"
        else:
            return self.message_or_thread.get_display_sender()

    def get_display_subject(self):
        if self.is_thread:
            return self.message_or_thread.get_display_subject()
        elif isinstance(self.message_or_thread, dict):
            subject = self.message_or_thread.get("subject", "")
            return subject if subject else "(No Subject)"
        else:
            return self.message_or_thread.get_display_subject()

    def get_display_date(self):
        if self.is_thread:
            return self.message_or_thread.get_display_date()
        elif isinstance(self.message_or_thread, dict):
            date_str = self.message_or_thread.get("date", "")
            if date_str:
                from datetime import datetime

                try:
                    if isinstance(date_str, str):
                        
                        import email.utils

                        parsed_date = email.utils.parsedate_tz(date_str)
                        if parsed_date:
                            timestamp = email.utils.mktime_tz(parsed_date)
                            date_obj = datetime.fromtimestamp(timestamp)

                            now = datetime.now()
                            diff = now - date_obj

                            if diff.days == 0:
                                return date_obj.strftime("%H:%M")
                            elif diff.days == 1:
                                return "Yesterday"
                            elif diff.days < 7:
                                return date_obj.strftime("%A")
                            elif diff.days < 365:
                                return date_obj.strftime("%b %d")
                            else:
                                return date_obj.strftime("%b %d, %Y")
                except:
                    pass
            return ""
        else:
            return self.message_or_thread.get_display_date()

    def get_attachment_count(self):
        if self.is_thread:
            return 1 if self.message_or_thread.has_attachments else 0
        elif isinstance(self.message_or_thread, dict):
            attachments = self.message_or_thread.get("attachments", [])
            return len(attachments)
        else:
            return self.message_or_thread.get_attachment_count()

    def get_message_count(self):
        if self.is_thread:
            return len(self.message_or_thread.messages)
        else:
            return 1

    def create_thread_count_badge(self):
        """Create a circular badge showing thread message count"""
        count = self.get_message_count()

        
        badge_label = Gtk.Label(label=str(count))
        badge_label.add_css_class("thread-count-badge-label")
        

        
        badge_box = Gtk.Box()
        badge_box.append(badge_label)
        badge_box.add_css_class("thread-count-container")

        return badge_box

    def mark_as_read(self):
        if self.get_is_read():
            return

        if not self.is_thread:
            if isinstance(self.message_or_thread, dict):
                self.message_or_thread["is_read"] = True
                
                if self.storage and self.current_account_data:
                    uid = self.message_or_thread.get("uid")
                    folder = self.message_or_thread.get("folder")
                    account_id = self.current_account_data.get("email")
                    
                    if uid and folder and account_id:
                        try:
                            self.storage.update_message_read_status(uid, folder, account_id, True)
                            
                            from utils.mail import mark_message_as_read_on_imap
                            def on_imap_update(error, result):
                                if error:
                                    logging.error(f"MessageRow: IMAP update failed: {error}")
                                else:
                                    logging.debug(f"MessageRow: IMAP update successful: {result}")
                            
                            mark_message_as_read_on_imap(self.current_account_data, folder, uid, on_imap_update)
                        except Exception as e:
                            logging.error(f"MessageRow: Storage/IMAP update failed: {e}")
                    else:
                        logging.warning(f"MessageRow: Missing required data for storage update - UID: {uid}, folder: {folder}, account: {account_id}")
                else:
                    logging.warning(f"MessageRow: No storage or account data available")
            else:
                self.message_or_thread.is_read = True

        # Update visual elements
        self.container.remove_css_class("message-row-unread")
        self.sender_label.widget.remove_css_class("message-row-sender-unread")
        self.subject_label.widget.remove_css_class("message-row-subject-unread")

        self.left_container.remove(self.read_indicator)
        self.read_indicator = self.create_read_indicator()
        self.left_container.prepend(self.read_indicator)

        if self.read_changed_callback:
            self.read_changed_callback(self.message_or_thread)

    def toggle_flag(self):
        if not self.is_thread:
            if isinstance(self.message_or_thread, dict):
                self.message_or_thread["is_flagged"] = not self.message_or_thread.get("is_flagged", False)
            else:
                self.message_or_thread.is_flagged = not self.message_or_thread.is_flagged

            if self.get_is_flagged():
                if not hasattr(self, "flag_indicator"):
                    self.flag_indicator = self.create_flag_indicator()
                    self.flag_indicator.add_css_class("message-row-flag-icon")
                    self.icons_container.append(self.flag_indicator)
            else:
                if hasattr(self, "flag_indicator"):
                    self.icons_container.remove(self.flag_indicator)
                    delattr(self, "flag_indicator")

    def update_display(self):
        if self.is_thread:
            return
            
        if isinstance(self.message_or_thread, dict):
            self.sender_label.set_text_content(self.get_display_sender())
            self.subject_label.set_text_content(self.get_display_subject())
            self.date_label.set_text_content(self.get_display_date())

            if not self.get_is_read():
                self.container.add_css_class("message-row-unread")
                self.sender_label.widget.add_css_class("message-row-sender-unread")
                self.subject_label.widget.add_css_class("message-row-subject-unread")
            else:
                self.container.remove_css_class("message-row-unread")
                self.sender_label.widget.remove_css_class("message-row-sender-unread")
                self.subject_label.widget.remove_css_class("message-row-subject-unread")

            self.left_container.remove(self.read_indicator)
            self.read_indicator = self.create_read_indicator()
            self.left_container.prepend(self.read_indicator)

            if self.get_attachment_count() > 0:
                if not hasattr(self, "attachment_indicator"):
                    self.attachment_indicator = self.create_attachment_indicator()
                    self.icons_container.append(self.attachment_indicator)
            else:
                if hasattr(self, "attachment_indicator"):
                    self.icons_container.remove(self.attachment_indicator)
                    delattr(self, "attachment_indicator")

    def get_message(self):
        return self.message_or_thread

    def connect_selected(self, callback):
        self.selected_callback = callback

    def connect_read_changed(self, callback):
        self.read_changed_callback = callback

    def set_storage_and_account(self, storage, account_data):
        self.storage = storage
        self.current_account_data = account_data

    def set_selected(self, selected):
        if selected:
            self.widget.add_css_class("message-row-selected")
        else:
            self.widget.remove_css_class("message-row-selected")

    def is_selected(self):
        return self.widget.has_css_class("message-row-selected")

    def get_height_request(self):
        return self.widget.get_allocated_height()

    def set_height_request(self, height):
        self.widget.set_size_request(-1, height)

    def get_sensitive(self):
        return self.widget.get_sensitive()

    def set_sensitive(self, sensitive):
        self.widget.set_sensitive(sensitive)
