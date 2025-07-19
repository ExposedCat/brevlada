from utils.toolkit import Gtk, Pango, Adw


class AppIcon:
    def __init__(
        self,
        icon_name,
        halign=None,
        valign=None,
        class_names=None,
        h_fill=None,
        w_fill=None,
        margin=None,
        margin_top=None,
        margin_bottom=None,
        margin_start=None,
        margin_end=None,
        **kwargs
    ):
        self.widget = Gtk.Image(**kwargs)
        self.widget.set_from_icon_name(icon_name)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        if margin_top is not None:
            self.widget.set_margin_top(margin_top)
        elif margin is not None:
            self.widget.set_margin_top(margin)

        if margin_bottom is not None:
            self.widget.set_margin_bottom(margin_bottom)
        elif margin is not None:
            self.widget.set_margin_bottom(margin)

        if margin_start is not None:
            self.widget.set_margin_start(margin_start)
        elif margin is not None:
            self.widget.set_margin_start(margin)

        if margin_end is not None:
            self.widget.set_margin_end(margin_end)
        elif margin is not None:
            self.widget.set_margin_end(margin)

        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def set_icon(self, icon_name):
        self.widget.set_from_icon_name(icon_name)

    def set_pixel_size(self, size):
        self.widget.set_pixel_size(size)

    def set_opacity(self, opacity):
        self.widget.set_opacity(opacity)


class AppText:
    def __init__(
        self,
        text="",
        expandable=True,
        halign=Gtk.Align.START,
        valign=None,
        class_names=None,
        h_fill=None,
        w_fill=None,
        margin=None,
        margin_top=None,
        margin_bottom=None,
        margin_start=None,
        margin_end=None,
        **kwargs
    ):
        self.widget = Gtk.Label(label=text, **kwargs)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if expandable:
            self.widget.set_hexpand(True)

        self.widget.set_ellipsize(Pango.EllipsizeMode.END)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        if margin_top is not None:
            self.widget.set_margin_top(margin_top)
        elif margin is not None:
            self.widget.set_margin_top(margin)

        if margin_bottom is not None:
            self.widget.set_margin_bottom(margin_bottom)
        elif margin is not None:
            self.widget.set_margin_bottom(margin)

        if margin_start is not None:
            self.widget.set_margin_start(margin_start)
        elif margin is not None:
            self.widget.set_margin_start(margin)

        if margin_end is not None:
            self.widget.set_margin_end(margin_end)
        elif margin is not None:
            self.widget.set_margin_end(margin)

        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def set_text_content(self, text):
        self.widget.set_label(text)

    def get_text_content(self):
        return self.widget.get_label()

    def set_markup(self, markup):
        self.widget.set_markup(markup)

    def set_opacity(self, opacity):
        self.widget.set_opacity(opacity)


class LoadingIcon:
    def __init__(self, size=16, class_names=None):
        self.widget = Gtk.Spinner()
        self.widget.set_size_request(size, size)

    def start(self):
        """Start the loading animation"""
        self.widget.start()

    def stop(self):
        """Stop the loading animation"""
        self.widget.stop()

    def is_spinning(self):
        """Check if the spinner is currently spinning"""
        return self.widget.get_spinning()
