"""Behavioral tests for the read-only security CLI; all credentials are synthetic."""
import json
from contextlib import redirect_stdout
from datetime import datetime, timezone
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from security import evaluate, main

SCRIPT = Path(__file__).with_name("security.py")


class SecurityTests(unittest.TestCase):
    def cli(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), *map(str, args)],
                              capture_output=True, text=True)

    def test_stale_rustsec_data_is_unverified_without_hiding_findings(self):
        now = datetime(2026, 10, 16, 12, tzinfo=timezone.utc)
        report = {"database": {"last-commit": "a" * 40, "advisory-count": 1,
                    "last-updated": "2026-09-15T12:00:00Z"},
                  "vulnerabilities": {"found": True, "count": 1, "list": [{
                    "advisory": {"id": "RUSTSEC-2099-0001"},
                    "package": {"name": "synthetic-package", "version": "1.0.0"}}]}, "warnings": {}}
        result = evaluate("rust", report, 1, now=now)
        self.assertEqual(result["status"], "unverified")
        self.assertEqual(result["findings"][0]["advisory"]["id"], "RUSTSEC-2099-0001")
        self.assertEqual(result["freshness"]["status"], "stale")

    def test_rustsec_freshness_uses_aware_time_and_explicit_age_boundaries(self):
        now = datetime(2026, 10, 16, 12, tzinfo=timezone.utc)
        cases = [
            ("2026-10-16T12:00:00Z", "clean", "fresh"),
            ("2026-10-16T19:00:00+07:00", "clean", "fresh"),
            ("2026-09-16T12:00:00Z", "clean", "fresh"),
            ("2026-09-16T11:59:59Z", "unverified", "stale"),
            ("2026-10-16T12:05:00Z", "clean", "fresh"),
            ("2026-10-16T12:05:01Z", "unverified", "future"),
            ("2099-01-01T00:00:00Z", "unverified", "future"),
            ("2026-10-16T12:00:00", "unverified", "invalid"),
            ("2026-10-16", "unverified", "invalid"),
            ("not-a-date", "unverified", "invalid"),
            ("2026-99-99T00:00:00Z", "unverified", "invalid"),
            (None, "unverified", "invalid"),
        ]
        for timestamp, status, freshness in cases:
            with self.subTest(timestamp=timestamp):
                report = {"database": {"last-commit": "a" * 40, "advisory-count": 1, "last-updated": timestamp},
                          "vulnerabilities": {"found": False, "count": 0, "list": []}, "warnings": {}}
                result = evaluate("rust", report, 0, now=now)
                self.assertEqual(result["status"], status)
                self.assertEqual(result["freshness"]["status"], freshness)
                self.assertEqual(result["freshness"]["assessed_at"], "2026-10-16T12:00:00+00:00")

    def test_invalid_and_future_timestamps_retain_advisories_and_informational_warnings(self):
        now = datetime(2026, 10, 16, 12, tzinfo=timezone.utc)
        advisory = {"advisory": {"id": "RUSTSEC-2099-0001"},
                    "package": {"name": "synthetic-package", "version": "1.0.0"}}
        for timestamp in ("not-a-date", "2099-01-01T00:00:00Z"):
            report = {"database": {"last-commit": "a" * 40, "advisory-count": 1, "last-updated": timestamp},
                      "vulnerabilities": {"found": True, "count": 1, "list": [advisory]},
                      "warnings": {"unmaintained": [advisory]}}
            result = evaluate("rust", report, 1, now=now)
            self.assertEqual(result["status"], "unverified")
            self.assertEqual(result["findings"][0]["advisory"]["id"], "RUSTSEC-2099-0001")
            self.assertEqual(result["warnings"]["unmaintained"][0]["advisory"]["id"], "RUSTSEC-2099-0001")

    def test_stale_rustsec_cli_exits_unverified_and_preserves_findings(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "audit.json"
            report.write_text(json.dumps({"database": {"last-commit": "a" * 40, "advisory-count": 1,
                "last-updated": "1900-01-01T00:00:00Z"}, "warnings": {}, "vulnerabilities": {
                "found": True, "count": 1, "list": [{"advisory": {"id": "RUSTSEC-2099-0001"},
                    "package": {"name": "synthetic-package", "version": "1.0.0"}}]}}))
            result = self.cli("evaluate", "--kind", "rust", "--report", report, "--exit-code", 1)
            self.assertEqual(result.returncode, 2)
            data = json.loads(result.stdout)
            self.assertEqual(data["status"], "unverified")
            self.assertEqual(data["freshness"]["status"], "stale")
            self.assertEqual(data["findings"][0]["advisory"]["id"], "RUSTSEC-2099-0001")

    def test_clean_audit_report_is_explicit(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "audit.json"
            report.write_text(json.dumps({"database": {"last-commit": "a" * 40, "advisory-count": 1, "last-updated": datetime.now(timezone.utc).isoformat()},
                                         "vulnerabilities": {"found": False, "count": 0, "list": []},
                                         "warnings": {}}))
            result = self.cli("evaluate", "--kind", "rust", "--report", report, "--exit-code", 0)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["status"], "clean")

    def test_advisories_are_visible_baseline_not_an_unapproved_merge_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "audit.json"
            report.write_text(json.dumps({"database": {"last-commit": "a" * 40, "advisory-count": 1, "last-updated": datetime.now(timezone.utc).isoformat()},
                "vulnerabilities": {"found": True, "count": 1, "list": [{
                    "advisory": {"id": "RUSTSEC-2099-0001", "cvss": "CVSS:3.1/AV:N"},
                    "package": {"name": "synthetic-package", "version": "1.0.0"},
                    "versions": {"patched": [">=1.0.1"]}}]}, "warnings": {}}))
            result = self.cli("evaluate", "--kind", "rust", "--report", report, "--exit-code", 1)
            self.assertEqual(result.returncode, 0)
            data = json.loads(result.stdout)
            self.assertEqual(data["status"], "findings")
            self.assertEqual(data["findings"][0]["advisory"]["id"], "RUSTSEC-2099-0001")

    def test_missing_database_and_scanner_errors_are_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "audit.json"
            for content, code in [({}, 0), ({"database": {"last-commit": "a" * 40}}, 3),
                                  ({"database": {"last-commit": "a" * 40},
                                    "vulnerabilities": {"found": False, "count": 2, "list": []}}, 0)]:
                with self.subTest(content=content, code=code):
                    report.write_text(json.dumps(content))
                    result = self.cli("evaluate", "--kind", "rust", "--report", report, "--exit-code", code)
                    self.assertEqual(result.returncode, 2)
                    self.assertEqual(json.loads(result.stdout)["status"], "unverified")

    def test_empty_or_malformed_advisory_database_is_not_clean(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "audit.json"
            for database in ({"last-commit": "a" * 40, "advisory-count": 0, "last-updated": "2026-10-14T00:00:00Z"},
                             {"last-commit": "invalid", "advisory-count": 1, "last-updated": "2026-10-14T00:00:00Z"},
                             {"last-commit": "a" * 40}):
                report.write_text(json.dumps({"database": database, "warnings": {},
                    "vulnerabilities": {"count": 0, "found": False, "list": []}}))
                result = self.cli("evaluate", "--kind", "rust", "--report", report, "--exit-code", 0)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(json.loads(result.stdout)["status"], "unverified")

    def test_secret_tool_report_is_allowlisted_and_errors_never_echo_raw_output(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "secrets.json"
            canary = "ghp_" + "SYNTHETIC0123456789" * 2
            report.write_text(json.dumps([{"RuleID": "github-pat", "File": "fixture.txt",
                "StartLine": 3, "Commit": "a" * 40, "Secret": canary,
                "Match": "password=" + canary, "Author": canary}]))
            result = self.cli("evaluate", "--kind", "secrets", "--report", report, "--exit-code", 1)
            self.assertEqual(result.returncode, 1)
            self.assertNotIn(canary, result.stdout + result.stderr)
            self.assertEqual(json.loads(result.stdout)["findings"], [{"rule": "github-pat",
                "path": "fixture.txt", "line": 3, "commit": "a" * 40}])

    def test_nuget_incomplete_and_vulnerable_results_remain_distinct(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "nuget.json"
            report = {"version": 1, "sources": ["https://api.nuget.org/v3/index.json"], "projects": [{
                "path": "fixture.csproj", "frameworks": [{"framework": "net8.0", "transitivePackages": [{
                    "id": "Synthetic.Package", "resolvedVersion": "1.2.3", "vulnerabilities": [{
                        "severity": "High", "advisoryurl": "https://github.com/advisories/GHSA-0000-0000-0000"}]}]}]}]}
            path.write_text(json.dumps(report))
            result = self.cli("evaluate", "--kind", "nuget", "--report", path, "--exit-code", 0)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["status"], "findings")
            for broken in ({"version": 1, "projects": []}, {"version": 1, "projects": [{}], "sources": []},
                           {**report, "problems": [{"severity": "error", "text": "failed advisory query"}]}):
                path.write_text(json.dumps(broken))
                result = self.cli("evaluate", "--kind", "nuget", "--report", path, "--exit-code", 0)
                self.assertEqual(result.returncode, 2)

    def test_missing_scanner_and_process_errors_are_unverified_without_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            for tool in ("rolauncher-intentionally-absent-scanner", sys.executable):
                result = self.cli("audit", "--kind", "rust", "--root", directory, "--tool", tool)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(json.loads(result.stdout), {"findings": [], "status": "unverified"})

    def test_error_report_never_prints_a_secret_or_sensitive_tool_message(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "bad-report.json"
            canary = "SYNTHETIC_PRIVATE_VALUE_" + "not-a-real-credential"
            report.write_text(json.dumps({"error": canary, "Secret": canary}))
            for kind in ("rust", "nuget", "secrets"):
                result = self.cli("evaluate", "--kind", kind, "--report", report, "--exit-code", 3)
                self.assertEqual(result.returncode, 2)
                self.assertNotIn(canary, result.stdout + result.stderr)

    @unittest.skipUnless(os.environ.get("CARGO_AUDIT"), "Set CARGO_AUDIT to the pinned scanner executable")
    def test_real_unreachable_advisory_database_is_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text('version = 4\n[[package]]\nname="fixture"\nversion="0.1.0"\n')
            result = self.cli("audit", "--kind", "rust", "--root", root,
                "--tool", os.environ["CARGO_AUDIT"], "--database", root / "absent-db",
                "--database-url", (root / "absent-upstream").as_uri())
            self.assertEqual(result.returncode, 2)
            self.assertEqual(json.loads(result.stdout)["status"], "unverified")

    def test_tool_download_cache_tampering_is_not_executed(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "gitleaks_8.30.1_linux_x64.tar.gz").write_bytes(b"synthetic tampered archive")
            result = subprocess.run([sys.executable, str(SCRIPT.with_name("security-tools.py")),
                "--directory", directory, "--platform", "linux", "--offline"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("unverified", result.stdout)
            self.assertFalse((Path(directory) / "gitleaks").exists())

    def test_empty_nuget_test_lock_is_not_a_clean_runtime_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "desktop.tests/RoLauncher.Desktop.Tests.csproj"
            project.parent.mkdir()
            project.write_bytes(SCRIPT.parent.parent.joinpath("desktop.tests/RoLauncher.Desktop.Tests.csproj").read_bytes())
            project.with_name("packages.lock.json").write_text(json.dumps({"version": 1, "dependencies": {"net8.0": {}}}))
            result = self.cli("audit", "--kind", "nuget", "--root", root, "--project", "desktop.tests/RoLauncher.Desktop.Tests.csproj",
                              "--tool", "intentionally-absent")
            self.assertEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout)["status"], "not-applicable")
            self.assertIn("SDK/runtime packs are not audited", json.loads(result.stdout)["reason"])

    def test_unreviewed_empty_nuget_graphs_are_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("desktop/RoLauncher.Desktop.csproj", "desktop.linux/RoLauncher.Linux.csproj", "other.csproj"):
                with self.subTest(project=name):
                    project = root / name
                    project.parent.mkdir(parents=True, exist_ok=True)
                    project.write_text('<Project Sdk="Microsoft.NET.Sdk"/>')
                    project.with_name("packages.lock.json").write_text(json.dumps({"version": 1, "dependencies": {"net8.0": {}}}))
                    result = self.cli("audit", "--kind", "nuget", "--root", root, "--project", name,
                                      "--tool", "intentionally-absent")
                    self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                    self.assertEqual(json.loads(result.stdout)["status"], "unverified")

    def test_empty_or_incomplete_production_locks_cannot_use_a_clean_native_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "desktop/RoLauncher.Desktop.csproj"
            project.parent.mkdir()
            project.write_text('<Project Sdk="Microsoft.NET.Sdk"/>')
            native = json.dumps({"version": 1, "sources": ["https://api.nuget.org/v3/index.json"],
                                 "projects": [{"path": str(project)}]}).encode()
            for lock in ({"version": 1, "dependencies": {"net8.0": {}}},
                         {"version": 1, "dependencies": {}}, {"version": 1},
                         {"version": 1, "dependencies": {"net8.0": None}},
                         {"version": 1, "dependencies": []}):
                with self.subTest(lock=lock):
                    project.with_name("packages.lock.json").write_text(json.dumps(lock))
                    output = io.StringIO()
                    with patch.object(sys, "argv", [str(SCRIPT), "audit", "--kind", "nuget", "--root", str(root),
                                    "--project", "desktop/RoLauncher.Desktop.csproj"]), \
                         patch("security.subprocess.run", return_value=subprocess.CompletedProcess([], 0, native, b"")), \
                         redirect_stdout(output):
                        code = main()
                    self.assertEqual(code, 2)
                    self.assertEqual(json.loads(output.getvalue())["status"], "unverified")

    def test_zero_package_exception_rejects_changed_configuration_and_implicit_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "desktop.tests/RoLauncher.Desktop.Tests.csproj"
            project.parent.mkdir()
            reviewed = SCRIPT.parent.parent.joinpath("desktop.tests/RoLauncher.Desktop.Tests.csproj").read_bytes()
            lock = project.with_name("packages.lock.json")
            lock.write_text(json.dumps({"version": 1, "dependencies": {"net8.0": {}}}))
            for location in (root, project.parent):
                for name in ("Directory.Build.props", "Directory.Build.targets", "Directory.Packages.props", "global.json"):
                    with self.subTest(location=location.name, configuration=name):
                        project.write_bytes(reviewed)
                        extra = location / name
                        extra.write_text('<Project><ItemGroup><PackageReference Include="Synthetic.Package" Version="1.0.0"/></ItemGroup></Project>')
                        result = self.cli("audit", "--kind", "nuget", "--root", root,
                            "--project", "desktop.tests/RoLauncher.Desktop.Tests.csproj", "--tool", "intentionally-absent")
                        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                        extra.unlink()
            for content in (reviewed.replace(b"</Project>", b'<ItemGroup><PackageReference Include="Synthetic.Package" Version="1.0.0"/></ItemGroup></Project>'),
                            reviewed.replace(b"</Project>", b'<Import Project="extra.props"/></Project>'),
                            reviewed.replace(b"net8.0", b"net10.0"), b"malformed XML"):
                with self.subTest(configuration=content[-50:]):
                    project.write_bytes(content)
                    result = self.cli("audit", "--kind", "nuget", "--root", root,
                        "--project", "desktop.tests/RoLauncher.Desktop.Tests.csproj", "--tool", "intentionally-absent")
                    self.assertEqual(result.returncode, 2)
            project.write_bytes(reviewed.replace(b"\n", b"\r\n"))
            result = self.cli("audit", "--kind", "nuget", "--root", root,
                "--project", "desktop.tests/RoLauncher.Desktop.Tests.csproj", "--tool", "intentionally-absent")
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            lock.write_text(json.dumps({"version": 1, "dependencies": {"net10.0": {}}}))
            result = self.cli("audit", "--kind", "nuget", "--root", root,
                "--project", "desktop.tests/RoLauncher.Desktop.Tests.csproj", "--tool", "intentionally-absent")
            self.assertEqual(result.returncode, 2)

    def test_workflow_uses_untrusted_pr_boundary_without_writes_secrets_or_artifacts(self):
        # JSON is valid YAML; stdlib parsing keeps the trust contract test dependency-free.
        workflow = json.loads(SCRIPT.parent.parent.joinpath(".github/workflows/security.yml").read_text())
        self.assertEqual(set(workflow["on"]), {"pull_request", "workflow_dispatch"})
        self.assertEqual(workflow["on"]["pull_request"]["branches"], ["dev", "main"])
        self.assertEqual(workflow["permissions"], {"contents": "read"})
        for job in workflow["jobs"].values():
            self.assertNotIn("permissions", job)
            for step in job["steps"]:
                if "uses" in step:
                    self.assertRegex(step["uses"], r"^[a-zA-Z0-9/-]+@[a-f0-9]{40}$")
                    self.assertNotIn("upload", step["uses"])
                if step.get("uses", "").startswith("actions/checkout@"):
                    self.assertFalse(step["with"]["persist-credentials"])
                text = json.dumps(step)
                self.assertNotIn("secrets.", text)
                self.assertNotIn("github.event.pull_request.title", text)
                self.assertNotIn("github.head_ref", text)
        self.assertNotIn("publish-release", json.dumps(workflow))
        self.assertNotIn("release-intents", json.dumps(workflow))

    @unittest.skipUnless(os.environ.get("CARGO_DENY"), "Set CARGO_DENY to the pinned scanner executable")
    def test_real_license_scanner_reports_missing_and_incompatible_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text("pub fn synthetic() {}\n")
            for license, status in [("MIT", "clean"), (None, "findings"), ("GPL-2.0-only", "findings")]:
                manifest = '[package]\nname="synthetic-license"\nversion="0.1.0"\nedition="2021"\n'
                if license:
                    manifest += f'license="{license}"\n'
                (root / "Cargo.toml").write_text(manifest)
                subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, capture_output=True, check=True)
                result = self.cli("audit", "--kind", "licenses", "--root", root,
                                  "--tool", os.environ["CARGO_DENY"])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(json.loads(result.stdout)["status"], status, result.stdout)

    @unittest.skipUnless(os.environ.get("GITLEAKS"), "Set GITLEAKS to the pinned scanner executable")
    def test_real_secret_scanner_clean_canary_false_positive_and_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            def commit():
                subprocess.run(["git", "-C", str(root), "add", "."], check=True)
                subprocess.run(["git", "-C", str(root), "-c", "user.name=Synthetic",
                                "-c", "user.email=synthetic@example.invalid", "commit", "-qm", "fixture"], check=True)
            fixture = root / "fixture.txt"
            fixture.write_text("cookie=$ENV_COOKIE\nhttps://example.invalid/documentation\n")
            commit()
            clean = self.cli("secrets", "--root", root, "--gitleaks", os.environ["GITLEAKS"])
            self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)
            base = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
            canary = "ghp_" + "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56Qr78"
            cookie = "_|WARNING:" + "-DO-NOT-SHARE-THIS." + "SYNTHETIC0123456789" * 3
            launch = "roblox-player:1+launchmode:play+authenticationTicket:" + "SYNTHETIC0123456789" * 3
            fixture.write_text(f"token={canary}\ncookie={cookie}\n{launch}\n")
            commit()
            detected = self.cli("secrets", "--root", root, "--gitleaks", os.environ["GITLEAKS"])
            self.assertEqual(detected.returncode, 1, detected.stdout + detected.stderr)
            for value in (canary, cookie, launch):
                self.assertNotIn(value, detected.stdout + detected.stderr)
            rules = {f["rule"] for f in json.loads(detected.stdout)["findings"]}
            self.assertTrue({"github-pat", "roblox-cookie", "roblox-authenticated-launch"} <= rules)
            fixture.write_text("credential removed\n")
            commit()
            head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
            history = self.cli("secrets", "--root", root, "--base", base, "--head", head,
                               "--gitleaks", os.environ["GITLEAKS"])
            self.assertEqual(history.returncode, 1, history.stdout + history.stderr)
            self.assertNotIn(canary, history.stdout + history.stderr)



if __name__ == "__main__":
    unittest.main()
