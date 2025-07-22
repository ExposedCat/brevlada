from utils.toolkit import Gtk
from components.ui import AppIcon, AppText
from components.button import AppButton


class ThreadRow:
    def __init__(self, thread):
        self.thread = thread
        self.is_expanded = False
        self.selected_message = None
        self.message_selected_callback = None
        self.expanded_changed_callback = None
        self.message_row_instances = {}

        self.widget = Gtk.ListBoxRow()
        self.widget.add_css_class("thread-row")

        self.main_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.widget.set_child(self.main_container)

        self.header_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.header_container.set_spacing(12)
        self.header_container.add_css_class("thread-header-container")

        if self.thread.get_unread_count() > 0:
            self.header_container.add_css_class("thread-row-unread")

        self.expand_button = AppButton(class_names="thread-expand-button")
        self.expand_button.widget.set_icon_name("pan-end-symbolic")
        self.expand_button.widget.set_size_request(24, 24)
        self.expand_button.connect("clicked", self.on_expand_clicked)

        self.read_indicator = self.create_read_indicator()

        self.sender_label = AppText(
            self.thread.get_display_sender(), class_names="thread-row-sender"
        )
        self.sender_label.widget.set_size_request(150, -1)

        self.subject_label = AppText(
            self.thread.get_display_subject(), class_names="thread-row-subject"
        )

        self.thread_subtitle = self.create_thread_subtitle()

        self.indicators_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.indicators_container.set_spacing(6)
        self.setup_thread_indicators()

        self.date_label = AppText(
            self.thread.get_display_date(),
            halign=Gtk.Align.END,
            class_names="thread-date-label",
        )
        self.date_label.widget.set_size_request(80, -1)

        self.header_container.append(self.expand_button.widget)
        self.header_container.append(self.read_indicator)
        self.header_container.append(self.sender_label.widget)
        self.header_container.append(self.subject_label.widget)
        self.header_container.append(self.thread_subtitle)
        self.header_container.append(self.indicators_container)
        self.header_container.append(self.date_label.widget)

        self.main_container.append(self.header_container)

        self.message_list_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.message_list_container.add_css_class("thread-message-list")
        self.message_list_container.set_visible(False)
        self.main_container.append(self.message_list_container)

        self.message_list = Gtk.ListBox()
        self.message_list.add_css_class("thread-message-rows")
        self.message_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.message_list.connect("row-activated", self.on_message_row_activated)
        self.message_list_container.append(self.message_list)

    def create_thread_subtitle(self):
        participants = self.thread.get_participant_summary()
        message_count = len(self.thread.messages)

        if message_count > 1:
            subtitle_text = f"{message_count} messages"
            if participants:
                subtitle_text += f" • {participants}"
        else:
            subtitle_text = participants if participants else ""

        subtitle = AppText(subtitle_text, class_names="thread-subtitle")
        subtitle.widget.set_size_request(200, -1)
        subtitle.set_opacity(0.7)

        return subtitle.widget

    def setup_thread_indicators(self):
        unread_count = self.thread.get_unread_count()
        if unread_count > 0:
            unread_badge = AppText(
                str(unread_count),
                halign=Gtk.Align.CENTER,
                class_names="thread-unread-badge",
            )
            unread_badge.widget.set_size_request(20, 20)
            self.indicators_container.append(unread_badge.widget)

        has_attachments = any(
            msg.get_attachment_count() > 0 for msg in self.thread.messages
        )
        if has_attachments:
            attachment_icon = AppIcon(
                "mail-attachment-symbolic", class_names="thread-attachment-icon"
            )
            attachment_icon.set_pixel_size(16)
            self.indicators_container.append(attachment_icon.widget)

        if self.thread.is_flagged:
            flag_icon = AppIcon("starred-symbolic", class_names="thread-flag-icon")
            flag_icon.set_pixel_size(16)
            self.indicators_container.append(flag_icon.widget)

    def populate_message_rows(self):
        self.message_row_instances.clear()
        while True:
            row = self.message_list.get_first_child()
            if row is None:
                break
            self.message_list.remove(row)

        for message in self.thread.messages:
            message_row = self.create_message_row(message)
            self.message_list.append(message_row)

    def create_message_row(self, message):
        row = Gtk.ListBoxRow()
        row.add_css_class("thread-message-row")

        container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        container.set_spacing(12)
        container.add_css_class("thread-message-row-container")

        if not message.is_read:
            container.add_css_class("thread-message-row-unread")

        read_indicator = self.create_message_read_indicator(message)
        container.append(read_indicator)

        sender_label = AppText(
            message.get_display_sender(), class_names="thread-message-sender"
        )
        sender_label.widget.set_size_request(120, -1)
        container.append(sender_label.widget)

        date_label = AppText(
            message.get_display_date(), class_names="thread-message-date"
        )
        date_label.widget.set_size_request(80, -1)
        container.append(date_label.widget)

        if message.get_attachment_count() > 0:
            attachment_icon = AppIcon(
                "mail-attachment-symbolic", class_names="thread-message-attachment-icon"
            )
            attachment_icon.set_pixel_size(12)
            container.append(attachment_icon.widget)

        row.set_child(container)
        self.message_row_instances[row] = message
        return row

    def create_read_indicator(self):
        latest_message = self.thread.latest_message
        if latest_message.is_read:
            indicator = AppIcon(
                "mail-read-symbolic", class_names="thread-row-read-indicator"
            )
        else:
            indicator = AppIcon(
                "mail-unread-symbolic", class_names="thread-row-unread-indicator"
            )
        indicator.set_pixel_size(16)
        return indicator.widget

    def create_message_read_indicator(self, message):
        if message.is_read:
            indicator = AppIcon(
                "mail-read-symbolic", class_names="thread-message-read-indicator"
            )
        else:
            indicator = AppIcon(
                "mail-unread-symbolic", class_names="thread-message-unread-indicator"
            )
        indicator.set_pixel_size(12)
        return indicator.widget

    def on_expand_clicked(self, button):
        self.toggle_expanded()

    def toggle_expanded(self):
        self.is_expanded = not self.is_expanded

        if self.is_expanded:
            self.expand_button.widget.set_icon_name("pan-down-symbolic")
            self.widget.add_css_class("thread-row-expanded")
            self.populate_message_rows()
            self.message_list_container.set_visible(True)
        else:
            self.expand_button.widget.set_icon_name("pan-end-symbolic")
            self.widget.remove_css_class("thread-row-expanded")
            self.message_list_container.set_visible(False)

        if self.expanded_changed_callback:
            self.expanded_changed_callback(self.is_expanded)

    def on_message_row_activated(self, list_box, row):
        if row in self.message_row_instances:
            self.selected_message = self.message_row_instances[row]
            if self.message_selected_callback:
                self.message_selected_callback(self.message_row_instances[row])

    def get_latest_message(self):
        return self.thread.latest_message

    def get_thread(self):
        return self.thread

    def get_selected_message(self):
        if self.selected_message:
            return self.selected_message
        return self.thread.latest_message

    def connect_message_selected(self, callback):
        self.message_selected_callback = callback

    def connect_expanded_changed(self, callback):
        self.expanded_changed_callback = callback

    def set_expanded(self, expanded):
        if self.is_expanded != expanded:
            self.toggle_expanded()

    def mark_thread_read(self):
        for message in self.thread.messages:
            message.is_read = True
        self.update_indicators()

    def update_indicators(self):
        self.header_container.remove(self.read_indicator)
        self.read_indicator = self.create_read_indicator()
        self.header_container.insert_child_after(
            self.read_indicator, self.expand_button.widget
        )

        if self.thread.get_unread_count() == 0:
            self.header_container.remove_css_class("thread-row-unread")

        while True:
            child = self.indicators_container.get_first_child()
            if child is None:
                break
            self.indicators_container.remove(child)

        self.setup_thread_indicators()
