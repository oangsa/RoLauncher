"""Behavioral promotion tests; all GitHub effects stay in an in-memory fixture."""
import base64
import copy
from contextlib import redirect_stdout
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
import warnings
import zipfile
from unittest.mock import patch
import release_promotion
from release_promotion import authorize


class FixtureEvidence:
    """Synthetic control transport only; real SSH verification lives in test-owner-evidence."""
    stage = 'all'
    def gate(self, phase, call, binding):
        if call('/immutable-releases').get('enabled') is not True:
            raise ValueError('Native release immutability disabled or missing')
        return call
    def fresh(self):
        pass


def promote(call, run):
    return release_promotion.promote(call, run, FixtureEvidence())


def promote_event(call, event, channel=None):
    return release_promotion.promote_event(call, event, channel, FixtureEvidence())


SHA = "a" * 40
INTENT = "12345678-1234-4234-8234-123456789abc"
PATH = ".github/release-intents/" + INTENT + ".json"
REPO = "oangsa/RoLauncher"


class GitHubFixture:
    def __init__(self):
        self.writes = []
        self.run = dict(id=12, run_attempt=1, head_sha=SHA, head_branch="main", event="push",
                        status="completed", conclusion="success", path=".github/workflows/ci-release.yml",
                        repository={"full_name": REPO}, head_repository={"full_name": REPO})
        self.files = [{"filename": "README.md", "status": "modified"}]
        self.intent = dict(schema=1, id=INTENT, version="2.3.4", channel="stable")
        self.intent_json = None
        self.pr = dict(merged=True, merge_commit_sha=SHA, merged_by={"id": 7},
                       base={"ref": "main", "repo": {"full_name": REPO}},
                       head={"ref": "dev", "repo": {"full_name": REPO}})
        self.history = [{"sha": "b" * 40}]
        self.jobs = [{"name": n, "conclusion": "success", "head_sha": SHA}
                     for n in ("Branch flow", "Windows checks", "Linux checks and package (Ubuntu 24.04)")]
        self.branch = {"commit": {"sha": SHA}}
        checks = ["Branch flow", "Windows checks", "Linux checks and package (Ubuntu 24.04)",
                  "Linux checks (Ubuntu)", "Linux checks (Arch)"]
        self.rules = [{"type": t, "ruleset_id": 2} for t in ("pull_request", "non_fast_forward", "deletion")]
        self.rules.append({"type": "required_status_checks", "ruleset_id": 2, "parameters": {
            "strict_required_status_checks_policy": True,
            "required_status_checks": [{"context": n, "integration_id": 15368} for n in checks]}})
        self.branch_ruleset = dict(id=2, target="branch", enforcement="active", bypass_actors=[])
        self.tag_rules = dict(id=1, target="tag", enforcement="active", bypass_actors=[],
                              conditions={"ref_name": {"include": ["refs/tags/v*", "refs/tags/beta-v*"], "exclude": []}},
                              rules=[{"type": "update"}, {"type": "deletion"}, {"type": "creation"}])
        self.linux_run = dict(self.run, id=13, path=".github/workflows/linux.yml")
        self.linux_jobs = [{"name": "Linux checks (" + n + ")", "conclusion": "success", "head_sha": SHA}
                           for n in ("Ubuntu", "Arch")]
        self.release = None
        self.tag = None
        self.assets = {}
        self.archives = {}
        self.artifacts = []
        self.fail_upload = None
        self.ambiguous_publish = False
        self.fail_create = False
        self.publisher_state = "active"
        self.immutable = {'enabled': True, 'enforced_by_owner': False}
        prefix = "rolauncher-v2.3.4"
        for artifact_id, name, suffix, endings in (
            (30, "release-files", "-SHA256SUMS.txt", ["-setup-x64.exe", "-windows-x64.zip", "-source.zip"]),
            (31, "release-linux-files", "-linux-SHA256SUMS.txt", ["-linux-x64.tar.gz"]),
        ):
            entries = {prefix + ending: ("synthetic " + ending).encode() for ending in endings}
            entries[prefix + suffix] = "".join(hashlib.sha256(data).hexdigest() + "  " + n + "\n"
                                               for n, data in entries.items()).encode()
            buffer = io.BytesIO()
            with zipfile.ZipFile(buffer, "w") as archive:
                for n, data in entries.items(): archive.writestr(n, data)
            self.archives[artifact_id] = buffer.getvalue()
            self.artifacts.append(dict(id=artifact_id, name=name + "-attempt-1", expired=False,
                                       size_in_bytes=len(buffer.getvalue()),
                                       digest="sha256:" + hashlib.sha256(buffer.getvalue()).hexdigest(),
                                       workflow_run={"id": 12, "head_sha": SHA}))

    def with_intent(self):
        self.files = [{"filename": PATH, "status": "added"}]
        return self

    def with_beta(self):
        self.run.update(path=".github/workflows/beta-release.yml", head_branch="beta-v2.3.4", actor={"id": 7})
        self.jobs = [j for j in self.jobs if j["name"] != "Branch flow"]
        self.artifacts[0]["name"] = "beta-windows-attempt-1"
        self.artifacts[1]["name"] = "beta-linux-attempt-1"
        self.tag = {"object": {"type": "commit", "sha": SHA}}
        return self

    def __call__(self, path, method="GET", body=None, **kwargs):
        if method != "GET":
            self.writes.append((path, method, body))
            if path == "/git/refs" and method == "POST":
                self.tag = {"object": {"type": "commit", "sha": body["sha"]}}
                return self.tag
            if path == "/releases" and method == "POST":
                self.release = dict(body, id=50, upload_url="https://uploads.github.com/repos/" + REPO + "/releases/50/assets{?name,label}")
                self.tag = {"object": {"type": "commit", "sha": SHA}}
                if self.fail_create:
                    self.fail_create = False
                    raise TimeoutError("Draft response lost")
                return self.release
            if path.startswith("https://uploads.github.com/"):
                name = path.split("?name=")[1]
                data = kwargs["binary"]
                asset_id = 100 + len(self.assets)
                self.assets[name] = dict(id=asset_id, name=name, size=len(data),
                                         digest="sha256:" + hashlib.sha256(data).hexdigest(), data=data)
                if name == self.fail_upload: raise TimeoutError("Upload response lost")
                return self.assets[name]
            if path == "/releases/50" and method == "PATCH":
                self.release.update(body)
                self.release['immutable'] = True
                if self.ambiguous_publish:
                    self.ambiguous_publish = False
                    raise TimeoutError("Publish response lost")
                return self.release
            raise AssertionError((path, method))
        if path in ("/commits/" + SHA, "/commits/" + SHA + "?per_page=100"):
            return {"files": self.files}
        if path == "":
            return {"full_name": REPO, "owner": {"id": 7, "login": "oangsa", "type": "User"}}
        if path == "/commits/" + SHA + "/pulls?per_page=100":
            return [{"number": 4}]
        if path == "/pulls/4": return self.pr
        if path.startswith("/commits?path="): return self.history
        if path == "/contents/" + PATH + "?ref=" + SHA:
            return {"encoding": "base64", "content": base64.b64encode((self.intent_json or json.dumps(self.intent)).encode()).decode()}
        if path.startswith("/contents/"):
            name = path.split("/contents/")[1].split("?ref=")[0]
            texts = {"Cargo.toml": '[package]\nname="rolauncher"\nversion="2.3.4"\n',
                     "Cargo.lock": '[[package]]\nname="rolauncher"\nversion="2.3.4"\n',
                     "README.md": "# RoLauncher 2.3.4\n",
                     "docs/RELEASE-2.3.4.md": "# RoLauncher 2.3.4\n\n" + "Synthetic fixture release notes. " * 5}
            return {"encoding": "base64", "content": base64.b64encode(texts[name].encode()).decode()}
        if path.startswith("/branches/"): return self.branch
        if path.startswith("/compare/"): return {"status": "identical"}
        if path.startswith("/rules/branches/"): return self.rules
        if path.startswith("/rulesets?"): return [{k: self.tag_rules[k] for k in ("id", "target", "enforcement")}]
        if path == "/rulesets/1": return self.tag_rules
        if path == "/rulesets/2": return self.branch_ruleset
        if path == "/actions/runs/12/attempts/1/jobs?per_page=100": return {"jobs": self.jobs, "total_count": len(self.jobs)}
        if path == "/actions/runs/13/attempts/1/jobs?per_page=100": return {"jobs": self.linux_jobs, "total_count": len(self.linux_jobs)}
        if path.startswith("/actions/workflows/linux.yml/runs?"): return {"workflow_runs": [self.linux_run], "total_count": 1}
        if path.startswith("/actions/workflows/ci-release.yml/runs?"): return {"workflow_runs": [self.run], "total_count": 1}
        if path == "/actions/runs/12": return self.run
        if path == "/actions/runs/13": return self.linux_run
        if path == "/actions/workflows/release-promotion.yml": return {"state": self.publisher_state}
        if path == "/actions/runs/12/artifacts?per_page=100": return {"artifacts": self.artifacts, "total_count": len(self.artifacts)}
        if path.startswith("/actions/artifacts/"): return self.archives[int(path.split("/")[3])]
        if path in ("/git/ref/tags/v2.3.4", "/git/ref/tags/beta-v2.3.4"): return self.tag
        if path in ("/releases/tags/v2.3.4", "/releases/tags/beta-v2.3.4"): return self.release
        if path == "/releases/latest": return {"tag_name": "v2.3.3"}
        if path == "/releases/50/assets": return list(self.assets.values())
        if path == '/releases/50': return self.release
        if path == '/immutable-releases': return self.immutable
        if path.startswith("/releases/assets/"):
            return next(a["data"] for a in self.assets.values() if a["id"] == int(path.split("/")[-1]))
        raise AssertionError(path)


