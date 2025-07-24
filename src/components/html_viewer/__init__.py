from utils.toolkit import Gtk, WebKit
from utils.message_parser import detect_and_process_embedded_replies
import html

class HtmlViewer:
    def __init__(self):
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
                height = max(50, content_height)
                self.webview.set_size_request(-1, height)
        except:
            self.webview.set_size_request(-1, 100)

    def load_html(self, html_content):
        processed_content = detect_and_process_embedded_replies(html_content, is_html=True)
        full_html = self._wrap_with_styles_and_scripts(processed_content)
        self.webview.load_html(full_html, "file:///")

    def load_plain_text(self, text_content):
        processed_content = detect_and_process_embedded_replies(text_content, is_html=False)
        html_content = f"""
        <body>
            {processed_content}
        </body>
        """
        full_html = self._wrap_with_styles_and_scripts(html_content)
        self.webview.load_html(full_html, "file:///")

    def _wrap_with_styles_and_scripts(self, body_content):
        """Wrap content with necessary styles and scripts for embedded reply functionality"""
        return f"""
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
                
                .embedded-reply-container {{
                    margin: 12px 0;
                    border: 1px solid #e0e0e0;
                    border-radius: 6px;
                    background-color: #f8f9fa;
                }}
                
                .embedded-reply-toggle {{
                    padding: 8px 12px;
                    cursor: pointer;
                    display: flex;
                    align-items: center;
                    background-color: #f0f0f0;
                    border-radius: 6px 6px 0 0;
                    user-select: none;
                    transition: background-color 0.2s ease;
                }}
                
                .embedded-reply-toggle:hover {{
                    background-color: #e8e8e8;
                }}
                
                .toggle-icon {{
                    font-size: 12px;
                    margin-right: 8px;
                    transition: transform 0.2s ease;
                    display: inline-block;
                    width: 12px;
                }}
                
                .toggle-icon.expanded {{
                    transform: rotate(90deg);
                }}
                
                .reply-summary {{
                    flex: 1;
                    font-size: 14px;
                    color: #666;
                    font-style: italic;
                }}
                
                .embedded-reply-content {{
                    padding: 12px;
                    border-top: 1px solid #e0e0e0;
                    background-color: #fff;
                    border-radius: 0 0 6px 6px;
                }}
                
                .embedded-reply-content pre {{
                    color: #666;
                    font-size: 13px;
                    margin: 0;
                    white-space: pre-wrap;
                    word-wrap: break-word;
                }}
                
                .embedded-reply-content blockquote {{
                    margin: 0;
                    padding-left: 12px;
                    border-left: 3px solid #ccc;
                    color: #666;
                }}
            </style>
            <script>
                function toggleEmbeddedReply(toggleElement) {{
                    const container = toggleElement.parentElement;
                    const content = container.querySelector('.embedded-reply-content');
                    const icon = toggleElement.querySelector('.toggle-icon');
                    
                    if (content.style.display === 'none') {{
                        content.style.display = 'block';
                        icon.classList.add('expanded');
                    }} else {{
                        content.style.display = 'none';
                        icon.classList.remove('expanded');
                    }}
                    
                    // Trigger resize to recalculate height
                    setTimeout(() => {{
                        window.dispatchEvent(new Event('resize'));
                    }}, 100);
                }}
            </script>
        </head>
        {body_content}
        </html>
        """

    def get_webview(self):
        return self.webview 