"""Read-only PoC receipt accounting and source checks. No native key operations."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import uuid

COUNTS = ("cleanup_succeeded", "never_persisted", "absent", "intentionally_retained", "incomplete")


def snapshot(data):
    if len(data) > 65536 or not data.endswith(b"\n"):
        raise ValueError("torn or oversized")
    previous = None
    for line in data.splitlines():
        if not line:
            continue
        record = json.loads(line)
        if set(record) != {"schema_version", "key", "owner_sid_reference", "phase", "cleanup", "native_status"}:
            raise ValueError("unknown receipt fields")
        key = record["key"]
        if set(key) != {"name", "provider", "scope", "blob"}:
            raise ValueError("unknown key fields")
        name = key["name"].removeprefix("RoLauncher.IdentityPoC.")
        identifier = uuid.UUID(name)
        if key["name"] != f"RoLauncher.IdentityPoC.{identifier}" or identifier.version != 4:
            raise ValueError("invalid namespace")
        owner = record["owner_sid_reference"]
        if record["schema_version"] != 1 or len(owner) != 32 or not any(owner):
            raise ValueError("invalid owner or schema")
        if key["provider"] not in ("software", "tpm") or key["scope"] not in ("user", "machine"):
            raise ValueError("invalid provider/scope")
        if len(key["blob"]) > 4096 or any(type(b) is not int or not 0 <= b <= 255 for b in key["blob"] + owner):
            raise ValueError("invalid public evidence")
        if record["phase"] not in ("reserved", "created", "finalize_attempted", "finalized", "retained_for_state"):
            raise ValueError("invalid phase")
        if record["cleanup"] not in (None, "succeeded", "never_persisted", "absent", "incomplete"):
            raise ValueError("invalid disposal")
        if record["cleanup"] == "succeeded" and record["native_status"] != 0:
            raise ValueError("deletion lacks native success")
        if record["cleanup"] == "absent" and record["native_status"] not in (0, 0x80090011):
            raise ValueError("absence lacks exact native evidence")
        if previous:
            if any(previous["key"][k] != key[k] for k in ("name", "provider", "scope")) or previous["owner_sid_reference"] != owner:
                raise ValueError("identity changed")
            if previous["key"]["blob"] and previous["key"]["blob"] != key["blob"]:
                raise ValueError("public evidence changed")
        previous = record
    if previous is None:
        raise ValueError("empty receipt")
    return previous


def account(roots):
    counts = dict.fromkeys(COUNTS, 0)
    records = []
    seen = set()
    for root in roots:
        root = Path(root)
        if not root.exists():
            continue
        def unreadable(_error):
            raise OSError("receipt accounting unavailable")
        paths = (Path(directory) / name for directory, _children, files in os.walk(root, onerror=unreadable, followlinks=False)
                 for name in files if name.endswith(".jsonl"))
        for path in paths:
            absolute = str(path.resolve())
            if absolute in seen:
                continue
            seen.add(absolute)
            try:
                if path.is_symlink():
                    raise ValueError("linked receipt requires review")
                data = path.read_bytes()
                records.append({"path": absolute, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
                record = snapshot(data)
                label = {"succeeded": "cleanup_succeeded", "never_persisted": "never_persisted", "absent": "absent"}.get(record["cleanup"])
                if label is None:
                    label = "intentionally_retained" if record["cleanup"] is None and record["phase"] == "retained_for_state" else "incomplete"
            except (OSError, ValueError, KeyError, TypeError, AttributeError):
                label = "incomplete"
            counts[label] += 1
    return {**counts, "receipt_count": sum(counts.values()), "reuse_blocked": bool(counts["incomplete"] or counts["intentionally_retained"]),
            "native_key_operations": False, "historical_overwrite_absence": "not_proven_without_prior_baseline"}, records


def save_baseline(path, records):
    # Strictly local: this manifest contains paths and must never be uploaded.
    with Path(path).open("x", encoding="utf-8") as stream:
        json.dump(records, stream)


def compare_baseline(path):
    records = json.loads(Path(path).read_text(encoding="utf-8"))
    changed = 0
    for record in records:
        try:
            digest = hashlib.sha256(Path(record["path"]).read_bytes()).hexdigest()
            changed += digest != record["sha256"]
        except OSError:
            changed += 1
    return {"baseline_receipts": len(records), "changed_or_missing": changed, "identifiers_omitted": True}


def check_sources(root):
    issues = []
    for path in (root / "src").rglob("*.rs"):
        if "identity_poc" in path.relative_to(root / "src").parts:
            continue
        if "identity_poc" in path.read_text(encoding="utf-8"):
            issues.append("production_identity_import")
    for path in (root / ".github/workflows").glob("*.yml"):
        if re.search(r"lab-(provision|enroll|rights|recover|pin-user)|key-probe\s+\S+\s+machine|measure\s+[AC]\b", path.read_text(encoding="utf-8")):
            issues.append("hosted_privileged_command")
    runner = (root / "scripts/test-identity-poc.ps1").read_text(encoding="utf-8")
    if "foreach ($taskScope in @('user'))" not in runner or "foreach ($taskCandidate in @('B'))" not in runner:
        issues.append("hosted_native_scope_not_user_only")
    installer = (root / "installer/RoLauncher.iss").read_text(encoding="utf-8")
    if "PrivilegesRequired=lowest" not in installer or "IdentityLab" in installer or "identity_poc" in installer:
        issues.append("installer_isolation")
    return sorted(set(issues))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt-root", action="append", type=Path)
    parser.add_argument("--save-local-baseline", type=Path)
    parser.add_argument("--compare-local-baseline", type=Path)
    parser.add_argument("--check-sources", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    roots = args.receipt_root or [root / "target" / name for name in ("identity-poc-recovery", "identity-poc-local", "pr24", "pr24-hardening")]
    if not args.receipt_root and os.environ.get("ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR"):
        roots.append(Path(os.environ["ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR"]))
    report, records = account(roots)
    if args.save_local_baseline:
        save_baseline(args.save_local_baseline, records)
        report["baseline_saved_without_overwrite"] = True
    if args.compare_local_baseline:
        report["baseline_comparison"] = compare_baseline(args.compare_local_baseline)
    if args.check_sources:
        report["source_isolation_issues"] = check_sources(root)
    print(json.dumps(report, indent=2))
    return 2 if report["reuse_blocked"] or report.get("source_isolation_issues") or report.get("baseline_comparison", {}).get("changed_or_missing") else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError):
        print(json.dumps({"preflight": "unavailable_preserve_all_records", "reuse_blocked": True, "identifiers_omitted": True}))
        raise SystemExit(2)
