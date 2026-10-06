"""Shared stable/development version handling. Development identities are never reused."""
from pathlib import Path
import re
import secrets

PATTERN = r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-dev\.[0-9a-f]{12})?"

def core(version):
    if re.fullmatch(PATTERN, version) is None:
        raise ValueError("Use MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-dev.<12 hex digits>")
    return version.split("-", 1)[0]

def development(version):
    return core(version) + "-dev." + secrets.token_hex(6)

def write_version(root, version):
    core(version)
    manifest = root / "Cargo.toml"
    old = re.search(r'^version = "([^"]+)"', manifest.read_text(), re.M)[1]
    manifest.write_text(manifest.read_text().replace(f'version = "{old}"', f'version = "{version}"', 1), encoding="utf-8")
    lock = root / "Cargo.lock"
    lock.write_text(re.sub(r'(name = "rolauncher"\s+version = ")[^"]+', lambda m: m[1] + version, lock.read_text()), encoding="utf-8")
    readme = root / "README.md"
    readme.write_text(re.sub(r'^# RoLauncher [^\n]+', '# RoLauncher ' + version, readme.read_text(encoding="utf-8"), count=1), encoding="utf-8")

def stamp(root):
    current = re.search(r'^version = "([^"]+)"', (root / "Cargo.toml").read_text(), re.M)[1]
    version = development(current)
    write_version(root, version)
    return version

if __name__ == "__main__":
    print(stamp(Path(__file__).resolve().parent.parent))
