from utils.toolkit import Gtk, Pango
from components.ui import AppIcon, AppText


class MessageRow:
    def __init__(self, message_or_thread):
        self.message_or_thread = message_or_thread
        self.selected_callback = None
        self.read_changed_callback = None

        # Determine if this is a thread or single message
        self.is_thread = hasattr(message_or_thread, "messages") and hasattr(
            message_or_thread, "get_unread_count"
        )

        self.widget = Gtk.ListBoxRow()
        self.widget.set_activatable(True)
        self.widget.set_selectable(True)

        self.container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.container.set_spacing(12)
        self.container.set_margin_top(10)
        self.container.set_margin_bottom(10)
        self.container.set_margin_start(16)
        self.container.set_margin_end(16)
        self.container.set_hexpand(True)

        # Left side: read indicator and message content
        self.left_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.left_container.set_spacing(12)
        self.left_container.set_hexpand(True)
        self.left_container.set_valign(Gtk.Align.CENTER)

        self.read_indicator = self.create_read_indicator()
        self.left_container.append(self.read_indicator)

        # Message content: sender and subject vertically stacked
        self.content_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.content_container.set_spacing(4)
        self.content_container.set_hexpand(True)
        self.content_container.set_valign(Gtk.Align.CENTER)

        # Sender with optional thread count
        sender_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        sender_container.set_spacing(8)
        sender_container.set_halign(Gtk.Align.START)

        self.sender_label = AppText(self.get_display_sender(), class_names="heading")
        sender_container.append(self.sender_label.widget)

        # Add thread count badge if this is a thread
        if self.is_thread and self.get_message_count() > 1:
            self.thread_count_badge = self.create_thread_count_badge()
            sender_container.append(self.thread_count_badge)

        self.content_container.append(sender_container)

        self.subject_label = AppText(
            self.get_display_subject(), class_names="dim-label"
        )
        self.subject_label.widget.set_halign(Gtk.Align.START)
        self.subject_label.widget.set_ellipsize(Pango.EllipsizeMode.END)
        self.content_container.append(self.subject_label.widget)

        self.left_container.append(self.content_container)
        self.container.append(self.left_container)

        # Right side: time and icons
        self.right_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.right_container.set_spacing(6)
        self.right_container.set_valign(Gtk.Align.CENTER)
        self.right_container.set_halign(Gtk.Align.END)

        self.date_label = AppText(
            self.get_display_date(), halign=Gtk.Align.END, class_names="dim-label"
        )
        self.date_label.widget.set_halign(Gtk.Align.END)
        self.right_container.append(self.date_label.widget)

        # Icons container
        self.icons_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.icons_container.set_spacing(6)
        self.icons_container.set_halign(Gtk.Align.END)
        self.icons_container.set_valign(Gtk.Align.CENTER)

        if self.get_attachment_count() > 0:
            self.attachment_indicator = self.create_attachment_indicator()
            self.icons_container.append(self.attachment_indicator)

        if self.get_is_flagged():
            self.flag_indicator = self.create_flag_indicator()
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
                # For threads, pass the latest message
                latest_message = (
                    self.message_or_thread.messages[-1]
                    if self.message_or_thread.messages
                    else None
                )
                if latest_message:
                    self.selected_callback(latest_message)
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
                        # Try to parse the date string
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

        # Create a label with the count
        badge_label = Gtk.Label(label=str(count))
        badge_label.add_css_class("thread-count-badge")

        # Wrap in a box for styling
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
            else:
                self.message_or_thread.is_read = True

        self.container.remove_css_class("message-row-unread")
        self.sender_label.widget.remove_css_class("message-row-sender-unread")
        self.subject_label.widget.remove_css_class("message-row-subject-unread")

        self.container.remove(self.read_indicator)
        self.read_indicator = self.create_read_indicator()
        self.container.prepend(self.read_indicator)

        if self.read_changed_callback:
            self.read_changed_callback(self.message)

    def mark_as_unread(self):
        if not self.message.is_read:
            return

        self.message.is_read = False

        self.container.add_css_class("message-row-unread")
        self.sender_label.widget.add_css_class("message-row-sender-unread")
        self.subject_label.widget.add_css_class("message-row-subject-unread")

        self.container.remove(self.read_indicator)
        self.read_indicator = self.create_read_indicator()
        self.container.prepend(self.read_indicator)

        if self.read_changed_callback:
            self.read_changed_callback(self.message)

    def toggle_flag(self):
        self.message.is_flagged = not self.message.is_flagged

        if self.message.is_flagged:
            if not hasattr(self, "flag_indicator"):
                self.flag_indicator = self.create_flag_indicator()
                self.container.append(self.flag_indicator)
        else:
            if hasattr(self, "flag_indicator"):
                self.container.remove(self.flag_indicator)
                delattr(self, "flag_indicator")

    def update_display(self):
        self.sender_label.set_text_content(self.message.get_display_sender())
        self.subject_label.set_text_content(self.message.get_display_subject())
        self.date_label.set_text_content(self.message.get_display_date())

        if not self.message.is_read:
            self.container.add_css_class("message-row-unread")
            self.sender_label.widget.add_css_class("message-row-sender-unread")
            self.subject_label.widget.add_css_class("message-row-subject-unread")
        else:
            self.container.remove_css_class("message-row-unread")
            self.sender_label.widget.remove_css_class("message-row-sender-unread")
            self.subject_label.widget.remove_css_class("message-row-subject-unread")

        self.container.remove(self.read_indicator)
        self.read_indicator = self.create_read_indicator()
        self.container.prepend(self.read_indicator)

        if self.message.get_attachment_count() > 0:
            if not hasattr(self, "attachment_indicator"):
                self.attachment_indicator = self.create_attachment_indicator()
                self.container.append(self.attachment_indicator)
        else:
            if hasattr(self, "attachment_indicator"):
                self.container.remove(self.attachment_indicator)
                delattr(self, "attachment_indicator")

    def get_message(self):
        return self.message

    def connect_selected(self, callback):
        self.selected_callback = callback

    def connect_read_changed(self, callback):
        self.read_changed_callback = callback

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