class PromotionTests(unittest.TestCase):
    def test_zip_directory_metadata_is_bounded_before_parsing_entries(self):
        github = GitHubFixture().with_intent()
        with zipfile.ZipFile(io.BytesIO(github.archives[30])) as original:
            entries = [(i, original.read(i)) for i in original.infolist()]
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            for info, data in entries:
                info.comment = b"x" * 20_000
                archive.writestr(info, data)
        github.archives[30] = buffer.getvalue()
        github.artifacts[0].update(size_in_bytes=len(buffer.getvalue()),
                                   digest="sha256:" + hashlib.sha256(buffer.getvalue()).hexdigest())
        with self.assertRaisesRegex(ValueError, "ZIP directory"): promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_network_downloads_are_bounded_even_without_honest_content_length(self):
        for raw in (True, False):
            for header in (None, "1000", "1"):
                with self.subTest(raw=raw, header=header):
                    class Response(io.BytesIO):
                        headers = {} if header is None else {"Content-Length": header}
                        reads = []
                        def read(self, size=-1):
                            self.reads.append(size)
                            return super().read(size)
                    response = Response(b"x" * 1000)
                    class Connection:
                        def open(self, *args, **kwargs): return response
                    with patch.object(release_promotion, "build_opener", return_value=Connection()), patch.multiple(
                            release_promotion, MAX_ARCHIVE_BYTES=64, MAX_JSON_BYTES=64):
                        api = release_promotion.github_api("synthetic-token")
                        with self.assertRaisesRegex(ValueError, "size limit"):
                            api("/actions/artifacts/30/zip", raw=raw)
                    self.assertEqual(response.reads, [] if header == "1000" else [65])

    def test_separate_beta_owner_creation_bypass_preserves_compatibility(self):
        for fault in (None, "app", "wrong-role", "pr-only"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_beta()
                github.tag_rules["conditions"]["ref_name"]["include"] = ["refs/tags/v*"]
                beta_rules = copy.deepcopy(github.tag_rules)
                beta_rules["id"] = 3
                beta_rules["conditions"]["ref_name"]["include"] = ["refs/tags/beta-v*"]
                actor = dict(actor_type="RepositoryRole", actor_id=5, bypass_mode="always")
                if fault == "app": actor["actor_type"] = "Integration"
                if fault == "wrong-role": actor["actor_id"] = 4
                if fault == "pr-only": actor["bypass_mode"] = "pull_request"
                beta_rules["bypass_actors"] = [actor]
                def api(path, method="GET", body=None, **kwargs):
                    if path.startswith("/rulesets?"): return [github.tag_rules, beta_rules]
                    if path == "/rulesets/3": return beta_rules
                    return github(path, method, body, **kwargs)
                if fault:
                    with self.assertRaisesRegex(ValueError, "tag bypass"): promote(api, github.run)
                    self.assertEqual(github.writes, [])
                else:
                    self.assertEqual(promote(api, github.run), "published")
                    self.assertTrue(github.release["prerelease"])
                    self.assertEqual(github.release["make_latest"], "false")

    def test_artifact_resource_limits_deny_before_any_release_mutation(self):
        for fault in ("metadata", "download", "aggregate", "entry", "checksum"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                sizes = []
                for data in github.archives.values():
                    with zipfile.ZipFile(io.BytesIO(data)) as archive:
                        sizes.extend(i.file_size for i in archive.infolist())
                limits = dict(MAX_ARCHIVE_BYTES=10_000, MAX_EXPANDED_BYTES=10_000,
                              MAX_ENTRY_BYTES=10_000, MAX_CHECKSUM_BYTES=10_000)
                if fault == "metadata": github.artifacts[0]["size_in_bytes"] = 10_001
                if fault == "download":
                    limits["MAX_ARCHIVE_BYTES"] = len(github.archives[30]) - 1
                    github.artifacts[0]["size_in_bytes"] = 1  # Metadata understates response size.
                if fault == "aggregate": limits["MAX_EXPANDED_BYTES"] = sum(sizes) - 1
                if fault == "entry": limits["MAX_ENTRY_BYTES"] = max(sizes) - 1
                if fault == "checksum": limits["MAX_CHECKSUM_BYTES"] = 1
                with patch.multiple(release_promotion, **limits):
                    with self.assertRaisesRegex(ValueError, "limit"): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_visible_tag_bypasses_cannot_weaken_immutable_tags(self):
        for actor in ("Integration", "RepositoryRole", "User", "DeployKey"):
            with self.subTest(actor=actor):
                github = GitHubFixture().with_intent()
                github.tag_rules["bypass_actors"] = [{"actor_type": actor, "actor_id": 5, "bypass_mode": "always"}]
                with self.assertRaisesRegex(ValueError, "tag bypass"): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_visible_branch_bypasses_and_inactive_rule_sources_deny(self):
        for fault in ("admin", "app", "team", "user", "deploy-key", "inactive", "wrong-target", "missing-source"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                actors = {"admin": "RepositoryRole", "app": "Integration", "team": "Team",
                          "user": "User", "deploy-key": "DeployKey"}
                if fault in actors:
                    github.branch_ruleset["bypass_actors"] = [{"actor_type": actors[fault], "actor_id": 5,
                                                              "bypass_mode": "pull_request"}]
                if fault == "inactive": github.branch_ruleset["enforcement"] = "evaluate"
                if fault == "wrong-target": github.branch_ruleset["target"] = "tag"
                if fault == "missing-source": github.rules[0].pop("ruleset_id")
                with self.assertRaises(ValueError): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_hidden_bypasses_fail_closed_for_feature_publication_too(self):
        github = GitHubFixture().with_intent()
        github.branch_ruleset.pop("bypass_actors")
        with self.assertRaisesRegex(ValueError, 'Unverified'):
            promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_weak_required_check_rules_cannot_authorize_publication(self):
        for branch in ("main", "dev"):
            for fault in ("wrong-app", "any-app", "missing-app", "not-strict", "missing-strict", "truncated"):
                with self.subTest(branch=branch, fault=fault):
                    github = GitHubFixture().with_intent()
                    weak = copy.deepcopy(github.rules)
                    params = weak[-1]["parameters"]
                    if fault == "wrong-app": params["required_status_checks"][0]["integration_id"] = 99
                    if fault == "any-app": params["required_status_checks"][0]["integration_id"] = None
                    if fault == "missing-app": params["required_status_checks"][0].pop("integration_id")
                    if fault == "not-strict": params["strict_required_status_checks_policy"] = False
                    if fault == "missing-strict": params.pop("strict_required_status_checks_policy")
                    if fault == "truncated": weak *= 25
                    def api(path, method="GET", body=None, **kwargs):
                        if path.split("?")[0] == "/rules/branches/" + branch: return weak
                        return github(path, method, body, **kwargs)
                    with self.assertRaises(ValueError): promote(api, github.run)
                    self.assertEqual(github.writes, [])

    def test_either_completed_workflow_can_reconcile_exact_source_once(self):
        for wakeup in ("ci", "linux"):
            with self.subTest(wakeup=wakeup):
                github = GitHubFixture().with_intent()
                run = github.run if wakeup == "ci" else github.linux_run
                event = dict(repository={"default_branch": "main"}, workflow_run=copy.deepcopy(run))
                self.assertEqual(promote_event(github, event), "published")
                github.writes.clear()
                self.assertEqual(promote_event(github, event), "consumed")
                self.assertEqual(github.writes, [])

    def test_cli_disabled_or_wrong_event_never_contacts_github(self):
        script = str(Path(__file__).with_name("release_promotion.py"))
        for enabled, event in (("false", "workflow_run"), ("true", "pull_request"), ("true", "workflow_dispatch")):
            with self.subTest(enabled=enabled, event=event):
                env = dict(os.environ, RELEASE_PROMOTION_ENABLED=enabled, GITHUB_EVENT_NAME=event,
                           GITHUB_REPOSITORY=REPO, GITHUB_TOKEN="", GITHUB_EVENT_PATH="missing-event.json")
                result = subprocess.run([sys.executable, script], env=env, text=True, capture_output=True)
                self.assertIn("disabled" if enabled == "false" else "workflow_run", result.stdout + result.stderr)
                self.assertEqual(result.returncode, 0 if enabled == "false" else 1)

    def test_owner_beta_tag_preserves_prerelease_and_stable_latest(self):
        github = GitHubFixture().with_beta()
        self.assertEqual(promote(github, github.run), "published")
        self.assertTrue(github.release["prerelease"])
        self.assertEqual(github.release["make_latest"], "false")
        self.assertEqual(github.release["tag_name"], "beta-v2.3.4")
        github.writes.clear()
        self.assertEqual(promote(github, github.run), "consumed")
        self.assertEqual(github.writes, [])

    def test_beta_from_non_owner_is_rejected(self):
        github = GitHubFixture().with_beta()
        github.run["actor"]["id"] = 8
        with self.assertRaisesRegex(ValueError, "owner"): promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_qualifying_intent_publishes_once_with_source_and_artifact_receipt(self):
        github = GitHubFixture().with_intent()
        self.assertEqual(promote(github, github.run), "published")
        self.assertFalse(github.release["draft"])
        self.assertFalse(github.release["prerelease"])
        self.assertEqual(github.release["target_commitish"], SHA)
        receipt = json.loads(github.assets["release-promotion.json"]["data"])
        self.assertEqual(receipt["intent"], INTENT)
        self.assertEqual(receipt["source"], SHA)
        self.assertEqual(len(receipt["files"]), 5)
        github.writes.clear()
        self.assertEqual(promote(github, github.run), "consumed")
        self.assertEqual(github.writes, [])

    def test_owner_merge_authorizes_exact_added_record(self):
        github = GitHubFixture().with_intent()
        self.assertEqual(authorize(github, github.run), github.intent)

    def test_deleted_and_reintroduced_intent_is_not_fresh(self):
        github = GitHubFixture().with_intent()
        github.history.append({"sha": "c" * 40})
        with self.assertRaisesRegex(ValueError, "reused"):
            authorize(github, github.run)
        self.assertEqual(github.writes, [])

    def test_failures_and_missing_evidence_never_start_publication(self):
        for change in ("failed", "skipped", "missing-job", "job-sha", "fork", "dispatch", "tag-run",
                       "wrong-workflow", "stale", "rules", "required-check", "artifact-sha", "artifact-run",
                       "artifact-digest", "expired", "missing-artifact", "duplicate-artifact", "forged-tag"):
            with self.subTest(change=change):
                github = GitHubFixture().with_intent()
                if change == "failed": github.run["conclusion"] = "failure"
                if change == "skipped": github.jobs[1]["conclusion"] = "skipped"
                if change == "missing-job": github.jobs.pop()
                if change == "job-sha": github.jobs[1]["head_sha"] = "b" * 40
                if change == "fork": github.run["head_repository"]["full_name"] = "attacker/fork"
                if change == "dispatch": github.run["event"] = "workflow_dispatch"
                if change == "tag-run": github.run["head_branch"] = "v2.3.4"
                if change == "wrong-workflow": github.run["path"] = ".github/workflows/forged.yml"
                if change == "stale": github.branch["commit"]["sha"] = "b" * 40
                if change == "rules": github.rules = []
                if change == "required-check": github.rules[-1]["parameters"]["required_status_checks"].pop()
                if change == "artifact-sha": github.artifacts[0]["workflow_run"]["head_sha"] = "b" * 40
                if change == "artifact-run": github.artifacts[0]["workflow_run"]["id"] = 99
                if change == "artifact-digest": github.artifacts[0]["digest"] = "sha256:" + "0" * 64
                if change == "expired": github.artifacts[0]["expired"] = True
                if change == "missing-artifact": github.artifacts.pop()
                if change == "duplicate-artifact": github.artifacts.append(copy.deepcopy(github.artifacts[0]))
                if change == "forged-tag": github.tag = {"object": {"type": "commit", "sha": "b" * 40}}
                with self.assertRaises(ValueError): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_lost_draft_upload_and_publish_responses_resume_without_replacement(self):
        for fault in ("draft", "upload", "publish"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                github.fail_create = fault == "draft"
                github.fail_upload = "rolauncher-v2.3.4-source.zip" if fault == "upload" else None
                github.ambiguous_publish = fault == "publish"
                with self.assertRaises(TimeoutError): promote(github, github.run)
                github.fail_upload = None
                github.writes.clear()
                self.assertEqual(promote(github, github.run), "consumed" if fault == "publish" else "published")
                self.assertFalse(github.release["draft"])
                self.assertFalse(any(method == "DELETE" or path == "/releases" for path, method, _ in github.writes))

    def test_conflicting_interrupted_draft_is_quarantined(self):
        github = GitHubFixture().with_intent()
        github.fail_upload = "rolauncher-v2.3.4-source.zip"
        with self.assertRaises(TimeoutError): promote(github, github.run)
        github.assets["rolauncher-v2.3.4-source.zip"]["digest"] = "sha256:" + "0" * 64
        github.writes.clear()
        with self.assertRaisesRegex(ValueError, "differs"): promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_rerun_started_during_upload_prevents_promotion(self):
        github = GitHubFixture().with_intent()
        original = github.__call__
        def racing_api(path, method="GET", body=None, **kwargs):
            result = original(path, method, body, **kwargs)
            if path.startswith("https://uploads.github.com/"):
                github.run["status"] = "in_progress"
            return result
        with self.assertRaises(ValueError): promote(racing_api, copy.deepcopy(github.run))
        self.assertTrue(github.release["draft"])

    def test_unprotected_version_tags_prevent_publication(self):
        github = GitHubFixture().with_intent()
        github.tag_rules["rules"] = []
        with self.assertRaisesRegex(ValueError, "tag rules"): promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_beta_creation_rule_is_required_to_fence_historical_tag_workflows(self):
        github = GitHubFixture().with_intent()
        github.tag_rules["rules"] = [{"type": "update"}, {"type": "deletion"}]
        with self.assertRaisesRegex(ValueError, "Historical beta"): promote(github, github.run)
        self.assertEqual(github.writes, [])

    def test_emergency_disable_during_upload_leaves_draft(self):
        github = GitHubFixture().with_intent()
        original = github.__call__
        def stopping_api(path, method="GET", body=None, **kwargs):
            result = original(path, method, body, **kwargs)
            if path.startswith("https://uploads.github.com/"): github.publisher_state = "disabled_manually"
            return result
        with self.assertRaisesRegex(ValueError, "disabled"): promote(stopping_api, github.run)
        self.assertTrue(github.release["draft"])

    def test_data_only_archives_reject_paths_duplicates_links_and_corruption(self):
        for fault in ("path", "duplicate", "link", "checksum"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                with zipfile.ZipFile(io.BytesIO(github.archives[30])) as original:
                    entries = [(info, original.read(info)) for info in original.infolist()]
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, "w") as archive:
                    for index, (info, data) in enumerate(entries):
                        if index == 0 and fault == "path": info.filename = "../publisher.py"
                        if index == 0 and fault == "link": info.external_attr = 0o120777 << 16
                        if index == 0 and fault == "checksum": data = b"tampered"
                        archive.writestr(info, data)
                    if fault == "duplicate":
                        # Duplicate entries must never become extracted publisher input.
                        info, data = entries[0]
                        with warnings.catch_warnings():
                            warnings.simplefilter("ignore", UserWarning)
                            archive.writestr(info, data)
                github.archives[30] = buffer.getvalue()
                github.artifacts[0]["digest"] = "sha256:" + hashlib.sha256(buffer.getvalue()).hexdigest()
                with self.assertRaises(ValueError): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_old_drafts_historical_releases_and_wrong_channels_are_preserved(self):
        for fault in ("old-marker", "wrong-source", "wrong-channel", "published-history"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                github.fail_create = True
                with self.assertRaises(TimeoutError): promote(github, github.run)
                if fault == "old-marker": github.release["body"] = "Legacy draft without intent receipt"
                if fault == "wrong-source": github.release["target_commitish"] = "b" * 40
                if fault == "wrong-channel": github.release["prerelease"] = True
                if fault == "published-history": github.release.update(draft=False, body="Historical GPL release")
                before = copy.deepcopy(github.release)
                github.writes.clear()
                with self.assertRaises(ValueError): promote(github, github.run)
                self.assertEqual(github.writes, [])
                self.assertEqual(github.release, before)

    def test_failed_linux_or_incomplete_api_evidence_denies(self):
        for fault in ("linux-failed", "linux-skipped", "linux-source", "missing-digest", "intent-version", "pagination"):
            with self.subTest(fault=fault):
                github = GitHubFixture().with_intent()
                if fault == "linux-failed": github.linux_run["conclusion"] = "failure"
                if fault == "linux-skipped": github.linux_jobs[1]["conclusion"] = "skipped"
                if fault == "linux-source": github.linux_run["head_sha"] = "b" * 40
                if fault == "missing-digest": github.artifacts[0].pop("digest")
                if fault == "intent-version": github.intent["version"] = "2.3.5"
                if fault == "pagination": github.files += [{"filename": f"docs/{i}.md", "status": "added"} for i in range(99)]
                with self.assertRaises(ValueError): promote(github, github.run)
                self.assertEqual(github.writes, [])

    def test_forged_event_and_rebuilt_attempt_cannot_reuse_authority(self):
        github = GitHubFixture().with_intent()
        event = dict(repository={"default_branch": "main"}, workflow_run=copy.deepcopy(github.run))
        event["workflow_run"]["run_attempt"] = 2
        with self.assertRaisesRegex(ValueError, "Stale"): promote_event(github, event)
        self.assertEqual(github.writes, [])

        github.fail_create = True
        with self.assertRaises(TimeoutError): promote(github, github.run)
        github.run["run_attempt"] = 2
        for artifact in github.artifacts:
            artifact["name"] = artifact["name"].replace("attempt-1", "attempt-2")
        github.writes.clear()
        # A rebuilt CI attempt is a different artifact identity, never a mutation retry.
        original = github.__call__
        def rebuilt_api(path, method="GET", body=None, **kwargs):
            return original(path.replace("/attempts/2/", "/attempts/1/"), method, body, **kwargs)
        with self.assertRaisesRegex(ValueError, "different intent"): promote(rebuilt_api, github.run)
        self.assertEqual(github.writes, [])

    def test_ambiguous_duplicate_json_fields_cannot_authorize(self):
        github = GitHubFixture().with_intent()
        github.intent_json = json.dumps(github.intent)[:-1] + ', "channel": "stable"}'
        with self.assertRaisesRegex(ValueError, "Duplicate"): authorize(github, github.run)
        self.assertEqual(github.writes, [])

    def test_artifacts_from_an_earlier_build_attempt_cannot_be_promoted(self):
        github = GitHubFixture().with_intent()
        github.run["run_attempt"] = 2
        original = github.__call__
        def later_attempt(path, method="GET", body=None, **kwargs):
            return original(path.replace("/attempts/2/", "/attempts/1/"), method, body, **kwargs)
        with self.assertRaisesRegex(ValueError, "artifact"): promote(later_attempt, github.run)
        self.assertEqual(github.writes, [])

    def test_ordinary_main_merge_never_publishes(self):
        github = GitHubFixture()
        self.assertEqual(promote(github, github.run), "no-intent")
        self.assertEqual(github.writes, [])

    def test_forged_owner_or_intent_is_denied_before_mutation(self):
        for change in ("merger", "branch", "source", "modified", "channel", "self-sha", "version", "id"):
            with self.subTest(change=change):
                github = GitHubFixture().with_intent()
                if change == "merger": github.pr["merged_by"]["id"] = 8
                if change == "branch": github.pr["head"]["ref"] = "feature/forged"
                if change == "source": github.pr["merge_commit_sha"] = "b" * 40
                if change == "modified": github.files[0]["status"] = "modified"
                if change == "channel": github.intent["channel"] = "beta"
                if change == "self-sha": github.intent["sha"] = SHA
                if change == "version": github.intent["version"] = "2.3.4-dev.123456abcdef"
                if change == "id": github.intent["id"] = "different"
                with self.assertRaises(ValueError):
                    authorize(github, github.run)
                self.assertEqual(github.writes, [])


if __name__ == "__main__":
    unittest.main()
