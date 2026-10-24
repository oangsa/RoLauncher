"""Read-only security evidence. Exit 1: secret finding; exit 2: unverified scan."""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
# Reviewed Microsoft.NET.Sdk/net8.0 project with no PackageReference or custom imports.
# Configuration changes require review, not an automatic extension of this exception.
ZERO_PACKAGE_PROJECT = "desktop.tests/RoLauncher.Desktop.Tests.csproj"
ZERO_PACKAGE_PROJECT_SHA256 = "26eb8bcbca31356d9436a2c4afb31a3a4929b56b9a9806d79a4b83d8d828c19d"
ADVISORY_MAX_AGE = timedelta(days=30)
ADVISORY_FUTURE_TOLERANCE = timedelta(minutes=5)


def run(command, **kwargs):
    # Never echo a scanner's raw output, exception text, or command-line arguments.
    return subprocess.run(list(map(str, command)), capture_output=True, timeout=600, **kwargs)


def secrets(args):
    with tempfile.TemporaryDirectory(prefix="rolauncher-security-") as directory:
        temporary = Path(directory)
        report = temporary / "report.json"
        ignore = temporary / "empty-ignore"
        ignore.touch()
        command = [args.gitleaks]
        if args.directory:
            command += ["dir", args.directory]
        elif args.history or args.base:
            if args.base:
                if not all(re.fullmatch(r"[a-f0-9]{40}", value or "") for value in (args.base, args.head)):
                    raise ValueError("Expected immutable Git SHAs")
                revision = f"{args.base}..{args.head}"
            else:
                revision = "--all"
            command += ["git", args.root, "--log-opts=" + revision]
        else:
            snapshot = temporary / "tracked"
            snapshot.mkdir()
            files = run(["git", "-C", args.root, "ls-files", "-z"])
            if files.returncode:
                raise ValueError("Tracked file enumeration failed")
            for name in files.stdout.decode("utf-8").split("\0"):
                if not name:
                    continue
                source = args.root / name
                if source.is_symlink() or not source.resolve().is_relative_to(args.root.resolve()):
                    raise ValueError("Unsupported tracked symlink")
                destination = snapshot / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
            command += ["dir", snapshot]
        command += ["--config", ROOT / "scripts/security-gitleaks.toml", "--redact=100",
                    "--no-banner", "--no-color", "--ignore-gitleaks-allow",
                    "--gitleaks-ignore-path", ignore, "--report-format", "json",
                    "--report-path", report, "--exit-code", "1", "--max-decode-depth", "2",
                    "--max-archive-depth", "2", "--timeout", "300"]
        completed = run(command)
        result = evaluate("secrets", json.loads(report.read_text(encoding="utf-8-sig")), completed.returncode)
        if not (args.history or args.base or args.directory):
            for finding in result["findings"]:
                finding["path"] = Path(finding["path"]).relative_to(snapshot).as_posix() if Path(finding["path"]).is_absolute() else finding["path"]
        return result


