"""Install official, SHA256-pinned free scanner binaries; never execute archives."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

# Digests read from each upstream GitHub release asset on 2026-10-16.
TOOLS = {
    "linux": [
        ("gitleaks", "gitleaks/gitleaks", "v8.30.1", "gitleaks_8.30.1_linux_x64.tar.gz",
         "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"),
        ("cargo-audit", "rustsec/rustsec", "cargo-audit/v0.22.2", "cargo-audit-x86_64-unknown-linux-musl-v0.22.2.tgz",
         "7fb9497f8594b389e5fce5ef9b92db08432996895b2e0c5a0167a69ed445c428"),
        ("cargo-deny", "EmbarkStudios/cargo-deny", "0.20.2", "cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz",
         "9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"),
    ],
    "windows": [
        ("gitleaks.exe", "gitleaks/gitleaks", "v8.30.1", "gitleaks_8.30.1_windows_x64.zip",
         "d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e"),
        ("cargo-audit.exe", "rustsec/rustsec", "cargo-audit/v0.22.2", "cargo-audit-x86_64-pc-windows-msvc-v0.22.2.zip",
         "0a7316540862c13d954f648917ceacca593747baed6eec180fafa590be2710ab"),
        ("cargo-deny.exe", "EmbarkStudios/cargo-deny", "0.20.2", "cargo-deny-0.20.2-x86_64-pc-windows-msvc.tar.gz",
         "975a22143262fd27476d19ee00c7af67978426e40e1dee94eed6bbade1cf87dc"),
    ],
}
LIMIT = 128 * 1024 * 1024


def install(directory, operating_system, offline):
    directory.mkdir(parents=True, exist_ok=True)
    for executable, repository, tag, filename, expected in TOOLS[operating_system]:
        archive = directory / filename
        if not archive.exists():
            if offline:
                raise ValueError("Missing archive")
            url = f"https://github.com/{repository}/releases/download/{tag}/{filename}"
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read(LIMIT + 1)
            if len(data) > LIMIT:
                raise ValueError("Archive budget exceeded")
            archive.write_bytes(data)
        if archive.stat().st_size > LIMIT or hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
            raise ValueError("Checksum mismatch")
        # Copy only the expected regular executable. Never extract arbitrary archive paths.
        if filename.endswith(".zip"):
            with zipfile.ZipFile(archive) as package:
                entries = [e for e in package.infolist() if Path(e.filename).name == executable and not e.is_dir()]
                if len(entries) != 1 or entries[0].file_size > LIMIT:
                    raise ValueError("Ambiguous executable")
                data = package.read(entries[0])
        else:
            with tarfile.open(archive) as package:
                entries = [e for e in package if Path(e.name).name == executable and e.isfile()]
                if len(entries) != 1 or entries[0].size > LIMIT:
                    raise ValueError("Ambiguous executable")
                with package.extractfile(entries[0]) as source:
                    data = source.read(LIMIT + 1)
        target = directory / executable
        target.write_bytes(data)
        target.chmod(0o755)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--platform", choices=TOOLS, default=platform.system().lower())
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    try:
        if platform.machine().lower() not in ("amd64", "x86_64"):
            raise ValueError("Only x64 is pinned")
        install(args.directory, args.platform, args.offline)
    except (OSError, ValueError, KeyError, tarfile.TarError, zipfile.BadZipFile):
        print(json.dumps({"status": "unverified", "reason": "Scanner installation failed; no raw diagnostics emitted"}))
        return 2
    print(json.dumps({"status": "verified", "tools": [item[0] for item in TOOLS[args.platform]]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
