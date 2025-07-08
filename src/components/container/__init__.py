from utils.toolkit import Gtk

class ButtonContainer(Gtk.Box):

    def __init__(self, spacing=6, class_names=None, children=None, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

        if children:
            if isinstance(children, list):
                for child in children:
                    self.append(child)
            else:
                self.append(children)

class ContentContainer(Gtk.Box):

    def __init__(self, spacing=10, class_names=None, children=None, **kwargs):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=spacing, **kwargs)

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

        if children:
            if isinstance(children, list):
                for child in children:
                    self.append(child)
            else:
                self.append(children)



class NavigationList(Gtk.ListBox):

    def __init__(self, class_names=None, children=None, **kwargs):
        super().__init__(**kwargs)
        self.set_selection_mode(Gtk.SelectionMode.NONE)
        self.add_css_class("navigation-list")

        if class_names:
            if isinstance(class_names, str):
                self.add_css_class(class_names)
            elif isinstance(class_names, list):
                for class_name in class_names:
                    self.add_css_class(class_name)

        if children:
            if isinstance(children, list):
                for child in children:
                    self.append(child)
            else:
                self.append(children)

class ContentItem(Gtk.ListBoxRow):

    def __init__(self, indent=False, class_names=None, children=None, **kwargs):
        super().__init__(**kwargs)
        if indent:
            self.add_css_class("content-item-indented")

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

class ScrollContainer(Gtk.ScrolledWindow):

    def __init__(self, class_names=None, children=None, **kwargs):
        super().__init__(**kwargs)
        self.set_vexpand(True)
        self.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)

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
