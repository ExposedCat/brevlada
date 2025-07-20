from utils.toolkit import Gtk, WebKit


class HtmlViewer:
    def __init__(
        self,
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
        self.widget = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        
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

        self.webview = WebKit.WebView()
        self.webview.set_vexpand(True)
        self.webview.set_hexpand(True)
        
        self.widget.append(self.webview)

    def load_html(self, html_content):
        self.webview.load_html(html_content, "file:///")

    def load_plain_text(self, text_content):
        html_content = f"""
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <style>
                body {{
                    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
                    line-height: 1.6;
                    color: #333;
                    margin: 0;
                    padding: 16px;
                    background-color: transparent;
                }}
                pre {{
                    white-space: pre-wrap;
                    word-wrap: break-word;
                    font-family: inherit;
                }}
            </style>
        </head>
        <body>
            <pre>{text_content}</pre>
        </body>
        </html>
        """
        self.load_html(html_content)

    def get_webview(self):
        return self.webview 