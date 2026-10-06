"""Register this extracted application in GNOME/KDE's application menu, without root."""
import os
from pathlib import Path

root = Path(__file__).resolve().parent
if not (root / "RoLauncher").is_file():
    raise SystemExit("Run install-linux.py from an extracted Linux package.")
xdg = Path(os.environ.get("XDG_DATA_HOME", ""))
if not xdg.is_absolute():
    xdg = Path.home() / ".local/share"
applications = xdg / "applications"
applications.mkdir(parents=True, exist_ok=True)
# Desktop Entry quoting is different from shell quoting. Escape reserved characters.
executable = str(root / "RoLauncher").replace("\\", "\\\\").replace('"', '\\"').replace("`", "\\`").replace("$", "\\$").replace("%", "%%")
icon = str(root / "desktop/RoLauncher.png")
if any(c in executable + icon for c in "\n\r"):
    raise SystemExit("The application path must not contain line breaks.")
entry = applications / "org.rolauncher.RoLauncher.desktop"
entry.write_text(f'''[Desktop Entry]
Type=Application
Name=RoLauncher
Comment=Your accounts. One workspace.
Exec="{executable}"
Icon={icon}
Terminal=false
Categories=Game;Utility;
StartupWMClass=RoLauncher.Desktop
''')
print("RoLauncher is registered in your application menu. Keep the extracted folder in place.")