def audit(args):
    if args.kind == "rust":
        command = [args.tool or "cargo-audit", "audit", "--json", "--file", args.root / "Cargo.lock"]
        if args.database:
            command += ["--db", args.database]
        if args.database_url:
            command += ["--url", args.database_url]
        completed = run(command, cwd=args.root)
        return evaluate("rust", json.loads(completed.stdout), completed.returncode)
    if args.kind == "nuget":
        project = (args.root / args.project).resolve(strict=True)
        lock = json.loads(project.with_name("packages.lock.json").read_text())
        dependencies = lock.get("dependencies")
        if (lock.get("version") != 1 or not isinstance(dependencies, dict) or not dependencies
                or not all(isinstance(packages, dict) for packages in dependencies.values())):
            raise ValueError("Incomplete NuGet graph")
        if all(not packages for packages in dependencies.values()):
            if (project.relative_to(args.root.resolve()).as_posix() != ZERO_PACKAGE_PROJECT
                    or hashlib.sha256(project.read_bytes().replace(b"\r\n", b"\n")).hexdigest() != ZERO_PACKAGE_PROJECT_SHA256
                    or lock != {"version": 1, "dependencies": {"net8.0": {}}}):
                raise ValueError("Unreviewed empty NuGet graph")
            for directory in (project.parent, *project.parent.parents):
                if any((directory / name).exists() or (directory / name).is_symlink() for name in (
                        "Directory.Build.props", "Directory.Build.targets", "Directory.Packages.props",
                        "Directory.Packages.targets", "Directory.Build.rsp", "MSBuild.rsp", "global.json")):
                    raise ValueError("Unreviewed implicit project configuration")
            return {"status": "not-applicable", "findings": [],
                    "reason": "Zero locked NuGet packages; SDK/runtime packs are not audited by this result"}
        command = [args.tool or "dotnet", "list", project, "package", "--vulnerable",
                   "--include-transitive", "--format", "json", "--output-version", "1"]
        completed = run(command, cwd=project.parent)
        return evaluate("nuget", json.loads(completed.stdout), completed.returncode)
    command = [args.tool or "cargo-deny", "--format", "json", "--locked", "--offline",
               "--manifest-path", args.root / "Cargo.toml", "--config", ROOT / "scripts/security-deny.toml",
               "check", "licenses", "sources"]
    completed = run(command, cwd=args.root)
    lines = [json.loads(line) for line in completed.stderr.splitlines() if line.strip()]
    summaries = [line["fields"] for line in lines if line.get("type") == "summary"]
    if len(summaries) != 1:
        raise ValueError("License scanner did not complete")
    summary = summaries[0]
    findings = [line for line in lines if line.get("type") == "diagnostic"]
    errors = sum(item["errors"] for item in summary.values())
    expected_code = (4 if summary["licenses"]["errors"] else 0) | (8 if summary["sources"]["errors"] else 0)
    if completed.returncode != expected_code:
        raise ValueError("License scanner failed")
    policy_findings = [item for item in findings if item["fields"].get("code") != "license-not-encountered"]
    return {"status": "findings" if errors or policy_findings else "clean", "summary": summary,
            "findings": [{"code": item["fields"].get("code"), "severity": item["fields"].get("severity"),
                          "message": item["fields"].get("message")} for item in findings]}


