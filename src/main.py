from utils.toolkit import Gtk, Adw, Gio
import os
import logging
import atexit
from components.sidebar import AccountsSidebar
from components.container import ScrollContainer, ContentContainer
from components.ui import AppIcon, AppText
from components.header import (
    UnifiedHeader,
    SidebarHeader,
    ContentHeader,
    MessageListHeader,
)
from components.message_list import MessageList
from components.message_viewer import MessageViewer
from utils.storage import EmailStorage
from utils.mail import cleanup_all_connections
import atexit

from models import Message

class MyWindow(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app)
        self.set_title("Brevlada Email Client")
        self.set_default_size(1400, 900)

        logging.basicConfig(
            level=logging.DEBUG, format="%(asctime)s - %(levelname)s - %(message)s"
        )

        self.storage = EmailStorage()
        self.current_account = None
        self.current_folder = None

        try:
            resource = Gio.resource_load("resources.gresource")
            Gio.resources_register(resource)
            logging.info("Resources loaded successfully")
        except Exception as e:
            logging.error(f"Could not load resources: {e}")

        display = self.get_display()
        if display:
            icon_theme = Gtk.IconTheme.get_for_display(display)
            icon_theme.add_resource_path("/org/gtk/example/icons")

        self.toolbar_view = Adw.ToolbarView()
        self.toolbar_view.set_top_bar_style(Adw.ToolbarStyle.FLAT)

        
        self.sidebar_header = SidebarHeader()
        self.message_list_header = MessageListHeader()
        self.content_header = ContentHeader()

        
        self.sidebar_wrapper = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.sidebar_wrapper.add_css_class("sidebar-wrapper")

        self.message_list_wrapper = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.message_list_wrapper.add_css_class("message-list-wrapper")

        self.content_wrapper = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.content_wrapper.add_css_class("content-wrapper")

        self.main_paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self.main_paned.set_position(300)
        self.main_paned.set_wide_handle(False)
        self.main_paned.set_vexpand(True)
        self.main_paned.set_hexpand(True)
        self.main_paned.connect("notify::position", self.on_main_paned_position_changed)

        self.content_paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self.content_paned.set_position(400)
        self.content_paned.set_wide_handle(False)
        self.content_paned.set_vexpand(True)
        self.content_paned.set_hexpand(True)
        self.content_paned.connect(
            "notify::position", self.on_content_paned_position_changed
        )

        css_provider = Gtk.CssProvider()
        css_file = os.path.join(os.path.dirname(__file__), "style.css")
        css_provider.load_from_path(css_file)
        Gtk.StyleContext.add_provider_for_display(
            self.get_display(), css_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )

        empty_icon = AppIcon("mail-unread-symbolic", class_names="empty-icon")
        empty_icon.set_pixel_size(64)
        empty_icon.set_opacity(0.5)

        empty_label = AppText(
            text="Select an account to view details",
            class_names="empty-label",
            expandable=False,
        )
        empty_label.set_markup(
            "<span size='large'>Select an account to view details</span>"
        )
        empty_label.set_opacity(0.7)

        self.message_viewer = MessageViewer(self.storage, None)
        
        
        self.message_viewer.set_content_header(self.content_header)
        
        self.content_area = ContentContainer(
            spacing=20,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="main-content",
            children=[self.message_viewer.widget],
            with_margin=True,
            h_fill=True,
            w_fill=True,
        )
        self.content_area.widget.set_vexpand(True)
        self.content_area.widget.set_hexpand(True)

        content_scroll = ScrollContainer(
            class_names="content-scroll", children=self.content_area.widget
        )
        content_scroll.widget.set_vexpand(True)
        content_scroll.widget.set_hexpand(True)

        self.message_list = MessageList(self.storage, None)
        self.message_list.connect_message_selected(self.on_message_selected)
        self.message_list.set_header(self.message_list_header)

        
        from components.ui import SearchBox
        self.search_box = SearchBox(placeholder="Search messages", class_names="message-list-search-box")
        self.search_box.connect_search_changed(self.message_list.on_search_changed)

        
        self.message_list_header.connect_refresh(self.on_refresh_requested)

        
        self.message_list_header.message_list = self.message_list
        self.message_list_header.search_box = self.search_box

        
        self.message_list_header.set_enabled(False)

        self.sidebar = AccountsSidebar(class_names="main-sidebar")
        self.sidebar.connect_row_selected(self.on_account_selected)

        
        self.sidebar_wrapper.append(self.sidebar_header.widget)
        self.sidebar_wrapper.append(self.sidebar.widget)

        
        message_list_scroll = Gtk.ScrolledWindow()
        message_list_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        message_list_scroll.set_vexpand(True)
        message_list_scroll.set_hexpand(True)
        message_list_scroll.set_child(self.message_list.widget)

        self.message_list_wrapper.append(self.message_list_header.widget)
        self.message_list_wrapper.append(self.search_box.widget)
        self.message_list_wrapper.append(message_list_scroll)

        self.content_wrapper.append(self.content_header.widget)
        self.content_wrapper.append(content_scroll.widget)

        
        self.main_paned.set_start_child(self.sidebar_wrapper)
        self.main_paned.set_end_child(self.content_paned)
        self.main_paned.set_resize_start_child(True)
        self.main_paned.set_shrink_start_child(False)

        self.content_paned.set_start_child(self.message_list_wrapper)
        self.content_paned.set_end_child(self.content_wrapper)
        self.content_paned.set_resize_start_child(True)
        self.content_paned.set_shrink_start_child(False)

        self.toolbar_view.set_content(self.main_paned)

        self.set_content(self.toolbar_view)

    def on_main_paned_position_changed(self, paned, param):
        pass

    def on_content_paned_position_changed(self, paned, param):
        pass

    def on_account_selected(self, listbox, row):
        if row is None:
            self.message_viewer.show_select_message_state()
            self.message_viewer.widget.set_visible(True)
            self.content_header.window_title.set_subtitle("Select an account")
            self.message_list_header.widget.set_title_widget(
                Gtk.Label(label="Messages")
            )
            self.message_list.set_account_data(None)
            self.message_list.set_folder(None)
            self.message_viewer.set_account_data(None)
            self.message_list_header.set_enabled(False)
            return

        if hasattr(row, "is_folder") and row.is_folder:
            account_data = row.parent_account
            folder_name = row.folder_name
            folder_full_path = getattr(row, "full_path", folder_name)

            self.current_account = account_data
            self.current_folder = folder_full_path

            self.content_header.window_title.set_title(
                f"{account_data['provider']} - {folder_full_path}"
            )
            self.content_header.window_title.set_subtitle(account_data["account_name"])

            self.message_list_header.widget.set_title_widget(
                Gtk.Label(label=folder_full_path)
            )
            self.message_list.set_account_data(account_data)
            self.message_list.set_folder(folder_full_path)
            self.message_viewer.set_account_data(account_data)
            self.message_list_header.set_enabled(True)

            self.message_viewer.show_select_message_state()
            self.message_viewer.widget.set_visible(True)
            return

        if not hasattr(row, "account_data"):
            return

        account_data = row.account_data

        self.current_account = account_data
        self.current_folder = "INBOX"

        self.content_header.window_title.set_title(account_data["provider"])
        self.content_header.window_title.set_subtitle(account_data["account_name"])

        self.message_list_header.widget.set_title_widget(Gtk.Label(label="INBOX"))
        self.message_list.set_account_data(account_data)
        self.message_list.set_folder("INBOX")
        self.message_viewer.set_account_data(account_data)
        self.message_list_header.set_enabled(True)

        self.message_viewer.show_select_message_state()
        self.message_viewer.widget.set_visible(True)

    def on_message_selected(self, message):
        if message:
            self.message_viewer.show_message(message)
            self.message_viewer.widget.set_visible(True)
        else:
            self.message_viewer.show_select_message_state()
            self.message_viewer.widget.set_visible(True)

    def on_refresh_requested(self):
        """Handle refresh request"""
        if self.current_account and self.current_folder:
            logging.info(
                f"Refreshing messages for {self.current_account['email']} - {self.current_folder}"
            )
            self.message_list.refresh_messages()
        

class MyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="org.example.OnlineAccounts")
        self.connect("activate", self.do_activate)

    def do_activate(self):
        self.window = MyWindow(self)
        self.window.present()

if __name__ == "__main__":
    
    atexit.register(cleanup_all_connections)

    app = MyApp()

    def cleanup_on_exit():
        if hasattr(app, "window") and hasattr(app.window, "message_list"):
            app.window.message_list.cleanup()

    atexit.register(cleanup_on_exit)
    app.run(None)
