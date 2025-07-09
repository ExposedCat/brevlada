from utils.toolkit import Gtk, Adw, Gio
import os
import logging
from components.sidebar import AccountsSidebar
from components.container import ScrollContainer, ContentContainer
from components.ui import AppIcon, AppText
from components.header import UnifiedHeader, SidebarHeader, ContentHeader

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

        self.unified_header = UnifiedHeader()
        self.sidebar_header = SidebarHeader()
        self.content_header = ContentHeader()

        self.unified_header.widget.append(self.sidebar_header.widget)
        self.unified_header.widget.append(self.content_header.widget)

        self.toolbar_view.add_top_bar(self.unified_header.widget)

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

        self.empty_state = ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER,
            valign=Gtk.Align.CENTER,
            class_names="empty-state",
            children=[empty_icon.widget, empty_label.widget]
        )

        self.account_details = ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="account-details"
        )
        self.account_details.widget.set_visible(False)

        self.content_area = ContentContainer(
            spacing=20,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="main-content",
            children=[self.empty_state.widget, self.account_details.widget],
            margin=30,
            h_fill=True,
            w_fill=True
        )

        content_scroll = ScrollContainer(
            class_names="content-scroll",
            children=self.content_area.widget
        )

        sidebar = AccountsSidebar(class_names="main-sidebar")
        sidebar.connect_row_selected(self.on_account_selected)

        self.paned.set_start_child(sidebar.widget)
        self.paned.set_end_child(content_scroll.widget)
        self.paned.set_resize_start_child(True)
        self.paned.set_shrink_start_child(False)

        self.toolbar_view.set_content(self.paned)

        self.set_content(self.toolbar_view)

    def on_paned_position_changed(self, paned, param):
        position = paned.get_position()
        self.sidebar_header.widget.set_size_request(position, -1)

    def on_account_selected(self, listbox, row):
        if row is None:
            self.empty_state.widget.set_visible(True)
            self.account_details.widget.set_visible(False)
            self.content_header.window_title.set_subtitle("Select an account")
            return

        if hasattr(row, 'is_folder') and row.is_folder:
            account_data = row.parent_account
            folder_name = row.folder_name
            folder_full_path = getattr(row, 'full_path', folder_name)

            self.content_header.window_title.set_title(f"{account_data['provider']} - {folder_full_path}")
            self.content_header.window_title.set_subtitle(account_data["account_name"])

            child = self.account_details.widget.get_first_child()
            while child:
                self.account_details.widget.remove(child)
                child = self.account_details.widget.get_first_child()

            name_label = AppText(
                text=folder_full_path,
                class_names="folder-title",
                margin_bottom=15,
                halign=Gtk.Align.START
            )
            name_label.set_markup(f"<span size='xx-large' weight='bold'>{folder_full_path}</span>")
            self.account_details.widget.append(name_label.widget)

            account_label = AppText(
                text="Account:",
                class_names=["dim-label"],
                halign=Gtk.Align.START
            )
            account_value = AppText(
                text=account_data["account_name"],
                class_names="account-value",
                halign=Gtk.Align.START
            )

            account_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="account-info-row",
                children=[account_label.widget, account_value.widget]
            )
            self.account_details.widget.append(account_container.widget)

            folder_info_label = AppText(
                text="Folder:",
                class_names=["dim-label"],
                halign=Gtk.Align.START
            )
            folder_info_value = AppText(
                text=folder_full_path,
                class_names="folder-value",
                halign=Gtk.Align.START
            )

            folder_info_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="folder-info-row",
                children=[folder_info_label.widget, folder_info_value.widget]
            )
            self.account_details.widget.append(folder_info_container.widget)

            self.empty_state.widget.set_visible(False)
            self.account_details.widget.set_visible(True)
            return

        if not hasattr(row, 'account_data'):
            return

        account_data = row.account_data

        self.content_header.window_title.set_title(account_data["provider"])
        self.content_header.window_title.set_subtitle(account_data["account_name"])

        child = self.account_details.widget.get_first_child()
        while child:
            self.account_details.widget.remove(child)
            child = self.account_details.widget.get_first_child()

        name_label = AppText(
            text=account_data['account_name'],
            class_names="account-title",
            margin_bottom=15,
            halign=Gtk.Align.START
        )
        name_label.set_markup(f"<span size='xx-large' weight='bold'>{account_data['account_name']}</span>")
        self.account_details.widget.append(name_label.widget)

        provider_label = AppText(
            text="Provider:",
            class_names=["dim-label"],
            halign=Gtk.Align.START
        )
        provider_value = AppText(
            text=account_data["provider"],
            class_names="provider-value",
            halign=Gtk.Align.START
        )

        provider_container = ContentContainer(
            spacing=10,
            orientation=Gtk.Orientation.HORIZONTAL,
            class_names="provider-info-row",
            children=[provider_label.widget, provider_value.widget]
        )
        self.account_details.widget.append(provider_container.widget)

        email_label = AppText(
            text="Email:",
            class_names=["dim-label"],
            halign=Gtk.Align.START
        )
        email_value = AppText(
            text=account_data["email"],
            class_names="email-value",
            halign=Gtk.Align.START
        )

        email_container = ContentContainer(
            spacing=10,
            orientation=Gtk.Orientation.HORIZONTAL,
            class_names="email-info-row",
            children=[email_label.widget, email_value.widget]
        )
        self.account_details.widget.append(email_container.widget)

        self.empty_state.widget.set_visible(False)
        self.account_details.widget.set_visible(True)



class MyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="org.example.OnlineAccounts")

    def do_activate(self):
        self.window = MyWindow(self)
        self.window.present()

if __name__ == "__main__":
    app = MyApp()
    app.run(None)