def evaluate(kind, report, code, *, now=None):
    if kind == "nuget":
        if (code != 0 or report.get("version") != 1 or not report.get("sources")
                or not report.get("projects") or report.get("problems")):
            return {"status": "unverified", "findings": []}
        findings = []
        for project in report["projects"]:
            if not project.get("path") or project.get("problems"):
                raise ValueError("Incomplete NuGet project")
            # NuGet omits frameworks for projects with zero vulnerable packages.
            for framework in project.get("frameworks", []):
                for kind in ("topLevelPackages", "transitivePackages"):
                    for package in framework.get(kind, []):
                        for vulnerability in package["vulnerabilities"]:
                            findings.append({"package": package["id"], "version": package["resolvedVersion"],
                                "scope": kind, "framework": framework["framework"],
                                "severity": vulnerability["severity"], "advisory": vulnerability["advisoryurl"]})
        return {"status": "findings" if findings else "clean", "findings": findings}
    if kind == "secrets":
        if code not in (0, 1) or not isinstance(report, list) or bool(report) != (code == 1):
            return {"status": "unverified", "findings": []}
        findings = []
        for item in report:
            rule, path = item["RuleID"], item["File"]
            line, commit = item["StartLine"], item.get("Commit", "")
            if (not re.fullmatch(r"[a-z0-9-]{1,80}", rule) or not isinstance(path, str)
                    or not isinstance(line, int) or line < 1
                    or not re.fullmatch(r"(?:[a-f0-9]{40})?", commit)):
                raise ValueError("Invalid location")
            findings.append({"rule": rule, "path": path, "line": line, "commit": commit})
        return {"status": "findings" if findings else "clean", "findings": findings}
    if kind == "rust":
        database = report.get("database", {})
        if (code not in (0, 1) or not re.fullmatch(r"[a-f0-9]{40}", database.get("last-commit", ""))
                or not isinstance(database.get("advisory-count"), int) or database["advisory-count"] < 1):
            return {"status": "unverified", "findings": []}
        settings = report.get("settings", {})
        if settings.get("ignore") or settings.get("severity") or settings.get("target_arch") or settings.get("target_os"):
            raise ValueError("Filtered advisory evidence")
        vulnerabilities = report["vulnerabilities"]
        findings = vulnerabilities["list"]
        if vulnerabilities["count"] != len(findings) or vulnerabilities["found"] != bool(findings):
            raise ValueError("Incomplete vulnerability report")
        def advisory(item):
            return {"advisory": {key: item["advisory"].get(key) for key in
                    ("id", "title", "url", "date", "aliases", "cvss", "informational")},
                    "package": {key: item["package"][key] for key in ("name", "version")},
                    "versions": item.get("versions"), "affected": item.get("affected")}
        result = {"status": "findings" if findings or report.get("warnings") else "clean",
                "database": report["database"], "findings": [advisory(item) for item in findings],
                "warnings": {key: [advisory(item) for item in items] for key, items in report.get("warnings", {}).items()}}
        assessed_at = now if now is not None else datetime.now(timezone.utc)
        freshness = {"status": "invalid", "assessed_at": assessed_at.isoformat(),
                     "max_age_seconds": int(ADVISORY_MAX_AGE.total_seconds()),
                     "future_tolerance_seconds": int(ADVISORY_FUTURE_TOLERANCE.total_seconds())}
        try:
            updated = datetime.fromisoformat(database["last-updated"])
            if updated.utcoffset() is None or assessed_at.utcoffset() is None:
                raise ValueError("Timezone required")
            age = assessed_at - updated
            freshness["age_seconds"] = age.total_seconds()
            freshness["status"] = ("stale" if age > ADVISORY_MAX_AGE else
                                   "future" if age < -ADVISORY_FUTURE_TOLERANCE else "fresh")
        except (KeyError, TypeError, ValueError, OverflowError):
            pass
        result["freshness"] = freshness
        if freshness["status"] != "fresh":
            result["status"] = "unverified"
        return result
    raise ValueError("Unknown report kind")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    policy = commands.add_parser("evaluate")
    policy.add_argument("--kind", choices=["rust", "secrets", "nuget"], required=True)
    policy.add_argument("--report", type=Path, required=True)
    policy.add_argument("--exit-code", type=int, required=True)
    secret = commands.add_parser("secrets")
    secret.add_argument("--root", type=Path, default=ROOT)
    secret.add_argument("--gitleaks", default="gitleaks")
    secret.add_argument("--history", action="store_true")
    secret.add_argument("--base")
    secret.add_argument("--head")
    secret.add_argument("--directory", type=Path, help="Explicit generated package input/output tree, including archives")
    auditing = commands.add_parser("audit")
    auditing.add_argument("--kind", choices=["rust", "nuget", "licenses"], required=True)
    auditing.add_argument("--root", type=Path, default=ROOT)
    auditing.add_argument("--tool")
    auditing.add_argument("--database", type=Path)
    auditing.add_argument("--database-url")
    auditing.add_argument("--project")
    args = parser.parse_args()
    try:
        if args.command == "secrets":
            result = secrets(args)
        elif args.command == "audit":
            result = audit(args)
        else:
            result = evaluate(args.kind, json.loads(args.report.read_text(encoding="utf-8-sig")), args.exit_code)
    except (OSError, ValueError, KeyError, TypeError, IndexError, AttributeError, subprocess.SubprocessError):
        result = {"status": "unverified", "findings": []}
    print(json.dumps(result, sort_keys=True))
    secret_scan = args.command == "secrets" or getattr(args, "kind", None) == "secrets"
    return 2 if result["status"] == "unverified" else int(secret_scan and bool(result["findings"]))


if __name__ == "__main__":
    raise SystemExit(main())
