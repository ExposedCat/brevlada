from utils.toolkit import Gtk, Pango

class ContentBox(Gtk.Box):
    """A reusable content box component with icon and text"""

    def __init__(self, icon_name=None, text="", spacing=10, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)
        self.add_css_class("content-box")

        self._icon = None
        self._text_label = None

        if icon_name:
            self.set_icon(icon_name)

        if text:
            self.set_text(text)

    def set_icon(self, icon_name):
        """Set or update the icon"""
        if self._icon:
            self.remove(self._icon)

        self._icon = Gtk.Image.new_from_icon_name(icon_name)
        self._icon.add_css_class("content-icon")
        self.prepend(self._icon)

    def set_text(self, text):
        """Set or update the text"""
        if self._text_label:
            self.remove(self._text_label)

        self._text_label = Gtk.Label(label=text)
        self._text_label.set_halign(Gtk.Align.START)
        self._text_label.set_hexpand(True)
        self._text_label.set_ellipsize(Pango.EllipsizeMode.END)
        self._text_label.add_css_class("content-text")
        self.append(self._text_label)

    def get_text(self):
        """Get the current text"""
        return self._text_label.get_text() if self._text_label else ""

class ButtonContainer(Gtk.Box):
    """A container for buttons"""

    def __init__(self, spacing=0, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)
        self.add_css_class("button-container")

class ContentListItem(Gtk.ListBoxRow):
    """A base content item for lists"""

    def __init__(self, item_type="base", **kwargs):
        super().__init__(**kwargs)
        self.add_css_class("content-list-item")

        if item_type != "base":
            self.add_css_class(f"content-list-item-{item_type}")
