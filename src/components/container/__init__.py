from utils.toolkit import Gtk

class ButtonContainer:
    def __init__(self, spacing=6, orientation=Gtk.Orientation.HORIZONTAL, halign=None, valign=None, class_names=None, children=None, h_fill=None, w_fill=None,
                 margin=None, margin_top=None, margin_bottom=None, margin_start=None, margin_end=None, **kwargs):
        self.widget = Gtk.Box(orientation=orientation, spacing=spacing, **kwargs)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        # Set margins with fallback to general margin
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

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.append(child)
            else:
                self.widget.append(children)

class ContentContainer:
    def __init__(self, spacing=10, orientation=Gtk.Orientation.HORIZONTAL, halign=None, valign=None, class_names=None, children=None, h_fill=None, w_fill=None,
                 margin=None, margin_top=None, margin_bottom=None, margin_start=None, margin_end=None, **kwargs):
        self.widget = Gtk.Box(orientation=orientation, spacing=spacing, **kwargs)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        # Set margins with fallback to general margin
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

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.append(child)
            else:
                self.widget.append(children)

class NavigationList:
    def __init__(self, selection_mode=Gtk.SelectionMode.NONE, halign=None, valign=None, class_names=None, children=None, h_fill=None, w_fill=None,
                 margin=None, margin_top=None, margin_bottom=None, margin_start=None, margin_end=None, **kwargs):
        self.widget = Gtk.ListBox(**kwargs)
        self.widget.set_selection_mode(selection_mode)
        self.widget.add_css_class("navigation-list")

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        # Set margins with fallback to general margin
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

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.append(child)
            else:
                self.widget.append(children)

class ContentItem:
    def __init__(self, indent=False, halign=None, valign=None, class_names=None, children=None, h_fill=None, w_fill=None,
                 margin=None, margin_top=None, margin_bottom=None, margin_start=None, margin_end=None, **kwargs):
        self.widget = Gtk.ListBoxRow(**kwargs)

        if indent:
            self.widget.add_css_class("content-item-indented")

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        # Set margins with fallback to general margin
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

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.set_child(child)
            else:
                self.widget.set_child(children)


class ScrollContainer:
    def __init__(self, h_policy=Gtk.PolicyType.NEVER, v_policy=Gtk.PolicyType.AUTOMATIC, halign=None, valign=None, class_names=None, children=None, h_fill=None, w_fill=None,
                 margin=None, margin_top=None, margin_bottom=None, margin_start=None, margin_end=None, **kwargs):
        self.widget = Gtk.ScrolledWindow(**kwargs)
        self.widget.set_vexpand(True)
        self.widget.set_policy(h_policy, v_policy)

        if halign is not None:
            self.widget.set_halign(halign)
        if valign is not None:
            self.widget.set_valign(valign)

        if h_fill is not None:
            self.widget.set_hexpand(h_fill)
        if w_fill is not None:
            self.widget.set_vexpand(w_fill)

        # Set margins with fallback to general margin
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

        if children:
            if isinstance(children, list):
                for child in children:
                    self.widget.set_child(child)
            else:
                self.widget.set_child(children)
