"""Trusted release-intent to promotion boundary. Candidate code is never executed."""
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import tempfile
import tomllib
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import UUID
import zipfile
from release_files import checked_files

REPO = "oangsa/RoLauncher"
CI = ".github/workflows/ci-release.yml"
BETA = ".github/workflows/beta-release.yml"
STABLE_VERSION = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
CHECKS = {"Branch flow", "Windows checks", "Linux checks and package (Ubuntu 24.04)"}
LINUX_CHECKS = {"Linux checks (Ubuntu)", "Linux checks (Arch)"}
GITHUB_ACTIONS_ID = 15368  # Public GitHub Actions App, verified against hosted check runs.
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_EXPANDED_BYTES = 512 * 1024 * 1024  # Across both platform archives, not per entry.
MAX_ENTRY_BYTES = 256 * 1024 * 1024
MAX_CHECKSUM_BYTES = 1024 * 1024
MAX_JSON_BYTES = 8 * 1024 * 1024
COPY_BYTES = 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def content(call, path, sha):
    result = call(f"/contents/{path}?ref={sha}")
    require(result["encoding"] == "base64", "Unsupported content encoding")
    return base64.b64decode(result["content"]).decode("utf-8")


def unique_object(pairs):
    result = dict(pairs)
    require(len(result) == len(pairs), "Duplicate JSON fields")
    return result


def authorize(call, run):
    require(run["repository"]["full_name"] == REPO and run["head_repository"]["full_name"] == REPO,
            "Foreign repository")
    sha = run["head_sha"]
    require(re.fullmatch(r"[a-f0-9]{40}", sha), "Invalid source identity")
    if run["path"] == BETA:
        tag = run["head_branch"]
        require(run["event"] == "push" and re.fullmatch("beta-v" + STABLE_VERSION, tag), "Invalid beta tag trigger")
        owner = call("")["owner"]
        require(owner["login"] == "oangsa" and owner["type"] == "User" and run["actor"]["id"] == owner["id"],
                "Beta tag push requires repository owner")
        require(promotion_state(call, tag, sha, annotated=True), "Beta tag is missing")
        require(call(f"/compare/{sha}...dev")["status"] in ("ahead", "identical"), "Beta source is not in dev history")
        return dict(schema=1, id="beta:" + tag, version=tag.removeprefix("beta-v"), channel="beta")
    require(run["event"] == "push" and run["head_branch"] == "main" and run["path"] == CI,
            "Stable authority requires main push CI")
    commit = call(f"/commits/{sha}?per_page=100")
    require(len(commit["files"]) < 100, "Commit diff exceeds auditable limit; split promotion")
    intents = [f for f in commit["files"] if f["filename"].startswith(".github/release-intents/")]
    if not intents:
        return None
    require(len(intents) == 1 and intents[0]["status"] == "added", "One newly added intent required")
    path = intents[0]["filename"]
    intent = json.loads(content(call, path, sha), object_pairs_hook=unique_object)
    require(isinstance(intent, dict), "Intent must be a JSON object")
    hotfix = type(intent.get("schema")) is int and intent["schema"] in (2, 3)
    recovery = type(intent.get("schema")) is int and intent["schema"] == 3
    require((hotfix or (set(intent) == {"schema", "id", "version", "channel"} and type(intent["schema"]) is int
            and intent["schema"] == 1)) and intent["channel"] == "stable", "Invalid release intent contract")
    require(str(UUID(intent["id"], version=4)) == intent["id"]
            and path == ".github/release-intents/" + intent["id"] + ".json", "Invalid intent identity")
    require(isinstance(intent["version"], str) and re.fullmatch(STABLE_VERSION, intent["version"]),
            "Stable version required")
    history = call(f"/commits?path={path}&sha={sha}&per_page=2")
    require(len(history) == 1, "Intent path was edited or reused")
    owner = call("")["owner"]
    require(owner["login"] == "oangsa" and owner["type"] == "User", "Unexpected repository owner")
    prs = call(f"/commits/{sha}/pulls?per_page=100")
    require(len(prs) < 100, "Ambiguous PR evidence")
    prs = [call(f'/pulls/{p["number"]}') for p in prs]
    expected_branch = ("hotfix-intent/" if recovery else "hotfix/") + intent["version"] if hotfix else "dev"
    matches = [p for p in prs if p["merged"] and p["merge_commit_sha"] == sha
               and p["base"]["ref"] == "main" and p["base"]["repo"]["full_name"] == REPO
               and p["head"]["ref"] == expected_branch and p["head"]["repo"]["full_name"] == REPO
               and (p.get("merged_by") or {}).get("id") == owner["id"]]
    require(len(matches) == 1, "Owner-authorized release source to main merge required")
    if hotfix:
        from hotfix_policy import verify_hotfix, verify_hotfix_recovery
        if recovery:
            return verify_hotfix_recovery(call, intent, matches[0], commit, path, sha, content)
        return verify_hotfix(call, intent, matches[0], commit, path, sha, content)
    return intent


