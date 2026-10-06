"""Exercise local build identities and stable preparation without network or repository mutations."""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
import tomllib
from versioning import core, development, stamp, write_version

root = Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory() as temp:
    fixture = Path(temp)
    (fixture / "scripts").mkdir()
    (fixture / "docs").mkdir()
    for name in ("Cargo.toml", "Cargo.lock", "README.md", "CHANGELOG.md"):
        shutil.copy2(root / name, fixture / name)
    (fixture / "desktop").mkdir()
    shutil.copy2(root / "desktop/CHANGELOG.txt", fixture / "desktop/CHANGELOG.txt")
    shutil.copy2(root / "docs/RELEASE-1.4.0.md", fixture / "docs/RELEASE-1.4.0.md")
    for name in ("versioning.py", "prepare-release.py"):
        shutil.copy2(root / "scripts" / name, fixture / "scripts" / name)
    first, second = stamp(fixture), stamp(fixture)
    assert first != second and core(first) == core(second) == "1.4.0"
    assert tomllib.loads((fixture / "Cargo.toml").read_text())["package"]["version"] == second
    packages = tomllib.loads((fixture / "Cargo.lock").read_text())["package"]
    assert next(p for p in packages if p["name"] == "rolauncher")["version"] == second
    assert (fixture / "README.md").read_text().startswith("# RoLauncher " + second)
    notes = (fixture / "docs/RELEASE-1.4.0.md").read_bytes()
    command = [sys.executable, str(fixture / "scripts/prepare-release.py"), "feature", "--base", "1.3.0"]
    subprocess.run(command, check=True, capture_output=True)
    subprocess.run(command + ["--stable"], check=True, capture_output=True)
    assert tomllib.loads((fixture / "Cargo.toml").read_text())["package"]["version"] == "1.4.0"
    assert (fixture / "docs/RELEASE-1.4.0.md").read_bytes() == notes
    assert len(list((fixture / "docs").glob("RELEASE-*"))) == 1
    for invalid in ("1.4.0-dev.bad", "1.4.0-dev.123456abcdef-extra", "1.4", "01.4.0"):
        try:
            core(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("Invalid version accepted: " + invalid)
print("Development identity, synchronized metadata, fixed target, stable preparation and consolidated-note checks passed.")
