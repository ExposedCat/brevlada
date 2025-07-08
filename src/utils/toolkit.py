import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Gio, Pango  # type: ignore[attr-defined]

__all__ = ["Gtk", "Adw", "GLib", "Gio", "Pango"]