def successful_jobs(call, run, expected):
    require(run["status"] == "completed" and run["conclusion"] == "success", "Source checks did not succeed")
    result = call(f'/actions/runs/{run["id"]}/attempts/{run["run_attempt"]}/jobs?per_page=100')
    jobs = result["jobs"]
    require(result["total_count"] == len(jobs) and len(jobs) < 100, "Incomplete check evidence")
    for name in expected:
        matches = [j for j in jobs if j["name"] == name]
        require(len(matches) == 1 and matches[0]["conclusion"] == "success"
                and matches[0]["head_sha"] == run["head_sha"], "Required check missing, skipped, failed or wrong source: " + name)


def unchanged_run(call, run, expected):
    current = call(f'/actions/runs/{run["id"]}')
    require(all(current[k] == run[k] for k in ("id", "run_attempt", "head_sha", "path", "event", "head_branch")),
            "Source run identity or attempt changed")
    successful_jobs(call, current, expected)


def protected_branches(call, strict=False):
    require(call("/actions/workflows/release-promotion.yml")["state"] == "active", "Publisher workflow is disabled")
    for branch in ("main", "dev"):
        rules = call("/rules/branches/" + branch + "?per_page=100")
        require(len(rules) < 100, "Incomplete branch ruleset evidence")
        types = {r["type"] for r in rules}
        require({"pull_request", "non_fast_forward", "deletion", "required_status_checks"} <= types,
                "Active branch rulesets required before promotion")
        checks = set()
        for rule in rules:
            if rule["type"] != "required_status_checks":
                continue
            params = rule["parameters"]
            require(params.get("strict_required_status_checks_policy") is True,
                    "Strict up-to-date required checks must be enforced")
            for check in params["required_status_checks"]:
                if check["context"] in CHECKS | LINUX_CHECKS:
                    require(type(check.get("integration_id")) is int and check["integration_id"] == GITHUB_ACTIONS_ID,
                            "Required check must originate from GitHub Actions")
                    checks.add(check["context"])
        require(CHECKS | LINUX_CHECKS <= checks, "Required CI checks are not enforced by rulesets")
        for rule_id in {r.get("ruleset_id") for r in rules}:
            require(type(rule_id) is int and rule_id > 0, "Missing branch ruleset source")
            source = call(f"/rulesets/{rule_id}")
            require(source["id"] == rule_id and source["target"] == "branch" and source["enforcement"] == "active",
                    "Inactive or incorrect branch ruleset source")
            if "bypass_actors" in source:
                require(source["bypass_actors"] == [], "Branch ruleset bypasses must be empty")
            else:
                require(not strict, "Unverified branch ruleset bypasses block hotfix publication")
                # Metadata-read does not expose bypasses. Never infer an empty list from omission.
                print("Owner/admin activation verification required: branch bypass actors are not visible.")
    rulesets = call("/rulesets?includes_parents=true&per_page=100")
    require(len(rulesets) < 100, "Incomplete tag ruleset evidence")
    tags = [call(f'/rulesets/{r["id"]}') for r in rulesets if r["target"] == "tag" and r["enforcement"] == "active"]
    for pattern in ("refs/tags/v*", "refs/tags/beta-v*"):
        matching = [r for r in tags if not r["conditions"]["ref_name"]["exclude"]
                    and (pattern in r["conditions"]["ref_name"]["include"]
                         or "~ALL" in r["conditions"]["ref_name"]["include"])]
        types = {rule["type"] for ruleset in matching for rule in ruleset["rules"]}
        for ruleset in matching:
            if "bypass_actors" not in ruleset:
                require(not strict, "Unverified tag ruleset bypasses block hotfix publication")
                print("Owner/admin activation verification required: tag bypass actors are not visible.")
                continue
            for actor in ruleset["bypass_actors"]:
                require(pattern == "refs/tags/beta-v*" and actor == {
                    "actor_type": "RepositoryRole", "actor_id": 5, "bypass_mode": "always"},
                    "Unexpected version tag bypass; beta permits only the sole owner/admin role")
        require({"update", "deletion"} <= types, "Active version tag rules must prevent updates and deletion")
        if pattern == "refs/tags/beta-v*":
            require("creation" in types, "Historical beta workflows require restricted tag creation")


