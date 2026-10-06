"""Run the real WinUI shell against a loopback fixture; never open accounts or external services."""
import json
import os
from pathlib import Path
import subprocess
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = Path(__file__).resolve().parent.parent
accounts = [dict(id=str(i), username=f"sample_{i}", alias=name,
    target=dict(place_id=1818, job_id=None, private_server_link=None), auto_recovery=i != 1,
    status=status, process=dict(pid=12000+i), failures=i, last_error=error)
    for i, (name, status, error) in enumerate([
        ("Main account", "running", None), ("Test account", "backoff", "Waiting for automatic rejoin"),
        ("Build account", "needs_attention", "Session expired; sign in again")])]
commands = []
profiles = []
backups = []
include_beta_updates = False
activity = [dict(timestamp="2026-10-05T09:20:00+07:00", account_id="1", place_id=1818, status="backoff", failures=1, message="Retry scheduled; inspect the next retry time")]

class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def reply(self, body, code=200):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        if self.path == "/v1/status":
            self.reply(dict(accounts=accounts, profiles=profiles, update_repository="", include_beta_updates=include_beta_updates, network_suspended=False, compatibility="UI fixture · real-client validation remains outstanding"))
        elif self.path == "/v1/profiles": self.reply(profiles)
        elif self.path == "/v1/backups": self.reply(backups)
        elif self.path.startswith("/v1/activity"):
            self.reply([a for a in activity if "account_id=" not in self.path or a["account_id"] == self.path.split("account_id=")[-1]])
        elif self.path == "/v1/diagnostics": self.reply(dict(version="1.0.0", activity=activity))
        elif self.path == "/v1/settings/discord":
            self.reply(dict(enabled=False, configured=True, notify_recovery=True, delivery_status="Ready for a test.",
                recent=[dict(timestamp="2026-10-05T09:20:00+07:00", account="Test account (@sample_1)",
                    title="Automatic rejoin scheduled", message="Waiting for the next retry. Your other accounts are still monitored.", place_id=1818)]))
        elif self.path == "/v1/test/commands": self.reply(commands)
        else: self.reply({}, 404)
    def do_POST(self):
        commands.append(self.path)
        data = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        body = json.loads(data) if data else None
        if self.path == "/v1/updates/official/check":
            major, minor, patch = map(int, version.split('.'))
            newer = f"{major}.{minor}.{patch + 1}"
            tag = ("beta-v" if include_beta_updates else "v") + newer
            prefix = f"https://github.com/oangsa/RoLauncher/releases/download/{tag}/rolauncher-v{newer}"
            self.reply(dict(available=True, version=newer,
                prerelease=include_beta_updates, release_url=f"https://github.com/oangsa/RoLauncher/releases/tag/{tag}",
                download_url=prefix + "-windows-x64.zip", checksums_url=prefix + "-SHA256SUMS.txt", installer_url=prefix + "-setup-x64.exe"))
            return
        if self.path == "/v1/profiles":
            selected = [next(a for a in accounts if a["id"] == i) for i in body["account_ids"]]
            entries = [dict(account_id=a["id"], alias=a["alias"], target=a["target"], auto_recovery=a["auto_recovery"], fallback_policy=a.get("fallback_policy", "allow_public"), group=a.get("group", "")) for a in selected]
            profile = dict(id=body.get("id") or str(uuid.uuid4()), name=body["name"], entries=entries)
            profiles[:] = [p for p in profiles if p["id"] != profile["id"]] + [profile]
            self.reply(profile); return
        if self.path.startswith("/v1/profiles/") and self.path.endswith(("/apply", "/start")):
            profile = next(p for p in profiles if p["id"] == self.path.split("/")[-2])
            for entry in profile["entries"]:
                a = next(a for a in accounts if a["id"] == entry["account_id"])
                a.update({k: v for k, v in entry.items() if k != "account_id"})
            self.reply([]); return
        if self.path == "/v1/backups":
            backups.insert(0, "20261005T092000123456789-00000000-0000-0000-0000-000000000000.rolbackup")
            self.reply(backups[0]); return
        if self.path.endswith("/stop"):
            account = next(a for a in accounts if a["id"] == self.path.split("/")[-2])
            account.update(status="stopped", process=None)
        self.reply({}, 202)
    def do_PATCH(self):
        global include_beta_updates
        patch = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        if self.path == "/v1/settings/updates":
            include_beta_updates = patch["include_beta"]
            self.reply({}); return
        if self.path == "/v1/accounts/bulk":
            ids = patch["account_ids"]
            selected = [next((a for a in accounts if a["id"] == i), None) for i in ids]
            values = patch["patch"]
            if any(a is None for a in selected) or values.get("alias") == "REJECT":
                self.reply(dict(error="Simulated atomic bulk save rejection."), 409); return
            for i, a in enumerate(selected):
                expanded = dict(values)
                if "alias" in expanded:
                    expanded["alias"] = expanded["alias"].replace("{username}", a["username"]).replace("{id}", a["id"]).replace("{index}", str(i+1))
                if expanded.pop("clear_target", False): expanded["target"] = None
                a.update(expanded)
            self.reply(selected); return
        account_id = self.path.rsplit("/", 1)[-1]
        account = next((a for a in accounts if a["id"] == account_id), None)
        if account is None: self.reply({}, 404)
        elif patch.get("alias") == "REJECT": self.reply(dict(error="Simulated save rejection."), 409)
        else: account.update(patch); self.reply(account)
    def do_DELETE(self):
        commands.append(self.path)
        if self.path.startswith("/v1/profiles/"):
            profiles[:] = [p for p in profiles if p["id"] != self.path.rsplit("/", 1)[-1]]
            self.reply({}); return
        account = next((a for a in accounts if a["id"] == self.path.rsplit("/", 1)[-1]), None)
        if account is None: self.reply(dict(error="Account not found."), 404)
        elif account["status"] != "stopped": self.reply(dict(error="Stop the account before removing it."), 409)
        else: accounts.remove(account); self.reply({})

server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
output = root / "target" / "ui-preview"
output.mkdir(parents=True, exist_ok=True)
(output / "result.txt").unlink(missing_ok=True)
version = next(line.split('"')[1] for line in (root / "Cargo.toml").read_text().splitlines() if line.startswith("version = "))
config = dict(port=server.server_port, token="TEST_ONLY_" + "x" * 48, version=version, parent_id=os.getpid())
env = os.environ | {"ROLAUNCHER_UI_SMOKE_DIR": str(output)}
exe = root / "target" / "ui-smoke" / "RoLauncher.Desktop.exe"
try:
    process = subprocess.Popen([str(exe)], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    process.communicate((json.dumps(config) + "\n").encode(), timeout=90)
    result = output / "result.txt"
    if not result.exists(): raise RuntimeError(f"WinUI exited without a result (exit {process.returncode}).")
    message = result.read_text()
    print(message)
    if message.startswith("FAILED"): raise RuntimeError("WinUI smoke failed.")
finally:
    if 'process' in globals() and process.poll() is None:
        process.kill()
        process.wait()
    server.shutdown()
