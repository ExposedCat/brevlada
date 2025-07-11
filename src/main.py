from utils.toolkit import Gtk, Adw, Gio
import os
import logging
from components.sidebar import AccountsSidebar
from components.container import ScrollContainer, ContentContainer
from components.ui import AppIcon, AppText
from components.header import UnifiedHeader, SidebarHeader, ContentHeader, MessageListHeader
from components.message_list import MessageList
from utils.storage import EmailStorage

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

        self.create_sample_messages()

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

        # Create headers
        self.sidebar_header = SidebarHeader()
        self.message_list_header = MessageListHeader()
        self.content_header = ContentHeader()

        # Create wrappers that group headers with their content
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
        self.content_paned.connect("notify::position", self.on_content_paned_position_changed)

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

        self.empty_state = ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER,
            valign=Gtk.Align.CENTER,
            class_names="empty-state",
            children=[empty_icon.widget, empty_label.widget],
        )

        self.account_details = ContentContainer(
            spacing=15,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="account-details",
        )
        self.account_details.widget.set_visible(False)

        self.content_area = ContentContainer(
            spacing=20,
            orientation=Gtk.Orientation.VERTICAL,
            class_names="main-content",
            children=[self.empty_state.widget, self.account_details.widget],
            margin=30,
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

        sidebar = AccountsSidebar(class_names="main-sidebar")
        sidebar.connect_row_selected(self.on_account_selected)

        # Add headers and content to their respective wrappers
        self.sidebar_wrapper.append(self.sidebar_header.widget)
        self.sidebar_wrapper.append(sidebar.widget)

        # Wrap message list in scrolled window for proper scrolling
        message_list_scroll = Gtk.ScrolledWindow()
        message_list_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        message_list_scroll.set_vexpand(True)
        message_list_scroll.set_hexpand(True)
        message_list_scroll.set_child(self.message_list.widget)

        self.message_list_wrapper.append(self.message_list_header.widget)
        self.message_list_wrapper.append(message_list_scroll)

        self.content_wrapper.append(self.content_header.widget)
        self.content_wrapper.append(content_scroll.widget)

        # Set up paned with wrappers
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
            self.empty_state.widget.set_visible(True)
            self.account_details.widget.set_visible(False)
            self.content_header.window_title.set_subtitle("Select an account")
            self.message_list_header.widget.set_title_widget(Gtk.Label(label="Messages"))
            self.message_list.set_account_data(None)
            self.message_list.set_folder(None)
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

            self.message_list_header.widget.set_title_widget(Gtk.Label(label=folder_full_path))
            self.message_list.set_account_data(account_data)
            self.message_list.set_folder(folder_full_path)

            child = self.account_details.widget.get_first_child()
            while child:
                self.account_details.widget.remove(child)
                child = self.account_details.widget.get_first_child()

            name_label = AppText(
                text=folder_full_path,
                class_names="folder-title",
                margin_bottom=15,
                halign=Gtk.Align.START,
            )
            name_label.set_markup(
                f"<span size='xx-large' weight='bold'>{folder_full_path}</span>"
            )
            self.account_details.widget.append(name_label.widget)

            account_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="account-info-row",
                children=[
                    AppText(
                        text="Account:",
                        class_names=["dim-label"],
                        halign=Gtk.Align.START,
                    ).widget,
                    AppText(
                        text=account_data["account_name"],
                        class_names="account-value",
                        halign=Gtk.Align.START,
                    ).widget,
                ],
            )
            self.account_details.widget.append(account_container.widget)

            folder_info_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="folder-info-row",
                children=[
                    AppText(
                        text="Folder:",
                        class_names=["dim-label"],
                        halign=Gtk.Align.START,
                    ).widget,
                    AppText(
                        text=folder_full_path,
                        class_names="folder-value",
                        halign=Gtk.Align.START,
                    ).widget,
                ],
            )
            self.account_details.widget.append(folder_info_container.widget)

            self.empty_state.widget.set_visible(False)
            self.account_details.widget.set_visible(True)
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

        child = self.account_details.widget.get_first_child()
        while child:
            self.account_details.widget.remove(child)
            child = self.account_details.widget.get_first_child()

        name_label = AppText(
            text=account_data["account_name"],
            class_names="account-title",
            margin_bottom=15,
            halign=Gtk.Align.START,
        )
        name_label.set_markup(
            f"<span size='xx-large' weight='bold'>{account_data['account_name']}</span>"
        )
        self.account_details.widget.append(name_label.widget)

        provider_container = ContentContainer(
            spacing=10,
            orientation=Gtk.Orientation.HORIZONTAL,
            class_names="provider-info-row",
            children=[
                AppText(
                    text="Provider:", class_names=["dim-label"], halign=Gtk.Align.START
                ).widget,
                AppText(
                    text=account_data["provider"],
                    class_names="provider-value",
                    halign=Gtk.Align.START,
                ).widget,
            ],
        )
        self.account_details.widget.append(provider_container.widget)

        email_container = ContentContainer(
            spacing=10,
            orientation=Gtk.Orientation.HORIZONTAL,
            class_names="email-info-row",
            children=[
                AppText(
                    text="Email:", class_names=["dim-label"], halign=Gtk.Align.START
                ).widget,
                AppText(
                    text=account_data["email"],
                    class_names="email-value",
                    halign=Gtk.Align.START,
                ).widget,
            ],
        )
        self.account_details.widget.append(email_container.widget)

        self.empty_state.widget.set_visible(False)
        self.account_details.widget.set_visible(True)

    def on_message_selected(self, message):
        if message:
            child = self.account_details.widget.get_first_child()
            while child:
                self.account_details.widget.remove(child)
                child = self.account_details.widget.get_first_child()

            subject_label = AppText(
                text=message.get_display_subject(),
                class_names="message-subject",
                margin_bottom=15,
                halign=Gtk.Align.START,
            )
            subject_label.set_markup(
                f"<span size='xx-large' weight='bold'>{message.get_display_subject()}</span>"
            )
            self.account_details.widget.append(subject_label.widget)

            sender_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="sender-info-row",
                children=[
                    AppText(
                        text="From:",
                        class_names=["dim-label"],
                        halign=Gtk.Align.START,
                    ).widget,
                    AppText(
                        text=message.get_display_sender(),
                        class_names="sender-value",
                        halign=Gtk.Align.START,
                    ).widget,
                ],
            )
            self.account_details.widget.append(sender_container.widget)

            date_container = ContentContainer(
                spacing=10,
                orientation=Gtk.Orientation.HORIZONTAL,
                class_names="date-info-row",
                children=[
                    AppText(
                        text="Date:",
                        class_names=["dim-label"],
                        halign=Gtk.Align.START,
                    ).widget,
                    AppText(
                        text=message.get_display_date(),
                        class_names="date-value",
                        halign=Gtk.Align.START,
                    ).widget,
                ],
            )
            self.account_details.widget.append(date_container.widget)

            self.empty_state.widget.set_visible(False)
            self.account_details.widget.set_visible(True)
        else:
            self.empty_state.widget.set_visible(True)
            self.account_details.widget.set_visible(False)

    def create_sample_messages(self):
        sample_messages = [
            {
                'uid': 1,
                'flags': [],
                'envelope': {
                    'subject': 'Welcome to Brevlada Email Client',
                    'from': [{'name': 'Brevlada Team', 'mailbox': 'team', 'host': 'brevlada.com'}],
                    'to': [{'name': '', 'mailbox': 'test', 'host': 'example.com'}],
                    'cc': [],
                    'bcc': [],
                    'reply_to': [],
                    'date': 'Mon, 01 Jan 2024 14:30:00 +0000',
                    'message_id': '<msg1@example.com>',
                    'in_reply_to': '',
                    'references': ''
                },
                'body': 'Welcome to Brevlada! This is your first email.',
                'headers': '',
                'bodystructure': None
            },
            {
                'uid': 2,
                'flags': ['\\Seen', '\\Flagged'],
                'envelope': {
                    'subject': 'Important: System Update Required',
                    'from': [{'name': 'System Administrator', 'mailbox': 'admin', 'host': 'company.com'}],
                    'to': [{'name': '', 'mailbox': 'test', 'host': 'example.com'}],
                    'cc': [],
                    'bcc': [],
                    'reply_to': [],
                    'date': 'Mon, 01 Jan 2024 11:30:00 +0000',
                    'message_id': '<msg2@example.com>',
                    'in_reply_to': '',
                    'references': ''
                },
                'body': 'Please update your system at your earliest convenience.',
                'headers': '',
                'bodystructure': None
            },
            {
                'uid': 3,
                'flags': [],
                'envelope': {
                    'subject': 'Re: Project Meeting Tomorrow',
                    'from': [{'name': 'John Doe', 'mailbox': 'john.doe', 'host': 'company.com'}],
                    'to': [{'name': '', 'mailbox': 'test', 'host': 'example.com'}],
                    'cc': [],
                    'bcc': [],
                    'reply_to': [],
                    'date': 'Sun, 31 Dec 2023 16:30:00 +0000',
                    'message_id': '<msg3@example.com>',
                    'in_reply_to': '',
                    'references': ''
                },
                'body': 'Thanks for scheduling the meeting. See you tomorrow.',
                'headers': '',
                'bodystructure': None
            },
            {
                'uid': 4,
                'flags': ['\\Seen'],
                'envelope': {
                    'subject': 'Monthly Newsletter - March 2024',
                    'from': [{'name': 'Newsletter Team', 'mailbox': 'newsletter', 'host': 'company.com'}],
                    'to': [{'name': '', 'mailbox': 'test', 'host': 'example.com'}],
                    'cc': [],
                    'bcc': [],
                    'reply_to': [],
                    'date': 'Sat, 30 Dec 2023 16:30:00 +0000',
                    'message_id': '<msg4@example.com>',
                    'in_reply_to': '',
                    'references': ''
                },
                'body': 'Check out this months updates and news.',
                'headers': '',
                'bodystructure': None
            },
            {
                'uid': 5,
                'flags': ['\\Flagged'],
                'envelope': {
                    'subject': 'Urgent: Server Maintenance Window',
                    'from': [{'name': 'IT Support', 'mailbox': 'support', 'host': 'company.com'}],
                    'to': [{'name': '', 'mailbox': 'test', 'host': 'example.com'}],
                    'cc': [],
                    'bcc': [],
                    'reply_to': [],
                    'date': 'Fri, 29 Dec 2023 16:30:00 +0000',
                    'message_id': '<msg5@example.com>',
                    'in_reply_to': '',
                    'references': ''
                },
                'body': 'Scheduled maintenance will occur tonight from 2-4 AM.',
                'headers': '',
                'bodystructure': None
            }
        ]

        try:
            for msg_data in sample_messages:
                message = Message(msg_data)
                self.storage.store_message(message, 'INBOX', 'test@example.com')
            logging.info("Sample messages created successfully")
        except Exception as e:
            logging.error(f"Error creating sample messages: {e}")


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
