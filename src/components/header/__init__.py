from utils.toolkit import Gtk, Adw
from components.button import AppButton
from components.ui import LoadingIcon


class UnifiedHeader:
    def __init__(self):
        self.widget = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.widget.set_size_request(-1, 48)
        self.widget.add_css_class("unified-header")


class SidebarHeader:
    def __init__(self, title="Accounts", width=350):
        self.widget = Adw.HeaderBar()
        self.widget.set_title_widget(Gtk.Label(label=title))
        self.widget.set_show_end_title_buttons(False)
        self.widget.set_size_request(width, -1)
        self.widget.add_css_class("sidebar-header")


class ContentHeader:
    def __init__(self, title="Online Accounts", subtitle="Select an account"):
        self.widget = Adw.HeaderBar()

        self.window_title = Adw.WindowTitle()
        self.window_title.set_title(title)
        self.window_title.set_subtitle(subtitle)

        self.widget.set_title_widget(self.window_title)
        self.widget.set_centering_policy(Adw.CenteringPolicy.STRICT)
        self.widget.set_hexpand(True)
        self.widget.add_css_class("content-header")


class MessageListHeader:
    def __init__(self, title="Messages", width=400):
        self.widget = Adw.HeaderBar()
        self.widget.set_title_widget(Gtk.Label(label=title))
        self.widget.set_show_end_title_buttons(False)
        self.widget.set_size_request(width, -1)
        self.widget.add_css_class("message-list-header")

        # Add refresh button
        self.refresh_button = AppButton()
        self.refresh_button.set_icon_name("view-refresh-symbolic")
        self.refresh_button.widget.set_tooltip_text("Refresh messages")
        self.refresh_button.connect("clicked", self.on_refresh_clicked)
        self.widget.pack_end(self.refresh_button.widget)

        self.refresh_callback = None

    def on_refresh_clicked(self, button):
        if self.refresh_callback:
            self.refresh_callback()

    def connect_refresh(self, callback):
        self.refresh_callback = callback

    def set_refreshing(self, refreshing):
        if refreshing:
            loading_icon = LoadingIcon(size=16)
            loading_icon.start()
            self.refresh_button.widget.set_child(loading_icon.widget)
        else:
            self.refresh_button.widget.set_child(None)
            self.refresh_button.set_icon_name("view-refresh-symbolic")

    def set_loading(self, loading, text="Loading..."):
        self.set_refreshing(loading)

    def set_enabled(self, enabled):
        self.refresh_button.widget.set_sensitive(enabled)
