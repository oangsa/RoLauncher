"""Run the Linux UI against simulated accounts only. Requires an X11 display/Xvfb."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import tomllib

root = Path(__file__).resolve().parent.parent
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
token = "isolated-linux-smoke-token-" + "x" * 40
requests = []
patches = []
commands = []
accounts = [dict(id=str(i), username=name, alias=name.title(), group="Fixture", target=dict(place_id=1),
                 auto_recovery=i == 1, status=status, process=None, failures=0, last_error=None)
            for i, name, status in ((1, "alpha", "running"), (2, "beta", "stopped"), (3, "gamma", "backoff"))]


class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        if self.headers.get("Authorization") != "Bearer " + token:
            self.send_error(401); return
        if self.path not in [f"/v1/accounts/{a['id']}/start" for a in accounts]:
            self.send_error(400); return
        commands.append(self.path)
        self.send_response(202)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", "2")
        self.end_headers(); self.wfile.write(b"{}")

    def do_PATCH(self):
        if self.headers.get("Authorization") != "Bearer " + token:
            self.send_error(401)
            return
        patch = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path != "/v1/accounts/1" or patch != {"alias": "Saved Linux fixture"}:
            self.send_error(400)
            return
        patches.append((self.path, patch))
        accounts[0]["alias"] = patch["alias"]
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"{}")

    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer " + token:
            self.send_error(401)
            return
        requests.append(self.path)
        if self.path == "/v1/status":
            body = dict(accounts=accounts, profiles=[], network_suspended=False, compatibility="Linux smoke fixture")
        elif self.path == "/v1/settings/discord":
            body = dict(enabled=False, configured=False, notify_recovery=True, delivery_status="Fixture", recent=[])
        elif self.path in ("/v1/activity", "/v1/backups"):
            body = []
        else:
            self.send_error(404)
            return
        data = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--shell", type=Path, required=True)
parser.add_argument("--output", type=Path, default=root / "target/linux-ui-preview")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
result = args.output / "result.txt"
if result.exists():
    result.unlink()
server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    env = dict(os.environ, ROLAUNCHER_UI_SMOKE_DIR=str(args.output.resolve()))
    with subprocess.Popen([str(args.shell.resolve())], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, text=True, env=env) as shell:
        bootstrap = json.dumps(dict(port=server.server_port, token=token, version=version, parent_id=os.getpid()))
        try:
            _, errors = shell.communicate(bootstrap + "\n", timeout=60)
        except subprocess.TimeoutExpired:
            shell.kill()
            shell.communicate()
            raise RuntimeError("Linux UI smoke timed out") from None
        if shell.returncode or not result.exists() or not result.read_text().startswith("Linux shell smoke passed"):
            # Only compile/smoke fixture diagnostics; no live credentials exist here.
            (args.output / "runtime.txt").write_text(errors[-20000:])
            if result.exists():
                print(result.read_text(), flush=True)
            if errors:
                print(errors[-20000:], flush=True)
            raise RuntimeError("Linux UI smoke failed; see isolated runtime.txt")
    assert "/v1/status" in requests and "/v1/settings/discord" in requests
    assert patches == [("/v1/accounts/1", {"alias": "Saved Linux fixture"})], "Only the explicit Save may change fixture accounts"
    assert sorted(commands) == ["/v1/accounts/1/start", "/v1/accounts/2/start", "/v1/accounts/3/start"], "Start must reach every selected account"
    print(result.read_text())
finally:
    server.shutdown()
