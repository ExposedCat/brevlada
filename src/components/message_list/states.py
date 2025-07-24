from utils.toolkit import Gtk, Adw
from components.ui import AppIcon, AppText

class MessageListStates:
    def __init__(self, widget):
        self.widget = widget
        self.empty_state = self._create_empty_state()
        self.loading_state = self._create_loading_state()
        self.error_state = self._create_error_state()
        self.error_text = None

    def _create_empty_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-empty-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)
        container.set_vexpand(True)  
        container.set_hexpand(True)

        icon = AppIcon("mail-unread-symbolic", class_names="message-list-empty-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)

        text = AppText(
            "No messages in this folder",
            halign=Gtk.Align.CENTER,
            class_names="message-list-empty-text",
        )
        text.set_opacity(0.7)

        container.append(icon.widget)
        container.append(text.widget)

        return container

    def _create_loading_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-loading-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)

        spinner = Gtk.Spinner()
        spinner.add_css_class("message-list-loading-spinner")
        spinner.set_spinning(True)

        text = AppText(
            "Loading messages...",
            halign=Gtk.Align.CENTER,
            class_names="message-list-loading-text",
        )

        container.append(spinner)
        container.append(text.widget)

        return container

    def _create_error_state(self):
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        container.add_css_class("message-list-error-state")
        container.set_halign(Gtk.Align.CENTER)
        container.set_valign(Gtk.Align.CENTER)
        container.set_spacing(12)

        icon = AppIcon("dialog-error-symbolic", class_names="message-list-error-icon")
        icon.set_pixel_size(48)
        icon.set_opacity(0.5)

        self.error_text = AppText(
            "Failed to load messages",
            halign=Gtk.Align.CENTER,
            class_names="message-list-error-text",
        )
        self.error_text.set_opacity(0.7)

        container.append(icon.widget)
        container.append(self.error_text.widget)

        return container

    def show_empty(self):
        self.hide_all()
        self.widget.add(self.empty_state)

    def show_loading(self):
        self.hide_all()
        self.widget.add(self.loading_state)

    def show_error(self, error_message=None):
        self.hide_all()
        self.widget.add(self.error_state)
        if error_message and self.error_text:
            self.error_text.set_text_content(f"Failed to load messages: {error_message}")

    def show_list(self):
        self.hide_all()

    def hide_all(self):
        if self.empty_state.get_parent():
            self.widget.remove(self.empty_state)
        if self.loading_state.get_parent():
            self.widget.remove(self.loading_state)
        if self.error_state.get_parent():
            self.widget.remove(self.error_state) 