def artifact_files(call, run, version, beta=False):
    result = call(f'/actions/runs/{run["id"]}/artifacts?per_page=100')
    require(result["total_count"] == len(result["artifacts"]) and len(result["artifacts"]) < 100,
            "Incomplete artifact evidence")
    prefix = "rolauncher-v" + version
    layouts = {"beta-windows" if beta else "release-files": {prefix + s for s in ("-setup-x64.exe", "-windows-x64.zip", "-source.zip", "-SHA256SUMS.txt")},
               "beta-linux" if beta else "release-linux-files": {prefix + s for s in ("-linux-x64.tar.gz", "-linux-SHA256SUMS.txt")}}
    layouts = {f'{name}-attempt-{run["run_attempt"]}': entries for name, entries in layouts.items()}
    provenance = []
    expanded = 0
    with tempfile.TemporaryDirectory() as temp:
        for name, expected in layouts.items():
            matches = [a for a in result["artifacts"] if a["name"] == name]
            require(len(matches) == 1, "Missing or ambiguous artifact: " + name)
            artifact = matches[0]
            require(not artifact["expired"] and artifact["workflow_run"]["id"] == run["id"]
                    and artifact["workflow_run"]["head_sha"] == run["head_sha"], "Wrong artifact source or expired artifact")
            require(type(artifact.get("size_in_bytes")) is int and 0 < artifact["size_in_bytes"] <= MAX_ARCHIVE_BYTES,
                    "Artifact archive metadata exceeds size limit")
            archive_data = call(f'/actions/artifacts/{artifact["id"]}/zip', raw=True)
            require(len(archive_data) <= MAX_ARCHIVE_BYTES, "Downloaded archive exceeds size limit")
            require(artifact.get("digest") == "sha256:" + hashlib.sha256(archive_data).hexdigest(), "Artifact archive digest mismatch")
            # Bound the central directory before ZipFile allocates one object per entry.
            end = archive_data.rfind(b"PK\x05\x06", max(0, len(archive_data) - 65_557))
            require(end >= 0 and end + 22 <= len(archive_data), "Invalid ZIP directory footer")
            disk, directory_disk, local_count, count, size, offset, comment = struct.unpack_from("<4H2IH", archive_data, end + 4)
            require(disk == directory_disk == 0 and local_count == count == len(expected)
                    and size <= 65_536 and offset + size == end and end + 22 + comment == len(archive_data)
                    and archive_data[max(0, end - 20):end - 16] != b"PK\x06\x07",
                    "ZIP directory limit exceeded or unsupported split/ZIP64 archive")
            with zipfile.ZipFile(io.BytesIO(archive_data)) as archive:
                require(len(archive.infolist()) == len(expected) and set(archive.namelist()) == expected,
                        "Unexpected artifact archive entries")
                entries = archive.infolist()
                require(expanded + sum(e.file_size for e in entries) <= MAX_EXPANDED_BYTES,
                        "Aggregate decompression size limit exceeded")
                for entry in entries:
                    limit = min(MAX_ENTRY_BYTES, MAX_CHECKSUM_BYTES) if entry.filename.endswith("SHA256SUMS.txt") else MAX_ENTRY_BYTES
                    require(entry.file_size <= limit, "Artifact entry size limit exceeded")
                    require(entry.external_attr >> 16 & 0o170000 != 0o120000
                            and not entry.flag_bits & 1 and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                            "Symlink, encrypted or unsupported artifact")
                    # Fixed allowlist basenames only; never extract attacker-selected paths.
                    written = 0
                    with archive.open(entry) as source, (Path(temp) / entry.filename).open("wb") as target:
                        while chunk := source.read(COPY_BYTES):
                            written += len(chunk)
                            expanded += len(chunk)
                            require(written <= limit and expanded <= MAX_EXPANDED_BYTES,
                                    "Actual decompression size limit exceeded")
                            target.write(chunk)
                    require(written == entry.file_size, "Artifact entry size mismatch")
            del archive_data
            provenance.append({k: artifact[k] for k in ("id", "name", "digest")})
        files = {n: p if isinstance(p, bytes) else p.read_bytes()
                 for n, p in checked_files(Path(temp), version).items()}
    return files, provenance


