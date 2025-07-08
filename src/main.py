from utils.toolkit import Gtk, Adw, Gio
import os
import logging
from components.sidebar import AccountsSidebar
from components.container import ScrollContainer, ContentContainer
from components.ui import AppIcon, AppText

class MyWindow(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app)
        self.set_title("Online Accounts")
        self.set_default_size(1200, 800)

        logging.basicConfig(level=logging.DEBUG, format='%(asctime)s - %(levelname)s - %(message)s')

        try:
            resource = Gio.resource_load('resources.gresource')
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

        self.unified_header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        self.unified_header.set_size_request(-1, 48)
        self.unified_header.add_css_class("unified-header")

        self.sidebar_header = Adw.HeaderBar()
        self.sidebar_header.set_title_widget(Gtk.Label(label="Accounts"))
        self.sidebar_header.set_show_end_title_buttons(False)
        self.sidebar_header.set_size_request(350, -1)
        self.sidebar_header.add_css_class("sidebar-header")

        self.window_title = Adw.WindowTitle()
        self.window_title.set_title("Online Accounts")
        self.window_title.set_subtitle("Select an account")

        self.content_header = Adw.HeaderBar()
        self.content_header.set_title_widget(self.window_title)
        self.content_header.set_centering_policy(Adw.CenteringPolicy.STRICT)
        self.content_header.set_hexpand(True)
        self.content_header.add_css_class("content-header")

        self.unified_header.append(self.sidebar_header)
        self.unified_header.append(self.content_header)

        self.toolbar_view.add_top_bar(self.unified_header)

        self.paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self.paned.set_position(350)
        self.paned.set_wide_handle(False)
        self.paned.connect("notify::position", self.on_paned_position_changed)

        css_provider = Gtk.CssProvider()
        css_file = os.path.join(os.path.dirname(__file__), 'style.css')
        css_provider.load_from_path(css_file)
        Gtk.StyleContext.add_provider_for_display(
            self.get_display(),
            css_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )



        self.content_area = ContentContainer(
            spacing=20,
            class_names="main-content",
            children=[]
        )
        self.content_area.set_orientation(Gtk.Orientation.VERTICAL)
        self.content_area.set_margin_top(30)
        self.content_area.set_margin_bottom(30)
        self.content_area.set_margin_start(30)
        self.content_area.set_margin_end(30)
        self.content_area.set_vexpand(True)
        self.content_area.set_hexpand(True)

        self.empty_state = ContentContainer(
            spacing=15,
            class_names="empty-state"
        )
        self.empty_state.set_orientation(Gtk.Orientation.VERTICAL)
        self.empty_state.set_halign(Gtk.Align.CENTER)
        self.empty_state.set_valign(Gtk.Align.CENTER)

        empty_icon = AppIcon("mail-unread-symbolic", class_names="empty-icon")
        empty_icon.set_pixel_size(64)
        empty_icon.set_opacity(0.5)

        empty_label = AppText(
            text="Select an account to view details",
            class_names="empty-label",
            expandable=False
        )
        empty_label.set_markup("<span size='large'>Select an account to view details</span>")
        empty_label.set_opacity(0.7)

        self.empty_state.append(empty_icon)
        self.empty_state.append(empty_label)

        self.content_area.append(self.empty_state)

        self.account_details = ContentContainer(
            spacing=15,
            class_names="account-details"
        )
        self.account_details.set_orientation(Gtk.Orientation.VERTICAL)
        self.account_details.set_visible(False)
        self.content_area.append(self.account_details)

        content_scroll = ScrollContainer(
            class_names="content-scroll",
            children=self.content_area
        )

        sidebar = AccountsSidebar(class_names="main-sidebar")
        sidebar.connect_row_selected(self.on_account_selected)

        self.paned.set_start_child(sidebar)
        self.paned.set_end_child(content_scroll)
        self.paned.set_resize_start_child(True)
        self.paned.set_shrink_start_child(False)

        self.toolbar_view.set_content(self.paned)

        self.set_content(self.toolbar_view)

    def on_paned_position_changed(self, paned, param):
        position = paned.get_position()
        self.sidebar_header.set_size_request(position, -1)

    def on_account_selected(self, listbox, row):
        if row is None:
            self.empty_state.set_visible(True)
            self.account_details.set_visible(False)
            self.window_title.set_subtitle("Select an account")
            return

        if hasattr(row, 'is_folder') and row.is_folder:
            account_data = row.parent_account
            folder_name = row.folder_name
            folder_full_path = getattr(row, 'full_path', folder_name)

            self.window_title.set_title(f"{account_data['provider']} - {folder_full_path}")
            self.window_title.set_subtitle(account_data["account_name"])

            child = self.account_details.get_first_child()
            while child:
                self.account_details.remove(child)
                child = self.account_details.get_first_child()

            name_label = AppText(
                text=folder_full_path,
                class_names="folder-title"
            )
            name_label.set_markup(f"<span size='xx-large' weight='bold'>{folder_full_path}</span>")
            name_label.set_halign(Gtk.Align.START)
            name_label.set_margin_bottom(15)
            self.account_details.append(name_label)

            account_container = ContentContainer(
                spacing=10,
                class_names="account-info-row"
            )
            account_container.set_orientation(Gtk.Orientation.HORIZONTAL)
            account_label = AppText(
                text="Account:",
                class_names=["dim-label"]
            )
            account_label.set_halign(Gtk.Align.START)
            account_value = AppText(
                text=account_data["account_name"],
                class_names="account-value"
            )
            account_value.set_halign(Gtk.Align.START)
            account_container.append(account_label)
            account_container.append(account_value)
            self.account_details.append(account_container)

            folder_info_container = ContentContainer(
                spacing=10,
                class_names="folder-info-row"
            )
            folder_info_container.set_orientation(Gtk.Orientation.HORIZONTAL)
            folder_info_label = AppText(
                text="Folder:",
                class_names=["dim-label"]
            )
            folder_info_label.set_halign(Gtk.Align.START)
            folder_info_value = AppText(
                text=folder_full_path,
                class_names="folder-value"
            )
            folder_info_value.set_halign(Gtk.Align.START)
            folder_info_container.append(folder_info_label)
            folder_info_container.append(folder_info_value)
            self.account_details.append(folder_info_container)

            self.empty_state.set_visible(False)
            self.account_details.set_visible(True)
            return

        if not hasattr(row, 'account_data'):
            return

        account_data = row.account_data

        self.window_title.set_title(account_data["provider"])
        self.window_title.set_subtitle(account_data["account_name"])

        child = self.account_details.get_first_child()
        while child:
            self.account_details.remove(child)
            child = self.account_details.get_first_child()

        name_label = AppText(
            text=account_data['account_name'],
            class_names="account-title"
        )
        name_label.set_markup(f"<span size='xx-large' weight='bold'>{account_data['account_name']}</span>")
        name_label.set_halign(Gtk.Align.START)
        name_label.set_margin_bottom(15)
        self.account_details.append(name_label)

        provider_container = ContentContainer(
            spacing=10,
            class_names="provider-info-row"
        )
        provider_container.set_orientation(Gtk.Orientation.HORIZONTAL)
        provider_label = AppText(
            text="Provider:",
            class_names=["dim-label"]
        )
        provider_label.set_halign(Gtk.Align.START)
        provider_value = AppText(
            text=account_data["provider"],
            class_names="provider-value"
        )
        provider_value.set_halign(Gtk.Align.START)
        provider_container.append(provider_label)
        provider_container.append(provider_value)
        self.account_details.append(provider_container)

        email_container = ContentContainer(
            spacing=10,
            class_names="email-info-row"
        )
        email_container.set_orientation(Gtk.Orientation.HORIZONTAL)
        email_label = AppText(
            text="Email:",
            class_names=["dim-label"]
        )
        email_label.set_halign(Gtk.Align.START)
        email_value = AppText(
            text=account_data["email"],
            class_names="email-value"
        )
        email_value.set_halign(Gtk.Align.START)
        email_container.append(email_label)
        email_container.append(email_value)
        self.account_details.append(email_container)

        self.empty_state.set_visible(False)
        self.account_details.set_visible(True)



class MyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="org.example.OnlineAccounts")
        self.connect("activate", self.do_activate)

    def do_activate(self):
        self.window = MyWindow(self)
        self.window.present()

if __name__ == "__main__":
    app = MyApp()
    app.run(None)
