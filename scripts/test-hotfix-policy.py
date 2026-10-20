"""Hotfix authority tests use synthetic GitHub evidence, never live mutations."""
import base64
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import unittest
import zipfile

from release_promotion import authorize

spec = importlib.util.spec_from_file_location("promotion_tests", Path(__file__).with_name("test-release-promotion.py"))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
promote = reference.promote
SHA, PATH = reference.SHA, reference.PATH
BASELINE, BASE, REVIEWED, HEAD = (c * 40 for c in "cdef")
FILE = dict(filename="src/updates.rs", status="modified", sha="1" * 40,
            patch="@@ -1 +1 @@\n-old_bug();\n+fixed_bug();")


def comparison(base, head, commits, files):
    return dict(status="ahead", merge_base_commit={"sha": base}, base_commit={"sha": base},
                total_commits=len(commits), commits=commits, files=files)


class HotfixFixture(reference.GitHubFixture):
    def __init__(self):
        super().__init__()
        self.with_intent()
        self.pr.update(head={"ref": "hotfix/2.3.4", "sha": HEAD, "repo": {"full_name": reference.REPO}},
                       base={"ref": "main", "sha": BASE, "repo": {"full_name": reference.REPO}})
        self.baseline_release = dict(tag_name="v2.3.3", draft=False, prerelease=False)
        self.latest = self.baseline_release
        self.baseline_tag = dict(object={"type": "commit", "sha": BASELINE})
        self.review_files = [copy.deepcopy(FILE)]
        self.review_commits = [dict(sha=REVIEWED, parents=[{"sha": BASE}])]
        encoded = json.dumps(self.review_files, sort_keys=True, separators=(",", ":")).encode()
        self.intent.update(schema=2, source="hotfix", baseline_tag="v2.3.3", baseline_sha=BASELINE,
                           base_sha=BASE, reviewed_sha=REVIEWED, commits=[REVIEWED],
                           paths=["src/updates.rs"], patch_sha256=hashlib.sha256(encoded).hexdigest(),
                           review=dict(bug="Synthetic updater bug", dependencies="No dependency changes; prerequisites reviewed.",
                                       premium_excluded=True))
        self.head_commit = dict(sha=HEAD, parents=[{"sha": REVIEWED}],
                                files=[dict(filename=PATH, status="added")], commit={'tree': {'sha': '5' * 40}})
        self.merge_parents = [{"sha": BASE}, {"sha": HEAD}]
        self.merge_tree = '5' * 40
        self.history = [{'sha': HEAD}]
        self.governance_files = [dict(filename="scripts/release_promotion.py", status="added", sha="2" * 40,
                                      patch="@@ -0,0 +1 @@\n+# governance")]
        source = io.BytesIO()
        with zipfile.ZipFile(source, 'w') as archive: archive.writestr('AGENTS.md', b'Fixture instructions\n')
        payload = b'Fixture instructions\n'
        self.source_tree = dict(truncated=False, tree=[dict(path='AGENTS.md', type='blob', mode='100644',
                               sha=hashlib.sha1(b'blob 21\0' + payload).hexdigest())])
        with zipfile.ZipFile(io.BytesIO(self.archives[30])) as archive:
            entries = {n: archive.read(n) for n in archive.namelist()}
        entries['rolauncher-v2.3.4-source.zip'] = source.getvalue()
        entries['rolauncher-v2.3.4-SHA256SUMS.txt'] = ''.join(
            hashlib.sha256(data).hexdigest() + '  ' + n + '\n' for n, data in entries.items() if not n.endswith('SHA256SUMS.txt')).encode()
        outer = io.BytesIO()
        with zipfile.ZipFile(outer, 'w') as archive:
            for n, data in entries.items(): archive.writestr(n, data)
        self.archives[30] = outer.getvalue()
        self.artifacts[0].update(size_in_bytes=len(outer.getvalue()), digest='sha256:' + hashlib.sha256(outer.getvalue()).hexdigest())

    def __call__(self, path, method="GET", body=None, **kwargs):
        if method != "GET": return super().__call__(path, method, body, **kwargs)
        if path == f"/commits/{SHA}?per_page=100":
            return dict(files=self.files, parents=self.merge_parents, commit={'tree': {'sha': self.merge_tree}})
        if path == f"/commits/{HEAD}?per_page=100": return self.head_commit
        if path == f"/commits/{BASE}?per_page=100": return dict(files=self.governance_files)
        if path == f"/commits/{REVIEWED}?per_page=100":
            return dict(self.review_commits[0], files=self.review_files)
        if path == f'/git/trees/{SHA}?recursive=1': return self.source_tree
        if path == "/releases/latest": return self.latest
        if path == "/releases/tags/v2.3.3": return self.baseline_release
        if path == "/git/ref/tags/v2.3.3": return self.baseline_tag
        if path.startswith(f"/compare/{BASELINE}...{BASE}"):
            return comparison(BASELINE, BASE, [dict(sha=BASE, parents=[{"sha": BASELINE}])], self.governance_files)
        if path.startswith(f"/compare/{BASE}...{REVIEWED}"):
            return comparison(BASE, REVIEWED, self.review_commits, self.review_files)
        if path.startswith(f"/compare/{REVIEWED}...dev"): return dict(status="diverged")
        if path.startswith("/contents/") and f"?ref={BASE}" in path:
            texts = {"Cargo.toml": '[package]\nname="rolauncher"\nversion="2.3.3"\n',
                     "Cargo.lock": '[[package]]\nname="rolauncher"\nversion="2.3.3"\n'}
            name = path.split("/contents/")[1].split("?")[0]
            return dict(encoding="base64", content=base64.b64encode(texts[name].encode()).decode())
        return super().__call__(path, method, body, **kwargs)