def promotion_state(call, tag, sha, annotated=False):
    ref = call("/git/ref/tags/" + tag, missing_ok=True)
    if ref:
        obj = ref["object"]
        if annotated and obj["type"] == "tag":
            obj = call("/git/tags/" + obj["sha"])["object"]
        # Pipeline-created stable tags are lightweight. Ambiguous annotated tags fail closed.
        require(obj["type"] == "commit" and obj["sha"] == sha, "Version tag points to another source")
    return ref


def promote(call, run, evidence=None):
    intent = authorize(call, run)
    if intent is None:
        return "no-intent"
    sha, version = run["head_sha"], intent["version"]
    beta = intent["channel"] == "beta"
    source_checks = CHECKS - {"Branch flow"} if beta else CHECKS
    successful_jobs(call, run, source_checks)
    linux_run = None
    if not beta:
        linux = call(f"/actions/workflows/linux.yml/runs?event=push&branch=main&head_sha={sha}&per_page=100")
        require(linux["total_count"] == len(linux["workflow_runs"]) and linux["total_count"] == 1,
                "One matching Linux validation run required")
        linux_run = linux["workflow_runs"][0]
        require(linux_run["head_sha"] == sha and linux_run["event"] == "push" and linux_run["head_branch"] == "main"
                and linux_run["head_repository"]["full_name"] == REPO and linux_run["path"] == ".github/workflows/linux.yml",
                "Wrong Linux validation source")
        successful_jobs(call, linux_run, LINUX_CHECKS)
    if not beta:
        require(call("/branches/main")["commit"]["sha"] == sha, "Stale intent: main has advanced")
    manifest_version = tomllib.loads(content(call, "Cargo.toml", sha))["package"]["version"]
    lock = tomllib.loads(content(call, "Cargo.lock", sha))
    require(manifest_version == version and next(p for p in lock["package"] if p["name"] == "rolauncher")["version"] == version
            and content(call, "README.md", sha).startswith(f"# RoLauncher {version}\n"), "Version metadata mismatch")
    notes = content(call, f"docs/RELEASE-{version}.md", sha)
    require(notes.startswith(f"# RoLauncher {version}\n") and len(notes.strip()) > 120
            and "Describe the user-visible changes" not in notes, "Release notes mismatch")
    files, artifacts = artifact_files(call, run, version, beta)
    if intent.get('source') in ('hotfix', 'hotfix-recovery'):
        from hotfix_policy import verify_source
        verify_source(call, sha, files[f'rolauncher-v{version}-source.zip'])
    receipt = dict(schema=1, intent=intent["id"], channel=intent["channel"], version=version, source=sha,
                   run=run["id"], attempt=run["run_attempt"], artifacts=artifacts,
                   files={n: hashlib.sha256(data).hexdigest() for n, data in sorted(files.items())})
    receipt_data = (json.dumps(receipt, sort_keys=True, separators=(",", ":")) + "\n").encode()
    marker = "<!-- release-promotion:" + hashlib.sha256(receipt_data).hexdigest() + " -->"
    files["release-promotion.json"] = receipt_data
    tag = ("beta-v" if beta else "v") + version
    ref = promotion_state(call, tag, sha, annotated=beta)
    release = call("/releases/tags/" + tag, missing_ok=True)
    if release:
        require(marker in (release.get("body") or "") and release["target_commitish"] == sha
                and release["prerelease"] == beta, "Existing release has different intent, artifacts, source or channel")
        require(ref is not None, "Existing release tag is missing")
    if not beta and (not release or release["draft"]):
        latest = call("/releases/latest", missing_ok=True)
        require(not latest or tuple(map(int, version.split("."))) > tuple(map(int, latest["tag_name"].removeprefix("v").split("."))),
                "Version must advance latest published stable")
    # All candidate admission above is read-only. No human request until it succeeds.
    from owner_evidence import binding_for
    binding = binding_for(intent, run, linux_run, receipt_data)
    require(evidence is not None, "Live owner evidence transport required")
    finishing = evidence.stage == 'finish'
    if not finishing:
        controls = evidence.gate('preflight', call, binding)
        protected_branches(controls, strict=True)
        require(authorize(call, run) == intent, 'Release authority changed during preflight')
        if not beta:
            require(call('/branches/main')['commit']['sha'] == sha, 'Main advanced during preflight')
        unchanged_run(call, run, source_checks)
        if linux_run:
            unchanged_run(call, linux_run, LINUX_CHECKS)
        require(promotion_state(call, tag, sha, annotated=beta) == ref
                and call('/releases/tags/' + tag, missing_ok=True) == release,
                'Publication state changed during preflight; retry with fresh evidence')
        evidence.fresh()
    else:
        evidence.resume(binding)
        require(ref and release, "Interrupted staging must be reconciled in a new attempt")
    if not ref:
        evidence.fresh()
        call("/git/refs", "POST", {"ref": "refs/tags/" + tag, "sha": sha})
    if not release:
        evidence.fresh()
        release = call("/releases", "POST", {"tag_name": tag, "target_commitish": sha,
                       "name": "RoLauncher " + version, "body": notes + "\n\n" + marker + "\n",
                       "draft": True, "prerelease": beta, "make_latest": "false"})
    path = "/releases/" + str(release["id"])
    upload = release["upload_url"].split("{")[0]
    require(upload == f'https://uploads.github.com/repos/{REPO}/releases/{release["id"]}/assets', "Unexpected upload host/path")
    assets = call(path + "/assets")
    require(len({a["name"] for a in assets}) == len(assets) and {a["name"] for a in assets} <= set(files), "Unexpected release assets")
    for asset in assets:
        data = files[asset["name"]]
        require(asset["size"] == len(data) and asset.get("digest") == "sha256:" + hashlib.sha256(data).hexdigest(),
                "Existing release asset differs; quarantine instead of replacing")
    existing = {a["name"] for a in assets}
    if not release["draft"] or finishing:
        require(existing == set(files), "Published release is incomplete")
    if not release['draft']:
        require(release.get('immutable') is True, 'Consumed release must be immutable')
    if not finishing:
        for name in sorted(set(files) - existing):
            evidence.fresh()
            call(upload + "?name=" + name, "POST", binary=files[name])
    uploaded = call(path + "/assets")
    require(len(uploaded) == len(files) and {a["name"] for a in uploaded} == set(files), "Incomplete uploaded assets")
    for asset in uploaded:
        data = files[asset["name"]]
        require(asset["size"] == len(data) and asset.get("digest") == "sha256:" + hashlib.sha256(data).hexdigest(), "Uploaded asset digest mismatch")
    require(promotion_state(call, tag, sha, annotated=beta), "Version tag disappeared")
    if not beta:
        require(call("/branches/main")["commit"]["sha"] == sha, "Main advanced before publication")
        require(authorize(call, run) == intent, "Release authority changed before publication")
    controls = evidence.gate('final', call, binding)
    protected_branches(controls, strict=True)
    # Repeat authority after the attended wait, not only before asking the owner.
    require(authorize(call, run) == intent, 'Release authority changed during owner wait')
    if not beta:
        require(call('/branches/main')['commit']['sha'] == sha, 'Main advanced during owner wait')
    unchanged_run(call, run, source_checks)
    if linux_run:
        unchanged_run(call, linux_run, LINUX_CHECKS)
    require(promotion_state(call, tag, sha, annotated=beta), 'Version tag disappeared during owner wait')
    require(call(path + '/assets') == uploaded, 'Release assets changed during owner wait')
    current = call(path)
    require(current['id'] == release['id'] and current['draft'] == release['draft']
            and current['prerelease'] == beta and current['target_commitish'] == sha
            and marker in (current.get('body') or ''), 'Release state changed during owner wait')
    if not beta and release['draft']:
        latest = call('/releases/latest', missing_ok=True)
        require(not latest or tuple(map(int, version.split('.'))) >
                tuple(map(int, latest['tag_name'].removeprefix('v').split('.'))), 'Latest stable advanced during owner wait')
    evidence.fresh()
    if not release['draft']:
        current = call(path)
        require(current.get('immutable') is True and not current['draft']
                and current['target_commitish'] == sha and current['prerelease'] == beta
                and marker in (current.get('body') or ''), 'Consumed release readback mismatch')
        return 'consumed'
    result = call(path, "PATCH", {"draft": False, "prerelease": beta, "make_latest": "false" if beta else "true"})
    require(not result["draft"] and result["prerelease"] == beta, "Promotion response has wrong channel")
    current = call(path)
    require(current.get('immutable') is True and not current['draft'] and current['prerelease'] == beta
            and current['target_commitish'] == sha and marker in (current.get('body') or ''),
            'Published release immutable readback mismatch; quarantine, never replace')
    return "published"


