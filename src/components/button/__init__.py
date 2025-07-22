from utils.toolkit import Gtk


class AppButton:
    def __init__(
        self,
        variant="default",
        expandable=False,
        halign=None,
        valign=None,
        class_names=None,
        children=None,
        h_fill=None,
        w_fill=None,
        with_margin=False
    ):
        self.widget = Gtk.Button()

        self.widget.add_css_class("flat")

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if expandable:
            self.widget.set_hexpand(True)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        if with_margin:
            self.widget.add_css_class("button-margin")

        if variant == "expand":
            pass
        elif variant == "primary":
            pass

        if class_names:
            if isinstance(class_names, str):
                self.widget.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.widget.add_css_class(class_name)

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.set_child(child)
            else:
                self.widget.set_child(children)

        self._selected = False

    def set_selected(self, selected):
        if self._selected == selected:
            return

        self._selected = selected
        if selected:
            self.widget.set_state_flags(Gtk.StateFlags.CHECKED, False)
        else:
            self.widget.unset_state_flags(Gtk.StateFlags.CHECKED)

    def get_selected(self):
        return self._selected

    def connect(self, signal, callback, *args):
        self.widget.connect(signal, callback, *args)

    def set_icon_name(self, icon_name):
        self.widget.set_icon_name(icon_name)

    def get_child(self):
        return self.widget.get_child()
