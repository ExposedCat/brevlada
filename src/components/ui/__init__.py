from utils.toolkit import Gtk, Pango

class AppIcon(Gtk.Image):

    def __init__(self, icon_name, class_names=None, **kwargs):
        super().__init__(**kwargs)
        self.set_from_icon_name(icon_name)

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

    def set_icon(self, icon_name):
        self.set_from_icon_name(icon_name)

class AppText(Gtk.Label):

    def __init__(self, text="", expandable=True, class_names=None, **kwargs):
        super().__init__(label=text, **kwargs)
        self.set_halign(Gtk.Align.START)
        if expandable:
            self.set_hexpand(True)
        self.set_ellipsize(Pango.EllipsizeMode.END)

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

    def set_text_content(self, text):
        self.set_label(text)

    def get_text_content(self):
        return self.get_label()
