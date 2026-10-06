"""RoLauncher's own ephemeral browser. Its cookie API is the only session source."""
import json
import hashlib
import sys
import threading

try:
    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import GLib, Gtk, WebKit2
except (ImportError, ValueError):
    print(json.dumps({"error": "Browser components unavailable"}), flush=True)
    sys.exit(1)

context = WebKit2.WebContext.new_ephemeral()
browser = WebKit2.WebView.new_with_context(context)
window = Gtk.Window(title="Sign in to Roblox — account saves automatically")
window.set_default_size(1024, 768)
window.add(browser)
window.connect("destroy", Gtk.main_quit)
browser.load_uri("https://www.roblox.com/login")
manager = context.get_cookie_manager()
pending = False
querying = False
attempted_cookie = None


def cookies_ready(source, result, _data):
    global pending, querying, attempted_cookie
    querying = False
    try:
        cookies = source.get_cookies_finish(result)
        for cookie in cookies:
            if cookie.get_name() == ".ROBLOSECURITY" and cookie.get_domain().lstrip(".") == "roblox.com":
                value = cookie.get_value()
                digest = hashlib.sha256(value.encode()).digest() if value else None
                if value and not pending and digest != attempted_cookie:
                    attempted_cookie = digest
                    pending = True
                    print(json.dumps({"cookie": value}), flush=True)
                    break
    except GLib.Error:
        pass


def poll_cookies():
    global querying
    if not pending and not querying:
        querying = True
        manager.get_cookies("https://www.roblox.com", None, cookies_ready, None)
    return True


def show_result(reply):
    global pending
    if reply.get("saved"):
        window.destroy()
    else:
        error = reply.get("error", "Unable to save account.")
        dialog = Gtk.MessageDialog(transient_for=window, modal=True,
                                  message_type=Gtk.MessageType.ERROR,
                                  buttons=Gtk.ButtonsType.CLOSE, text="RoLauncher")
        dialog.format_secondary_text(error)
        dialog.run()
        dialog.destroy()
        if reply.get("close"):
            window.destroy()
        pending = False
    return False


def read_results():
    for line in sys.stdin:
        try:
            GLib.idle_add(show_result, json.loads(line))
        except (ValueError, TypeError):
            pass
    GLib.idle_add(window.destroy)


threading.Thread(target=read_results, daemon=True).start()
GLib.timeout_add_seconds(1, poll_cookies)
window.show_all()
Gtk.main()
