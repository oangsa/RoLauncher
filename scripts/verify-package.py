"""Verify the current version's release artifacts; optional isolated published-shell smoke."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time
import zipfile
from versioning import core
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = Path(__file__).resolve().parent.parent
version = re.search(r'^version = "(\d+\.\d+\.\d+(?:-dev\.[0-9a-f]{12})?)"', (root / "Cargo.toml").read_text(encoding="utf-8"), re.M)[1]
name = f"rolauncher-v{version}"
dist = root / "dist"
parser = argparse.ArgumentParser()
parser.add_argument("--smoke-shell", action="store_true")
parser.add_argument("--dist-directory", type=Path, default=dist)
args = parser.parse_args()
dist = args.dist_directory.resolve()
lines = (dist / f"{name}-SHA256SUMS.txt").read_text().splitlines()
assert {line.split()[1] for line in lines} == {f"{name}-source.zip", f"{name}-windows-x64.zip", f"{name}-setup-x64.exe"}
for line in lines:
    digest, filename = line.split()
    assert hashlib.sha256((dist / filename).read_bytes()).hexdigest() == digest, filename
with (dist / f"{name}-setup-x64.exe").open("rb") as installer:
    assert installer.read(2) == b"MZ", "Installer must be a Windows executable"
with zipfile.ZipFile(dist / f"{name}-source.zip") as source:
    assert source.testzip() is None
    paths = {p.replace("\\", "/"): p for p in source.namelist()}
    for filename in ["Cargo.toml", "Cargo.lock", "AGENTS.md", "README.md", "src/engine/management.rs",
                     "src/updates.rs", "desktop/MainWindow.Management.cs", "desktop/MainWindow.Workspace.cs",
                     "desktop/MainWindow.xaml", "desktop/MainWindow.xaml.cs", "desktop/BulkDraft.cs",
                     "scripts/sober-isolation/options.json", "desktop.linux/MainWindow.Smoke.cs",
                     f"docs/RELEASE-{core(version)}.md", "docs/VALIDATION.md", "docs/API.md", "docs/USAGE.md",
                     "docs/DEVELOPMENT.md", "docs/PERFORMANCE.md", "desktop/packages.lock.json",
                     "desktop.tests/packages.lock.json", "desktop/RoLauncher.Desktop.csproj",
                     "desktop.tests/RoLauncher.Desktop.Tests.csproj", "src/store.rs", "src/platform.rs",
                     "scripts/verify-package.py", "build.rs", "assets/RoLauncher.ico", "installer/RoLauncher.iss",
                     "scripts/apply-update.ps1", "scripts/build-installer.ps1", ".github/workflows/ci-release.yml",
                     "desktop/MainWindow.Updates.cs", "desktop/UpdateDownloader.cs", "desktop/UpdateHandoff.cs", "CHANGELOG.md"]:
        assert filename in paths, filename
        assert source.read(paths[filename]) == (root / filename).read_bytes(), filename
    for filename in ["desktop.linux/RoLauncher.Linux.csproj", "desktop.linux/packages.lock.json",
                     "desktop.linux/MainWindow.Platform.cs", "desktop/Bootstrap.cs", "desktop/MainWindow.Platform.cs",
                     "src/platform_linux.rs", "src/login_linux.rs", "src/ui_linux.rs", "docs/LINUX.md",
                     "scripts/build-linux.sh", "scripts/package-linux.py", "scripts/login-linux.py",
                     "scripts/message-linux.py", "scripts/apply-update-linux.py", "scripts/install-linux.py",
                     "scripts/test-linux-update.py", "scripts/ui-smoke-linux.py", "scripts/verify-linux-package.py",
                     "scripts/publish-beta.py", "scripts/test-publish-beta.py", ".github/workflows/beta-release.yml", ".github/workflows/linux.yml"]:
        assert filename in paths, filename
        assert source.read(paths[filename]) == (root / filename).read_bytes(), filename
    for file in (root / "desktop.linux").iterdir():
        if file.is_file():
            filename = "desktop.linux/" + file.name
            assert filename in paths, filename
            assert source.read(paths[filename]) == file.read_bytes(), filename
    assert not any("/bin/" in p or "/obj/" in p or ".tools/" in p for p in paths)
    assert f'version = "{version}"'.encode() in source.read(paths["Cargo.toml"])
    assert re.search(r'name = "rolauncher"\s+version = "' + re.escape(version) + '"', source.read(paths["Cargo.lock"]).decode())
    assert source.read(paths["README.md"]).decode().startswith(f"# RoLauncher {version}")
    assert not any("RbxTools.Desktop" in p for p in paths)
with zipfile.ZipFile(dist / f"{name}-windows-x64.zip") as package:
    assert package.testzip() is None
    paths = {p.replace("\\", "/"): p for p in package.namelist()}
    for filename in ["RoLauncher.exe", "WebView2Loader.dll", "AGENTS.md", "README.md", "LICENSE",
                     "THIRD_PARTY_NOTICES.md", "desktop/RoLauncher.Desktop.exe", "desktop/RoLauncher.Desktop.pri",
                     "desktop/CHANGELOG.txt", "desktop/RoLauncher.ico", "desktop/licenses/dependencies.json", "licenses/dependencies.json"]:
        assert f"{name}/{filename}" in paths, filename
    assert package.read(paths[f"{name}/AGENTS.md"]) == (root / "AGENTS.md").read_bytes()
    assert package.read(paths[f"{name}/README.md"]) == (root / "README.md").read_bytes()
    for filename in ["API.md", "USAGE.md", "DEVELOPMENT.md", "PERFORMANCE.md"]:
        assert package.read(paths[f"{name}/docs/{filename}"]) == (root / "docs" / filename).read_bytes()
    assert not any("RbxTools.Desktop" in p or p.endswith("/rbx-tools.exe") for p in paths)
    assert b"RbxTools" not in package.read(paths[f"{name}/desktop/RoLauncher.Desktop.dll"])
    assert "WinUI bridge passed".encode("utf-16le") not in package.read(paths[f"{name}/desktop/RoLauncher.Desktop.dll"])
print("Verified SHA256, intact ZIPs, matching version/source/instructions, runtime and license files, and absence of smoke code/build caches.")

if args.smoke_shell:
    requests = []
    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass
        def respond(self, body):
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        def do_POST(self):
            requests.append(self.path)
            assert self.path == "/v1/updates/official/check", self.path
            self.respond(dict(repository="oangsa/RoLauncher", current_version=version, version=version,
                              available=False, release_url=f"https://github.com/oangsa/RoLauncher/releases/tag/v{version}",
                              download_url="", checksums_url="", installer_url=""))
        def do_GET(self):
            requests.append(self.path)
            if self.path == "/v1/status":
                body = dict(accounts=[], profiles=[], update_repository="", network_suspended=False, compatibility="Isolated release fixture")
            elif self.path == "/v1/settings/discord":
                body = dict(enabled=False, configured=False, notify_recovery=True, delivery_status="Fixture only", recent=[])
            elif self.path in ("/v1/backups", "/v1/activity"):
                body = []
            else:
                body = {}
            self.respond(body)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = dict(port=server.server_port, token="FIXTURE_ONLY_" + "x" * 48, version=version, parent_id=os.getpid())
    process = subprocess.Popen([str(dist / name / "desktop" / "RoLauncher.Desktop.exe")], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        process.stdin.write((json.dumps(config) + "\n").encode())
        process.stdin.close()
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline and requests.count("/v1/status") < 2 and process.poll() is None:
            time.sleep(0.1)
        assert process.poll() is None, f"Published shell exited: {process.returncode}"
        assert requests.count("/v1/status") >= 2 and "/v1/settings/discord" in requests, requests
        assert "/v1/updates/official/check" in requests, "Automatic startup update check did not occur"
        print("Published WinUI shell passed startup/polling against an isolated API fixture; no saved accounts or external services accessed.")
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        server.shutdown()