SOURCE_MERGE = "8" * 40
INTENT_HEAD = "9" * 40
GOVERNANCE_HEAD = "7" * 40
RECOVERY_BASE = "6" * 40


class RecoveryFixture(HotfixFixture):
    """A follow-up intent PR bound to an already owner-merged hotfix PR."""
    def __init__(self):
        super().__init__()
        self.source_pr = copy.deepcopy(self.pr)
        self.review_commits[0]['commit'] = {'tree': {'sha': '5' * 40}}
        self.source_pr.update(merge_commit_sha=SOURCE_MERGE,
                              head={"ref": "hotfix/2.3.4", "sha": REVIEWED,
                                    "repo": {"full_name": reference.REPO}},
                              base={"ref": "main", "sha": BASE,
                                    "repo": {"full_name": reference.REPO}})
        self.pr.update(merge_commit_sha=SHA,
                       head={"ref": "hotfix-intent/2.3.4", "sha": INTENT_HEAD,
                             "repo": {"full_name": reference.REPO}},
                       base={"ref": "main", "sha": RECOVERY_BASE,
                             "repo": {"full_name": reference.REPO}})
        self.intent.update(schema=3, source="hotfix-recovery", source_pr=4,
                           source_merge_sha=SOURCE_MERGE)
        self.intent.pop("reviewed_sha", None)
        self.intent["reviewed_sha"] = REVIEWED
        self.intent_head_commit = dict(sha=INTENT_HEAD, parents=[{"sha": RECOVERY_BASE}],
                                       files=[dict(filename=PATH, status="added")],
                                       commit={"tree": {"sha": "6" * 40}})
        self.merge_parents = [{"sha": RECOVERY_BASE}, {"sha": INTENT_HEAD}]
        self.merge_tree = "6" * 40

    def __call__(self, path, method="GET", body=None, **kwargs):
        if method != "GET": return super().__call__(path, method, body, **kwargs)
        if path == f"/commits/{SHA}?per_page=100":
            return dict(files=self.files, parents=self.merge_parents,
                        commit={"tree": {"sha": self.merge_tree}})
        if path == f"/commits/{INTENT_HEAD}?per_page=100": return self.intent_head_commit
        if path == f"/commits/{SOURCE_MERGE}?per_page=100":
            return dict(parents=[{"sha": BASE}, {"sha": REVIEWED}],
                        commit={"tree": {"sha": "5" * 40}})
        if path == f"/commits/{GOVERNANCE_HEAD}?per_page=100":
            return dict(files=self.governance_files)
        if path == f"/commits/{RECOVERY_BASE}?per_page=100":
            return dict(parents=[{"sha": SOURCE_MERGE}, {"sha": GOVERNANCE_HEAD}],
                        files=self.governance_files, commit={"tree": {"sha": "3" * 40}})
        if path.startswith(f"/compare/{SOURCE_MERGE}...{GOVERNANCE_HEAD}"):
            commit = dict(sha=GOVERNANCE_HEAD, parents=[{"sha": SOURCE_MERGE}])
            return comparison(SOURCE_MERGE, GOVERNANCE_HEAD, [commit], self.governance_files)
        if path.startswith(f"/compare/{SOURCE_MERGE}...{RECOVERY_BASE}"):
            commits = [dict(sha=GOVERNANCE_HEAD, parents=[{"sha": SOURCE_MERGE}]),
                       dict(sha=RECOVERY_BASE, parents=[{"sha": SOURCE_MERGE}, {"sha": GOVERNANCE_HEAD}])]
            return comparison(SOURCE_MERGE, RECOVERY_BASE, commits, self.governance_files)
        if path == f"/commits/{SHA}/pulls?per_page=100": return [{"number": 5}]
        if path == "/pulls/4": return self.source_pr
        if path == "/pulls/5": return self.pr
        if path == f"/contents/{PATH}?ref={INTENT_HEAD}":
            return dict(encoding="base64", content=base64.b64encode(json.dumps(self.intent).encode()).decode())
        if path == f"/contents/{PATH}?ref={SHA}":
            return dict(encoding="base64", content=base64.b64encode(json.dumps(self.intent).encode()).decode())
        return super().__call__(path, method, body, **kwargs)


