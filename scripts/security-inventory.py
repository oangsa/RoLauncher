"""Reproduce a lockfile inventory without restores, builds, or absolute cache paths."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
TARGETS = ("x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def reachable(graph, runtime):
    nodes = {n["id"]: n for n in graph["resolve"]["nodes"]}
    packages = {p["id"]: p for p in graph["packages"]}
    seen, todo = set(), [graph["resolve"]["root"]]
    while todo:
        identity = todo.pop()
        if identity in seen:
            continue
        seen.add(identity)
        for dependency in nodes[identity]["deps"]:
            if runtime:
                if not any(k["kind"] is None for k in dependency["dep_kinds"]):
                    continue
                if any("proc-macro" in t["kind"] for t in packages[dependency["pkg"]]["targets"]):
                    continue
            todo.append(dependency["pkg"])
    return seen


def inventory(caches, rust_audit, nuget_audits):
    graphs = {}
    for target in (*TARGETS, "all"):
        command = ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"]
        if target != "all":
            command += ["--filter-platform", target]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=180)
        if result.returncode:
            raise ValueError("Locked cargo metadata unavailable")
        graphs[target] = json.loads(result.stdout)
    exposures = {t: (reachable(g, False), reachable(g, True)) for t, g in graphs.items() if t != "all"}
    graph = graphs["all"]
    root = next(p for p in graph["packages"] if p["id"] == graph["resolve"]["root"])
    direct = {d["pkg"] for n in graph["resolve"]["nodes"] if n["id"] == root["id"] for d in n["deps"]}
    rust = []
    for package in sorted(graph["packages"], key=lambda p: (p["name"], p["version"])):
        directory = Path(package["manifest_path"]).parent
        notices = [{"file": p.name, "sha256": digest(p)} for p in sorted(directory.iterdir())
                   if p.is_file() and p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE"))]
        rust.append({"name": package["name"], "version": package["version"],
            "source": package["source"] or "workspace", "license": package["license"],
            "license_file": Path(package["license_file"]).name if package["license_file"] else None,
            "repository": package["repository"],
            "direct": package["id"] in direct, "notice_evidence": notices,
            "advisories": [item["advisory"]["id"] for item in rust_audit.get("findings", [])
                           if item["package"] == {"name": package["name"], "version": package["version"]}],
            "advisory_status": ("unverified" if rust_audit.get("status") not in ("clean", "findings") else
                "known-advisory" if any(item["package"] == {"name": package["name"], "version": package["version"]}
                                        for item in rust_audit.get("findings", [])) else "no-known-advisory"),
            "exposure": {target: ("runtime-candidate" if package["id"] in runtime else "build-or-test")
                         for target, (all_ids, runtime) in exposures.items() if package["id"] in all_ids},
            "dependencies": [{"name": d["name"], "kind": d["kind"] or "normal", "target": d["target"], "requirement": d["req"]}
                             for d in package["dependencies"]]})
    nuget = []
    for project in ("desktop", "desktop.linux", "desktop.tests"):
        lock = json.loads((ROOT / project / "packages.lock.json").read_text())
        for framework, packages in lock["dependencies"].items():
            for name, package in sorted(packages.items()):
                version = package["resolved"]
                directory = next((c / name.lower() / version.lower() for c in caches
                                  if (c / name.lower() / version.lower()).is_dir()), None)
                spec = next(directory.glob("*.nuspec"), None) if directory else None
                license, upstream, evidence = None, None, []
                if spec:
                    xml = ET.parse(spec).getroot()
                    metadata = next(e for e in xml if e.tag.split("}")[-1] == "metadata")
                    fields = {e.tag.split("}")[-1]: e for e in metadata}
                    if "license" in fields:
                        node = fields["license"]
                        license = {"type": node.get("type"), "value": node.text}
                    elif "licenseUrl" in fields:
                        license = {"type": "legacy-url-unverified", "value": fields["licenseUrl"].text}
                    if "repository" in fields:
                        upstream = fields["repository"].attrib
                    for notice in sorted(directory.rglob("*")):
                        if notice.is_file() and (notice.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "THIRDPARTY"))
                            or (license and license["type"] == "file" and notice == directory / license["value"])):
                            evidence.append({"file": notice.relative_to(directory).as_posix(), "sha256": digest(notice)})
                assets = json.loads((ROOT / project / "obj/project.assets.json").read_text()) if (ROOT / project / "obj/project.assets.json").is_file() else {}
                targets = [details for group in assets.get("targets", {}).values()
                           for identity, details in group.items() if identity.lower() == f"{name}/{version}".lower()]
                asset_kinds = sorted({kind for details in targets for kind in details if kind != "dependencies"})
                nuget.append({"project": project, "framework": framework, "name": name, "version": version,
                    "direct": package["type"] == "Direct", "content_hash": package.get("contentHash"),
                    "dependencies": package.get("dependencies", {}), "license": license,
                    "repository": upstream, "notice_evidence": evidence, "asset_kinds": asset_kinds,
                    "advisory_status": ("no-known-advisory" if nuget_audits.get(project, {}).get("status") == "clean"
                        else "known-advisory" if any(item.get("package", "").lower() == name.lower() and item.get("version") == version
                            for item in nuget_audits.get(project, {}).get("findings", [])) else "unverified"),
                    "scope": "test-only" if project == "desktop.tests" else (
                        "runtime-candidate" if any(k in asset_kinds for k in ("runtime", "runtimeTargets", "native", "contentFiles"))
                        else "build-or-publish-review-required")})
    files = ["Cargo.toml", "Cargo.lock", "desktop/packages.lock.json", "desktop.linux/packages.lock.json",
             "desktop.tests/packages.lock.json", "desktop.linux/global.json"]
    return {"schema": 1, "inputs": {name: digest(ROOT / name) for name in files},
            "advisory_database": rust_audit.get("database"),
            "nuget_audits": nuget_audits,
            "rust": rust, "nuget": nuget,
            "sdk": json.loads((ROOT / "desktop.linux/global.json").read_text()),
            "limits": ["runtime-candidate is graph/asset evidence, not binary reachability proof",
                       "SDK runtime packs, external tools and OS libraries need separate review",
                       "License metadata and notice hashes do not establish legal compatibility"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nuget-cache", action="append", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rust-audit", type=Path, required=True)
    parser.add_argument("--nuget-audit", action="append", default=[], metavar="PROJECT=REPORT")
    args = parser.parse_args()
    try:
        nuget_audits = {project: json.loads(Path(path).read_text(encoding="utf-8-sig"))
                        for project, path in (argument.split("=", 1) for argument in args.nuget_audit)}
        result = inventory(args.nuget_cache, json.loads(args.rust_audit.read_text(encoding="utf-8-sig")), nuget_audits)
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("Inventory unverified; restore locked packages and retry. Raw tool output withheld.")
    print(f"Inventory: {len(result['rust'])} Rust entries; {len(result['nuget'])} NuGet target entries")


if __name__ == "__main__":
    main()
