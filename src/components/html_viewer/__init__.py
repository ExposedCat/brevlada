from utils.toolkit import Gtk, WebKit

class HtmlViewer:
    def __init__(
        self,
    ):
        self.widget = Gtk.Frame()
        self.widget.set_hexpand(True)
        self.widget.set_vexpand(True)
        self.widget.add_css_class("html-viewer-frame")

        self.webview = WebKit.WebView()
        self.webview.set_vexpand(True)
        self.webview.set_hexpand(True)
        self.webview.add_css_class("html-viewer-view")
        
        self.widget.set_child(self.webview)

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