class HotfixTests(unittest.TestCase):
    def test_exact_windows_sys_features_for_process_checks_do_not_change_dependency_versions(self):
        api = HotfixFixture()
        baseline = '''[package]\nname = "rolauncher"\nversion = "2.3.3"\n\n[target."cfg(windows)".dependencies]\nwindows-sys = { version = "0.61", features = ["Win32_Foundation"] }\n'''
        reviewed = '''[package]\nname = "rolauncher"\nversion = "2.3.4"\n\n[target."cfg(windows)".dependencies]\nwindows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Diagnostics_Debug", "Win32_System_Diagnostics_ToolHelp", "Win32_System_RemoteDesktop"] }\n'''
        original = api.__call__
        def evidence(path, method='GET', body=None, **kwargs):
            if path == f'/contents/Cargo.toml?ref={BASE}':
                return dict(encoding='base64', content=base64.b64encode(baseline.encode()).decode())
            if path == f'/contents/Cargo.toml?ref={SHA}':
                return dict(encoding='base64', content=base64.b64encode(reviewed.encode()).decode())
            return original(path, method, body, **kwargs)

        self.assertEqual(promote(evidence, api.run), 'published')

    def test_other_windows_sys_feature_or_version_changes_still_fail_closed(self):
        baseline = '''[package]\nname = "rolauncher"\nversion = "2.3.3"\n\n[target."cfg(windows)".dependencies]\nwindows-sys = { version = "0.61", features = ["Win32_Foundation"] }\n'''
        variants = (
            '''[package]\nname = "rolauncher"\nversion = "2.3.4"\n\n[target."cfg(windows)".dependencies]\nwindows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Memory"] }\n''',
            '''[package]\nname = "rolauncher"\nversion = "2.3.4"\n\n[target."cfg(windows)".dependencies]\nwindows-sys = { version = "0.62", features = ["Win32_Foundation"] }\n''',
        )
        for reviewed in variants:
            with self.subTest(reviewed=reviewed):
                api = HotfixFixture()
                original = api.__call__
                def evidence(path, method='GET', body=None, **kwargs):
                    if path == f'/contents/Cargo.toml?ref={BASE}':
                        return dict(encoding='base64', content=base64.b64encode(baseline.encode()).decode())
                    if path == f'/contents/Cargo.toml?ref={SHA}':
                        return dict(encoding='base64', content=base64.b64encode(reviewed.encode()).decode())
                    return original(path, method, body, **kwargs)
                with self.assertRaises(ValueError): promote(evidence, api.run)
                self.assertEqual(api.writes, [])

    def test_net_diff_cannot_hide_experimental_changes_in_an_intermediate_commit(self):
        api = HotfixFixture()
        original = api.__call__
        def evidence(path, method='GET', body=None, **kwargs):
            if path == f'/commits/{REVIEWED}?per_page=100':
                return dict(api.review_commits[0], files=[dict(FILE, filename='src/identity_poc/mod.rs')])
            return original(path, method, body, **kwargs)
        with self.assertRaisesRegex(ValueError, 'Premium'):
            authorize(evidence, api.run)
        self.assertEqual(api.writes, [])

    def test_denials_preserve_every_tag_release_and_asset(self):
        faults = ('owner', 'branch', 'foreign', 'old-schema', 'baseline-tag', 'stale-baseline', 'unpublished',
                  'not-patch', 'patch-hash', 'extra-commit', 'nonlinear', 'unexpected-path', 'missing-patch',
                  'dependencies', 'premium-review', 'intent-plus-code', 'merge-parents', 'replayed',
                  'dev-commit', 'governance-application', 'missing-windows', 'failed-linux', 'artifact-source',
                  'source-bytes', 'hidden-tag-bypass', 'missing-rules', 'dependency-upgrade')
        for fault in faults:
            with self.subTest(fault=fault):
                api = HotfixFixture()
                if fault == 'owner': api.pr['merged_by']['id'] = 8
                if fault == 'branch': api.pr['head']['ref'] = 'codex/forged-hotfix'
                if fault == 'foreign': api.pr['head']['repo']['full_name'] = 'attacker/fork'
                if fault == 'old-schema': api.intent = dict(schema=1, id=reference.INTENT, version='2.3.4', channel='stable')
                if fault == 'baseline-tag': api.baseline_tag['object']['sha'] = '8' * 40
                if fault == 'stale-baseline': api.latest = dict(tag_name='v2.4.0', draft=False, prerelease=False)
                if fault == 'unpublished': api.baseline_release['draft'] = True
                if fault == 'not-patch': api.intent['version'] = '2.4.0'
                if fault == 'patch-hash': api.intent['patch_sha256'] = '0' * 64
                if fault == 'extra-commit': api.intent['commits'].append('8' * 40)
                if fault == 'nonlinear': api.review_commits[0]['parents'].append({'sha': '8' * 40})
                if fault == 'unexpected-path': api.review_files[0]['filename'] = 'scripts/build.ps1'
                if fault == 'missing-patch': api.review_files[0].pop('patch')
                if fault == 'dependencies': api.intent['review']['dependencies'] = ''
                if fault == 'premium-review': api.intent['review']['premium_excluded'] = False
                if fault == 'intent-plus-code': api.head_commit['files'].append(dict(filename='src/lib.rs', status='modified'))
                if fault == 'merge-parents': api.merge_parents.reverse()
                if fault == 'replayed': api.history.append({'sha': '8' * 40})
                if fault == 'governance-application': api.governance_files[0]['filename'] = 'src/identity_poc/mod.rs'
                if fault == 'missing-windows': api.jobs = [j for j in api.jobs if j['name'] != 'Windows checks']
                if fault == 'failed-linux': api.linux_jobs[0]['conclusion'] = 'failure'
                if fault == 'artifact-source': api.artifacts[0]['workflow_run']['head_sha'] = '8' * 40
                if fault == 'source-bytes': api.source_tree['tree'][0]['sha'] = '0' * 40
                if fault == 'hidden-tag-bypass': api.tag_rules.pop('bypass_actors')
                if fault == 'missing-rules': api.rules = []
                original = api.__call__
                def evidence(path, method='GET', body=None, **kwargs):
                    if fault == 'dev-commit' and path.startswith(f'/compare/{REVIEWED}...dev'):
                        return {'status': 'ahead'}
                    if fault == 'dependency-upgrade' and path == f'/contents/Cargo.toml?ref={SHA}':
                        text = '[package]\nname="rolauncher"\nversion="2.3.4"\n[dependencies]\nnew_premium_library="1"\n'
                        return dict(encoding='base64', content=base64.b64encode(text.encode()).decode())
                    return original(path, method, body, **kwargs)
                with self.assertRaises(ValueError): promote(evidence, api.run)
                self.assertEqual(api.writes, [])

    def test_missing_intent_never_turns_hotfix_branch_or_successful_ci_into_authority(self):
        api = HotfixFixture()
        api.files = [dict(filename='src/updates.rs', status='modified')]
        for _ in range(2): self.assertEqual(promote(api, api.run), 'no-intent')
        self.assertEqual(api.writes, [])

    def test_migration_workflows_stay_read_only_and_publisher_requires_owner_guards(self):
        root = Path(__file__).resolve().parent.parent
        for file in ('ci-release.yml', 'linux.yml', 'beta-release.yml'):
            workflow = (root / '.github/workflows' / file).read_text()
            self.assertNotIn('contents: write', workflow)
            self.assertNotIn('scripts/publish-', workflow)
            self.assertNotIn('pull_request_target', workflow)
        workflow = (root / '.github/workflows/release-promotion.yml').read_text()
        self.assertNotIn('false &&', workflow)
        self.assertIn("vars.RELEASE_PROMOTION_ENABLED == 'true'", workflow)
        self.assertIn("github.repository == 'oangsa/RoLauncher'", workflow)
        self.assertIn("github.event.workflow_run.event == 'push'", workflow)
        self.assertIn("github.event.workflow_run.conclusion == 'success'", workflow)
        self.assertIn('github.event.workflow_run.head_repository.full_name == github.repository', workflow)
        self.assertIn("(github.event.workflow_run.head_branch == 'main' || github.event.workflow_run.name == 'Beta release')", workflow)
        self.assertIn('ref: ${{ github.sha }}', workflow)
        self.assertNotIn('ref: ${{ github.event.workflow_run.head_sha }}', workflow)

    def test_branch_names_grant_admission_only_after_scope_evidence(self):
        from hotfix_policy import check_branch_flow
        for fault in ('fork', 'branch', 'application', 'intent', 'version'):
            with self.subTest(fault=fault):
                api = HotfixFixture()
                event = dict(pull_request=dict(head=dict(ref='codex/hotfix-release-lane', sha=BASE, repo={'full_name': reference.REPO}),
                                               base=dict(ref='main', sha=BASELINE)))
                if fault == 'fork': event['pull_request']['head']['repo']['full_name'] = 'attacker/fork'
                if fault == 'branch': event['pull_request']['head']['ref'] = 'codex/forged'
                if fault == 'application': api.governance_files[0]['filename'] = 'src/lib.rs'
                if fault == 'intent': api.governance_files[0]['filename'] = PATH
                original = api.__call__
                def evidence(path, method='GET', body=None, **kwargs):
                    if fault == 'version' and path == f'/contents/Cargo.toml?ref={BASE}':
                        return dict(encoding='base64', content=base64.b64encode(b'[package]\nversion="2.3.4"\n').decode())
                    return original(path, method, body, **kwargs)
                with self.assertRaises(ValueError): check_branch_flow(event, evidence)
                self.assertEqual(api.writes, [])

    def test_hotfix_cannot_publish_with_unverified_ruleset_bypasses(self):
        api = HotfixFixture()
        api.branch_ruleset.pop('bypass_actors')
        with self.assertRaisesRegex(ValueError, 'Unverified'):
            promote(api, api.run)
        self.assertEqual(api.writes, [])

    def test_preparation_branch_can_be_reviewed_against_main_without_release_authority(self):
        from hotfix_policy import check_branch_flow
        api = HotfixFixture()
        event = dict(pull_request=dict(head=dict(ref='codex/hotfix-release-lane', sha=BASE, repo={'full_name': reference.REPO}),
                                       base=dict(ref='main', sha=BASELINE)))
        self.assertEqual(check_branch_flow(event, api), 'governance-only')
        self.assertEqual(api.writes, [])

    def test_owner_evidence_enrollment_only_accepts_governance_delta_on_current_main(self):
        from hotfix_policy import check_branch_flow
        api = HotfixFixture()
        branch = 'codex/phase-d-owner-evidence-enrollment-20261020'
        api.pr.update(head=dict(ref=branch, sha=HEAD, repo={'full_name': reference.REPO}),
                      base=dict(ref='main', sha=BASE, repo={'full_name': reference.REPO}))
        files = [dict(filename='.github/owner-evidence-policy.json', status='modified', sha='2' * 40,
                      patch='@@ -1 +1 @@\n-old\n+new')]

        def call(path, *args, **kwargs):
            if path.startswith(f'/compare/{BASE}...{HEAD}'):
                return comparison(BASE, HEAD, [dict(sha=HEAD, parents=[{'sha': BASE}])], files)
            if path == f'/commits/{HEAD}?per_page=100':
                return dict(files=files, parents=[{'sha': BASE}])
            return api(path, *args, **kwargs)

        event = dict(pull_request=dict(head=dict(ref=branch, sha=HEAD, repo={'full_name': reference.REPO}),
                                       base=dict(ref='main', sha=BASE, repo={'full_name': reference.REPO})))
        self.assertEqual(check_branch_flow(event, call), 'governance-only')
        self.assertEqual(api.writes, [])

        files[0]['filename'] = 'src/updates.rs'
        with self.assertRaisesRegex(ValueError, 'Non-governance change'):
            check_branch_flow(event, call)
        self.assertEqual(api.writes, [])

    def test_hotfix_pr_reports_reviewed_scope_before_owner_authorization(self):
        from hotfix_policy import check_branch_flow
        for head in (REVIEWED, HEAD):
            with self.subTest(head=head):
                api = HotfixFixture()
                event = dict(pull_request=dict(head=dict(ref='hotfix/2.3.4', sha=head, repo={'full_name': reference.REPO}),
                                               base=dict(ref='main', sha=BASE)))
                report = json.loads(check_branch_flow(event, api))
                self.assertEqual(report['commits'], [REVIEWED])
                self.assertEqual(report['paths'], ['src/updates.rs'])
                self.assertIn('cherry-pick prerequisites', report['semantic_review'])
                self.assertEqual(api.writes, [])

    def test_hotfix_publishes_once_and_reconciles_interruption(self):
        for interrupted in (False, True):
            with self.subTest(interrupted=interrupted):
                api = HotfixFixture()
                if interrupted:
                    api.fail_upload = 'rolauncher-v2.3.4-source.zip'
                    with self.assertRaises(TimeoutError): promote(api, api.run)
                    api.fail_upload = None
                    api.writes.clear()
                self.assertEqual(promote(api, api.run), 'published')
                self.assertFalse(any(method == 'DELETE' for _, method, _ in api.writes))
                api.latest = dict(tag_name='v2.3.4', draft=False, prerelease=False)
                api.writes.clear()
                self.assertEqual(promote(api, api.run), 'consumed')
                self.assertEqual(api.writes, [])

    def test_merge_cannot_change_owner_reviewed_tree(self):
        api = HotfixFixture()
        api.merge_tree = '6' * 40
        with self.assertRaisesRegex(ValueError, 'tree'):
            authorize(api, api.run)
        self.assertEqual(api.writes, [])

    def test_source_archive_bytes_must_match_exact_source_tree(self):
        from hotfix_policy import verify_source
        data = b'published stable source\n'
        blob = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        tree = dict(truncated=False, tree=[dict(path='AGENTS.md', type='blob', mode='100644', sha=blob)])
        def api(path):
            self.assertEqual(path, f'/git/trees/{SHA}?recursive=1')
            return tree
        for payload in (data, b'tampered source\n'):
            with self.subTest(payload=payload):
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, 'w') as archive: archive.writestr('AGENTS.md', payload)
                if payload == data:
                    verify_source(api, SHA, buffer.getvalue())
                else:
                    with self.assertRaisesRegex(ValueError, 'Source blob'):
                        verify_source(api, SHA, buffer.getvalue())

    def test_semantically_unreviewed_or_incomplete_patch_cannot_publish(self):
        api = HotfixFixture()
        api.review_files[0]['patch'] += '\n+machine_key::provision();'
        # Even rehashing an excluded patch must not grant release authority.
        api.intent['patch_sha256'] = hashlib.sha256(json.dumps(api.review_files, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        with self.assertRaisesRegex(ValueError, 'Premium'):
            authorize(api, api.run)
        self.assertEqual(api.writes, [])

    def test_governance_merge_history_can_be_a_base_without_importing_application_changes(self):
        api = HotfixFixture()
        original = api.__call__
        side = '9' * 40
        def evidence(path, method='GET', body=None, **kwargs):
            if path.startswith(f'/compare/{BASELINE}...{BASE}'):
                return comparison(BASELINE, BASE, [dict(sha=side, parents=[{'sha': BASELINE}]),
                                                  dict(sha=BASE, parents=[{'sha': BASELINE}, {'sha': side}])], api.governance_files)
            if path in (f'/commits/{side}?per_page=100', f'/commits/{BASE}?per_page=100'):
                return dict(files=api.governance_files)
            return original(path, method, body, **kwargs)
        self.assertEqual(authorize(evidence, api.run), api.intent)

    def test_owner_merge_authorizes_isolated_patch_without_dev_merge(self):
        api = HotfixFixture()
        self.assertEqual(authorize(api, api.run), api.intent)
        self.assertEqual(api.writes, [])

    def test_recovery_intent_promotes_from_exact_already_merged_hotfix(self):
        api = RecoveryFixture()
        self.assertEqual(promote(api, api.run), 'published')
        receipt = json.loads(api.assets['release-promotion.json']['data'])
        self.assertEqual(receipt['intent'], reference.INTENT)
        self.assertEqual(receipt['source'], SHA)
        self.assertEqual(len(receipt['files']), 5)

    def test_recovery_intent_branch_flow_admits_only_intent_commit(self):
        from hotfix_policy import check_branch_flow
        api = RecoveryFixture()
        event = dict(pull_request=dict(
            head=dict(ref='hotfix-intent/2.3.4', sha=INTENT_HEAD,
                      repo={'full_name': reference.REPO}),
            base=dict(ref='main', sha=RECOVERY_BASE, repo={'full_name': reference.REPO})))
        self.assertIn('semantic_review', check_branch_flow(event, api))
        api.intent_head_commit['files'].append(dict(filename='src/updates.rs', status='modified'))
        with self.assertRaises(ValueError): check_branch_flow(event, api)

    def test_recovery_authority_rejects_source_and_binding_drift_before_writes(self):
        for fault in ('source-owner', 'source-head', 'source-tree', 'recovery-base',
                      'governance-drift', 'intent-patch', 'intent-commit'):
            with self.subTest(fault=fault):
                api = RecoveryFixture()
                if fault == 'source-owner': api.source_pr['merged_by']['id'] = 8
                if fault == 'source-head': api.source_pr['head']['sha'] = '7' * 40
                if fault == 'source-tree': api.review_commits[0]['commit']['tree']['sha'] = '7' * 40
                if fault == 'recovery-base': api.pr['base']['sha'] = '7' * 40
                if fault == 'governance-drift': api.governance_files[0]['filename'] = 'src/updates.rs'
                if fault == 'intent-patch': api.intent['patch_sha256'] = '0' * 64
                if fault == 'intent-commit': api.intent_head_commit['files'].append(
                    dict(filename='src/updates.rs', status='modified'))
                with self.assertRaises(ValueError): promote(api, api.run)
                self.assertEqual(api.writes, [])

    def test_recovery_governance_lane_is_based_on_current_hotfix_and_scope_limited(self):
        from hotfix_policy import check_branch_flow
        api = RecoveryFixture()
        event = dict(pull_request=dict(
            head=dict(ref='codex/hotfix-recovery-governance', sha=GOVERNANCE_HEAD,
                      repo={'full_name': reference.REPO}),
            base=dict(ref='main', sha=SOURCE_MERGE, repo={'full_name': reference.REPO})))
        self.assertEqual(check_branch_flow(event, api), 'governance-only')
        api.governance_files[0]['filename'] = 'src/updates.rs'
        with self.assertRaisesRegex(ValueError, 'Non-governance'): check_branch_flow(event, api)


if __name__ == "__main__":
    unittest.main()

