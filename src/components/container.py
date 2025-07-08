import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk

class ButtonContainer(Gtk.Box):

    def __init__(self, spacing=6, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)

class ContentContainer(Gtk.Box):

    def __init__(self, spacing=10, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)

class Sidebar(Gtk.Box):

    def __init__(self, **kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, **kwargs)
        self.add_css_class("sidebar")

class NavigationList(Gtk.ListBox):

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.set_selection_mode(Gtk.SelectionMode.NONE)
        self.add_css_class("navigation-list")

class ContentItem(Gtk.ListBoxRow):

    def __init__(self, indent=False, **kwargs):
        super().__init__(**kwargs)
        if indent:
            self.add_css_class("content-item-indented")

class ScrollContainer(Gtk.ScrolledWindow):

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.set_vexpand(True)
        self.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
