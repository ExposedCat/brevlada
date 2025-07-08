import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk

class AppIcon(Gtk.Image):

    def __init__(self, icon_name, **kwargs):
        super().__init__(**kwargs)
        self.set_from_icon_name(icon_name)

    def set_icon(self, icon_name):
        self.set_from_icon_name(icon_name)

class AppText(Gtk.Label):

    def __init__(self, text="", expandable=True, **kwargs):
        super().__init__(label=text, **kwargs)
        self.set_halign(Gtk.Align.START)
        if expandable:
            self.set_hexpand(True)
        self.set_ellipsize(3)  # PANGO_ELLIPSIZE_END

    def set_text_content(self, text):
        self.set_label(text)

    def get_text_content(self):
        return self.get_label()
