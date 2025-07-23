from utils.toolkit import Gtk, WebKit

class HtmlViewer:
    def __init__(
        self,
    ):
        self.widget = Gtk.Frame()
        self.widget.set_hexpand(True)
        self.widget.add_css_class("html-viewer-frame")

        self.webview = WebKit.WebView()
        self.webview.set_hexpand(True)
        self.webview.set_size_request(-1, 50)
        self.webview.add_css_class("html-viewer-view")
        
        self.webview.connect("load-changed", self._on_load_changed)
        
        self.widget.set_child(self.webview)

    def _on_load_changed(self, webview, load_event):
        if load_event == WebKit.LoadEvent.FINISHED:
            self._resize_to_content()

    def _resize_to_content(self):
        script = """
        Math.max(
            document.body.scrollHeight,
            document.body.offsetHeight,
            document.documentElement.clientHeight,
            document.documentElement.scrollHeight,
            document.documentElement.offsetHeight
        );
        """
        self.webview.evaluate_javascript(script, -1, None, None, None, self._on_height_received, None)

    def _on_height_received(self, webview, result, user_data):
        try:
            js_result = self.webview.evaluate_javascript_finish(result)
            if js_result and js_result.is_number():
                content_height = int(js_result.to_int32())
                height = max(100, min(content_height, 800))
                self.webview.set_size_request(-1, height)
        except:
            self.webview.set_size_request(-1, 300)

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
                    padding: 16px 16px 8px 16px;
                    background-color: transparent;
                }}
                pre {{
                    white-space: pre-wrap;
                    word-wrap: break-word;
                    font-family: inherit;
                    margin: 0;
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