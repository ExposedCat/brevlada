import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("WebKit", "6.0")
from gi.repository import Gtk, Adw, GLib, Gio, Pango, WebKit  

__all__ = ["Gtk", "Adw", "GLib", "Gio", "Pango", "WebKit"]
