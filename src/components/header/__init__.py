from utils.toolkit import Gtk, Adw
from components.button import AppButton
from components.ui import LoadingIcon, SearchBox

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
    
    def set_message_subject(self, subject):
        """Update header to show message subject"""
        if subject:
            self.window_title.set_title(subject)
            self.window_title.set_subtitle("Message")
        else:
            self.window_title.set_title("Online Accounts")
            self.window_title.set_subtitle("Select an account")

class MessageListHeader:
    def __init__(self, title="Messages", width=400):
        self.widget = Adw.HeaderBar()
        self.widget.set_title_widget(Gtk.Label(label=title))
        self.widget.set_show_end_title_buttons(False)
        self.widget.set_size_request(width, -1)
        self.widget.add_css_class("message-list-header")

        
        self.search_button = Gtk.ToggleButton()
        self.search_button.set_icon_name("system-search-symbolic")
        self.search_button.add_css_class("flat")
        self.search_button.set_tooltip_text("Search messages")
        self.search_button.connect("toggled", self.on_search_toggled)
        self.widget.pack_end(self.search_button)

        
        self.refresh_button = AppButton()
        self.refresh_button.set_icon_name("view-refresh-symbolic")
        self.refresh_button.widget.set_tooltip_text("Refresh messages")
        self.refresh_button.connect("clicked", self.on_refresh_clicked)
        self.widget.pack_start(self.refresh_button.widget)

        self.refresh_callback = None
        self.search_callback = None
        self.message_list = None  
        self.search_box = None  

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
        
    def on_search_changed(self, search_text):
        if self.search_callback:
            self.search_callback(search_text)
            
    def connect_search(self, callback):
        self.search_callback = callback
        
    def on_search_toggled(self, button):
        """Handle search button toggle"""
        active = button.get_active()
        if self.search_box:
            self.search_box.set_search_mode(active)
            
            
            if active:
                self.search_box.search_entry.grab_focus()
        

