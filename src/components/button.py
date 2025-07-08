import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk

class AppButton(Gtk.Button):

    def __init__(self, variant="default", expandable=False, **kwargs):
        super().__init__(**kwargs)

        self.add_css_class("flat")

        if expandable:
            self.set_hexpand(True)

        if variant == "expand":
            pass
        elif variant == "primary":
            pass

        self._selected = False

    def set_selected(self, selected):
        if self._selected == selected:
            return

        self._selected = selected
        if selected:
            self.set_state_flags(Gtk.StateFlags.CHECKED, False)
        else:
            self.unset_state_flags(Gtk.StateFlags.CHECKED)

    def get_selected(self):
        return self._selected
