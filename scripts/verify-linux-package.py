"""Check the Linux archive and optionally start its normal UI with a simulated API."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tarfile
import time
import tomllib

root = Path(__file__).resolve().parent.parent
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
name = f"rolauncher-v{version}"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, required=True)
parser.add_argument("--dist-directory", type=Path, default=root / "dist")
parser.add_argument("--smoke-shell", action="store_true")
args = parser.parse_args()
directory = args.directory.resolve(strict=True)
archive = args.dist_directory / f"{name}-linux-x64.tar.gz"
sums = (args.dist_directory / f"{name}-linux-SHA256SUMS.txt").read_text().splitlines()
with archive.open("rb") as source:
    checksum = hashlib.file_digest(source, "sha256").hexdigest()
assert sums == [f"{checksum}  {archive.name}"], "Linux checksum mismatch"
with tarfile.open(archive, "r:gz") as package:
    entries = {entry.name: entry for entry in package}
    for path in ("RoLauncher", "desktop/RoLauncher.Desktop", "desktop/RoLauncher.Desktop.dll",
                 "desktop/CHANGELOG.txt", "desktop/RoLauncher.png", "AGENTS.md", "LICENSE",
                 "README.md", "docs/LINUX.md", "install-linux.py", "licenses/dependencies.json",
                 "desktop/licenses/dependencies.json"):
        entry = entries[f"{name}/{path}"]
        assert entry.isfile(), path
    for path in ("RoLauncher", "desktop/RoLauncher.Desktop"):
        assert entries[f"{name}/{path}"].mode & 0o111, "Executable mode missing"
        assert package.extractfile(f"{name}/{path}").read(4) == b"\x7fELF", "Expected Linux ELF"
    for path in ("README.md", "AGENTS.md", "docs/LINUX.md", "install-linux.py"):
        assert package.extractfile(f"{name}/{path}").read() == (directory / path).read_bytes(), path
    for font in ("Inter-Regular.ttf", "Inter-SemiBold.ttf", "Inter-Bold.ttf", "Inter-Italic.ttf", "Inter-Regular.ttf.manifest"):
        assert package.extractfile(f"{name}/desktop/Assets/Fonts/{font}").read() == (root / "assets/fonts" / font).read_bytes(), font
    assert package.extractfile(f"{name}/licenses/inter-OFL.txt").read() == (root / "licenses/inter-OFL.txt").read_bytes()
    assert not any("/bin/" in path or "/obj/" in path or "/.tools/" in path for path in entries)
    assembly = package.extractfile(f"{name}/desktop/RoLauncher.Desktop.dll").read()
    assert b"LinuxSmokeAsync" not in assembly, "Smoke code must not be shipped"
    assert f"# RoLauncher {version}".encode() in package.extractfile(f"{name}/README.md").read()
print("Verified Linux SHA256, ELF files, executable modes, runtime, instructions and notices; no smoke code.")

if args.smoke_shell:
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    import threading
    token = "isolated-normal-shell-fixture-" + "x" * 40
    requests = []

    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            self.do_GET()

        def do_GET(self):
            if self.headers.get("Authorization") != "Bearer " + token:
                self.send_error(401)
                return
            requests.append(self.path)
            if self.path == "/v1/status":
                body = dict(accounts=[], profiles=[], network_suspended=False, compatibility="Isolated package fixture")
            elif self.path == "/v1/settings/discord":
                body = dict(enabled=False, configured=False, notify_recovery=True, delivery_status="Fixture", recent=[])
            elif self.path in ("/v1/activity", "/v1/backups"):
                body = []
            elif self.path == "/v1/updates/official/check":
                body = dict(available=False, version=version, release_url="https://github.com/oangsa/RoLauncher/releases/tag/v" + version,
                            download_url="", checksums_url="", installer_url="")
            else:
                self.send_error(404)
                return
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    # A fixture supervisor passes bootstrap only through a pipe and exits on command.
    helper = r'''import json,os,subprocess,sys
config=json.loads(sys.stdin.readline())
config["parent_id"]=os.getpid()
child=subprocess.Popen([sys.argv[1]],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
child.stdin.write((json.dumps(config)+"\n").encode())
child.stdin.close()
print(child.pid,flush=True)
sys.stdin.readline()
'''
    supervisor = subprocess.Popen([sys.executable, "-c", helper, str(directory / "desktop/RoLauncher.Desktop")],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    desktop = None
    try:
        supervisor.stdin.write(json.dumps(dict(port=server.server_port, token=token, version=version)) + "\n")
        supervisor.stdin.flush()
        pid = int(supervisor.stdout.readline())
        desktop = os.pidfd_open(pid)
        poll = select.poll()
        poll.register(desktop, select.POLLIN)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and "/v1/status" not in requests:
            assert not poll.poll(0), "Packaged desktop closed unexpectedly"
            time.sleep(0.1)
        assert "/v1/status" in requests, "Normal desktop did not authenticate its API requests"
        time.sleep(2)
        assert not poll.poll(0), "Normal desktop exited during startup"
        supervisor.stdin.write("exit\n")
        supervisor.stdin.flush()
        supervisor.wait(timeout=10)
        assert poll.poll(10000), "Normal desktop did not close after supervisor exit"
        print("Normal packaged Uno shell, pipe bootstrap, authenticated fixture requests and supervisor-exit cleanup passed.")
    finally:
        if supervisor.poll() is None:
            supervisor.kill()
            supervisor.wait()
        if desktop is not None:
            try:
                signal.pidfd_send_signal(desktop, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.close(desktop)
        server.shutdown()
