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
        with_margin=False
    ):
        self.widget = Gtk.Image()
        self.widget.set_from_icon_name(icon_name)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        if with_margin:
            self.widget.add_css_class("app-icon-margin")

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
        with_margin=False,
        margin_top=None,
        margin_bottom=None,
        margin_start=None,
        margin_end=None
    ):
        self.widget = Gtk.Label(label=text)

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

        if with_margin:
            self.widget.add_css_class("app-text-margin")
        if margin_top is not None:
            self.widget.set_margin_top(margin_top)
        if margin_bottom is not None:
            self.widget.set_margin_bottom(margin_bottom)
        if margin_start is not None:
            self.widget.set_margin_start(margin_start)
        if margin_end is not None:
            self.widget.set_margin_end(margin_end)

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

class SearchBox:
    def __init__(self, placeholder="Search messages...", class_names=None):
        self.search_changed_callback = None
        
        
        self.search_bar = Gtk.SearchBar()
        self.search_bar.set_search_mode(False)  
        
        
        self.search_entry = Gtk.SearchEntry()
        self.search_entry.set_placeholder_text(placeholder)
        self.search_entry.set_width_chars(25)
        self.search_entry.set_max_width_chars(40)
        
        
        self.search_bar.set_child(self.search_entry)
        
        
        self.search_entry.connect("search-changed", self._on_search_changed)
        
        
        if class_names:
            if isinstance(class_names, str):
                self.search_bar.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.search_bar.add_css_class(class_name)
        
        
        self.widget = self.search_bar

    def _on_search_changed(self, search_entry):
        """Internal handler for search changed events"""
        if self.search_changed_callback:
            self.search_changed_callback(search_entry.get_text())

    def connect_search_changed(self, callback):
        """Connect a callback for when search text changes"""
        self.search_changed_callback = callback

    def get_search_text(self):
        """Get the current search text"""
        return self.search_entry.get_text()

    def set_search_text(self, text):
        """Set the search text"""
        self.search_entry.set_text(text)

    def clear_search(self):
        """Clear the search text"""
        self.search_entry.set_text("")

    def set_placeholder(self, placeholder):
        """Set the placeholder text"""
        self.search_entry.set_placeholder_text(placeholder)

    def set_search_mode(self, active):
        """Show or hide the search bar"""
        self.search_bar.set_search_mode(active)

    def get_search_mode(self):
        """Get current search mode"""
        return self.search_bar.get_search_mode()

class AppPaned:
    def __init__(self, orientation=Gtk.Orientation.HORIZONTAL, position=None, wide_handle=False, class_names=None):
        self.widget = Gtk.Paned(orientation=orientation)
        
        if position is not None:
            self.widget.set_position(position)
        
        self.widget.set_wide_handle(wide_handle)
        self.widget.set_vexpand(True)
        self.widget.set_hexpand(True)
        
        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def set_start_child(self, child):
        self.widget.set_start_child(child)

    def set_end_child(self, child):
        self.widget.set_end_child(child)

    def set_resize_start_child(self, resize):
        self.widget.set_resize_start_child(resize)

    def set_shrink_start_child(self, shrink):
        self.widget.set_shrink_start_child(shrink)

    def set_position(self, position):
        self.widget.set_position(position)

    def connect_position_changed(self, callback):
        self.widget.connect("notify::position", callback)

class AppBox:
    def __init__(self, orientation=Gtk.Orientation.VERTICAL, spacing=0, homogeneous=False, class_names=None):
        self.widget = Gtk.Box(orientation=orientation)
        self.widget.set_spacing(spacing)
        self.widget.set_homogeneous(homogeneous)
        
        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def append(self, child):
        self.widget.append(child)

    def prepend(self, child):
        self.widget.prepend(child)

    def remove(self, child):
        self.widget.remove(child)

    def set_vexpand(self, expand):
        self.widget.set_vexpand(expand)

    def set_hexpand(self, expand):
        self.widget.set_hexpand(expand)

class AppScrolledWindow:
    def __init__(self, h_policy=Gtk.PolicyType.AUTOMATIC, v_policy=Gtk.PolicyType.AUTOMATIC, class_names=None):
        self.widget = Gtk.ScrolledWindow()
        self.widget.set_policy(h_policy, v_policy)
        
        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def set_child(self, child):
        self.widget.set_child(child)

    def set_vexpand(self, expand):
        self.widget.set_vexpand(expand)

    def set_hexpand(self, expand):
        self.widget.set_hexpand(expand)

class AppLabel:
    def __init__(self, text="", class_names=None):
        self.widget = Gtk.Label(label=text)
        
        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

    def set_label(self, text):
        self.widget.set_label(text)

    def get_label(self):
        return self.widget.get_label()

    def set_markup(self, markup):
        self.widget.set_markup(markup)
