from utils.toolkit import Gtk

class AppButton(Gtk.Button):

    def __init__(self, variant="default", expandable=False, class_names=None, children=None, **kwargs):
        super().__init__(**kwargs)

        self.add_css_class("flat")

        if expandable:
            self.set_hexpand(True)

        if variant == "expand":
            pass
        elif variant == "primary":
            pass

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

        if children:
            if isinstance(children, list):
                for child in children:
                    self.set_child(child)
            else:
                self.set_child(children)

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
