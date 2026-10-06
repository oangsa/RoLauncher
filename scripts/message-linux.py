"""Visible launcher errors for application-menu launches; input stays on a pipe."""
import sys

try:
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk
    if not Gtk.init_check()[0]:
        sys.exit(1)
    dialog = Gtk.MessageDialog(
        message_type=Gtk.MessageType.ERROR,
        buttons=Gtk.ButtonsType.CLOSE,
        text="RoLauncher",
    )
    dialog.set_title("RoLauncher")
    dialog.format_secondary_text(sys.stdin.read(8192))
    dialog.run()
    dialog.destroy()
except (ImportError, ValueError):
    sys.exit(1)