class SafeRedirect(HTTPRedirectHandler):
    """GitHub artifact downloads redirect to signed storage URLs, without our token."""
    def redirect_request(self, request, fp, code, message, headers, newurl):
        require(urlparse(newurl).scheme == "https", "Refuse non-HTTPS redirect")
        redirected = super().redirect_request(request, fp, code, message, headers, newurl)
        if redirected and urlparse(request.full_url).netloc != urlparse(newurl).netloc:
            redirected.remove_header("Authorization")
        return redirected


def github_api(token):
    opener = build_opener(SafeRedirect())
    def call(path, method="GET", body=None, missing_ok=False, binary=None, raw=False):
        url = path if path.startswith("https://") else "https://api.github.com/repos/" + REPO + path
        require(url.startswith("https://api.github.com/repos/" + REPO + "/") or url == "https://api.github.com/repos/" + REPO
                or re.fullmatch(r"https://uploads.github.com/repos/oangsa/RoLauncher/releases/\d+/assets\?name=[A-Za-z0-9._-]+", url),
                "Refuse unexpected GitHub endpoint")
        data = binary if binary is not None else json.dumps(body).encode() if body is not None else None
        headers = {"Authorization": "Bearer " + token, "Accept": "application/vnd.github+json",
                   "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "RoLauncher-protected-promotion"}
        if data is not None:
            headers["Content-Type"] = "application/octet-stream" if binary is not None else "application/json"
        try:
            with opener.open(Request(url, data=data, method=method, headers=headers), timeout=120) as response:
                limit = MAX_ARCHIVE_BYTES if raw else MAX_JSON_BYTES
                length = response.headers.get("Content-Length")
                require(length is None or 0 <= int(length) <= limit, "GitHub response Content-Length exceeds size limit")
                result = response.read(limit + 1)
                require(len(result) <= limit, "GitHub response exceeds size limit")
                return result if raw else json.loads(result, object_pairs_hook=unique_object) if result else None
        except HTTPError as error:
            if error.code == 404 and missing_ok:
                return None
            raise RuntimeError(f"GitHub {method} failed (HTTP {error.code}); no automatic mutation retry") from None
    return call


def promote_event(call, event, channel=None, evidence=None):
    """Reconcile a GitHub completion event against fresh API evidence, never its artifacts alone."""
    require(event["repository"]["default_branch"] == "main", "Trusted workflow must reside on main")
    source = event["workflow_run"]
    require(type(source["id"]) is int and source["id"] > 0, "Invalid run ID")
    run = call(f'/actions/runs/{source["id"]}')
    require(run["run_attempt"] == source["run_attempt"] and run["head_sha"] == source["head_sha"], "Stale workflow event")
    require(run["status"] == "completed" and run["conclusion"] == "success", "Triggering run did not succeed")
    if run["path"] == ".github/workflows/linux.yml":
        require(run["event"] == "push" and run["head_branch"] == "main"
                and run["head_repository"]["full_name"] == REPO, "Linux wakeup must be an official main push")
        runs = call(f'/actions/workflows/ci-release.yml/runs?event=push&branch=main&head_sha={run["head_sha"]}&per_page=100')
        require(runs["total_count"] == 1 and len(runs["workflow_runs"]) == 1, "Matching source CI required")
        run = runs["workflow_runs"][0]
    if channel:
        require(run["path"] == (BETA if channel == "beta" else CI), "Wrong publication channel")
    return promote(call, run, evidence)


def main(channel=None):
    if os.environ.get("RELEASE_PROMOTION_ENABLED") != "true":
        print("Release promotion disabled; separate owner activation required.")
        return
    require(os.environ.get("GITHUB_EVENT_NAME") == "workflow_run", "Only workflow_run may promote")
    require(os.environ.get("GITHUB_REPOSITORY") == REPO, "Official repository required")
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    import argparse
    from owner_evidence import StagedEvidence, ChallengeReady
    parser = argparse.ArgumentParser()
    parser.add_argument('--stage', choices=('prepare', 'stage', 'finish'), required=True)
    args = parser.parse_args()
    transport = StagedEvidence(args.stage)
    try:
        result = promote_event(github_api(os.environ['GITHUB_TOKEN']), event, channel, transport)
    except ChallengeReady:
        result = 'challenge-ready'
    print(result)
    if args.stage == 'prepare':
        with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as output:
            output.write('admitted=' + ('true' if result == 'challenge-ready' else 'false') + '\n')


if __name__ == "__main__":
    main()

