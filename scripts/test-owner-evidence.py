"""Synthetic production integration tests. Temporary test keys; no network or enrollment."""
import base64
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch
import zipfile

import owner_evidence as evidence
import release_promotion as promotion

spec = importlib.util.spec_from_file_location('promotion_reference', Path(__file__).with_name('test-release-promotion.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


class LiveFixture(reference.GitHubFixture):
    def __init__(self):
        super().__init__()
        self.with_intent()
        self.pr['merged_by']['id'] = evidence.OWNER
        self.comments = []
        for item in (self.branch_ruleset, self.tag_rules):
            item.update(source=evidence.REPO, source_type='Repository')
        self.branch_ruleset.update(conditions={'ref_name': {'include': ['refs/heads/main', 'refs/heads/dev'], 'exclude': []}},
                                   rules=[{k: v for k, v in r.items() if k != 'ruleset_id'} for r in self.rules])

    def full(self, path):
        if path.startswith('/rulesets?'):
            return [evidence.project(r, evidence.SUMMARY) for r in (self.branch_ruleset, self.tag_rules)]
        if path in ('/rulesets/1', '/rulesets/2'):
            return copy.deepcopy(self.tag_rules if path.endswith('/1') else self.branch_ruleset)
        if path == '':
            return dict(id=evidence.REPO_ID, full_name=evidence.REPO, default_branch='main',
                        owner=dict(id=evidence.OWNER, login='oangsa', type='User'))
        return super().__call__(path)

    def __call__(self, path, method='GET', body=None, **kwargs):
        if method != 'GET':
            return super().__call__(path, method, body, **kwargs)
        if path.startswith('/issues/1/comments?'):
            return copy.deepcopy(self.comments)
        if path.startswith('/issues/comments/'):
            return copy.deepcopy(next(c for c in self.comments if c['id'] == int(path.split('/')[-1])))
        if path.startswith('/rulesets') or path == '':
            result = self.full(path)
            if isinstance(result, dict):
                result.pop('bypass_actors', None)  # Model runtime's real omitted field.
            return result
        return super().__call__(path, method, body, **kwargs)


class AutomaticEvidenceTests(unittest.TestCase):
    def test_publishes_and_reconciles_without_local_signatures(self):
        api = LiveFixture()
        for expected in ('published', 'consumed'):
            transport = evidence.AutomaticEvidence(api.full)
            self.assertEqual(promotion.promote(api, api.run, transport), expected)
        self.assertTrue(api.release['immutable'])

    def test_admission_never_writes(self):
        api = LiveFixture()
        with self.assertRaises(evidence.ChallengeReady):
            promotion.promote(api, api.run, evidence.AdmissionOnly())
        self.assertEqual(api.writes, [])

    def test_no_intent_never_reads_app_controls(self):
        api = LiveFixture()
        api.files = []
        transport = evidence.AutomaticEvidence(lambda path: self.fail('App called without release intent'))
        self.assertEqual(promotion.promote(api, api.run, transport), 'no-intent')
        self.assertEqual(api.writes, [])

    def test_final_gate_denial_leaves_staged_release_unpublished(self):
        api = LiveFixture()
        def app(path):
            if path == '/immutable-releases' and api.release is not None:
                return dict(enabled=False, enforced_by_owner=False)
            return api.full(path)
        with self.assertRaisesRegex(ValueError, 'immutability disabled'):
            promotion.promote(api, api.run, evidence.AutomaticEvidence(app))
        self.assertTrue(api.release['draft'])
        self.assertFalse(any(method == 'PATCH' for _, method, _ in api.writes))

    def test_missing_bypasses_wrong_repo_and_disabled_immutability_deny_before_writes(self):
        for fault in ('bypasses', 'repository', 'immutable', 'permission'):
            with self.subTest(fault=fault):
                api = LiveFixture()
                def app(path):
                    if fault == 'permission':
                        raise RuntimeError('403')
                    result = api.full(path)
                    if fault == 'bypasses' and path == '/rulesets/2':
                        result.pop('bypass_actors')
                    if fault == 'repository' and path == '':
                        result['id'] = 1
                    if fault == 'immutable' and path == '/immutable-releases':
                        result = dict(enabled=False, enforced_by_owner=False)
                    return result
                with self.assertRaises((ValueError, RuntimeError)):
                    promotion.promote(api, api.run, evidence.AutomaticEvidence(app))
                self.assertEqual(api.writes, [])

    def test_app_adapter_denies_mutations_and_unrelated_reads(self):
        transport = evidence.AutomaticEvidence(lambda path: self.fail('Unexpected API request'))
        for path, args in (('/releases', ()), ('/rulesets/2', ('PATCH',)),
                           ('https://example.com', ()), ('/issues/35', ())):
            with self.assertRaises(ValueError):
                transport.read(path, *args)

    def test_slow_upload_refreshes_and_changed_controls_stop_publication(self):
        api = LiveFixture()
        transport = evidence.AutomaticEvidence(api.full)
        transport.gate('preflight', api, {'source': 'test'})
        transport.checked -= 46
        api.branch_ruleset['bypass_actors'] = [dict(actor_type='RepositoryRole', actor_id=5, bypass_mode='always')]
        with self.assertRaisesRegex(ValueError, 'bypasses must be empty'):
            transport.fresh()
        with self.assertRaisesRegex(ValueError, 'binding changed'):
            transport.gate('final', api, {'source': 'changed'})


class OwnerEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.key = Path(cls.temp.name) / 'ephemeral-unit-test'
        subprocess.run([evidence.ssh(), '-q', '-t', 'ed25519', '-N', '', '-f', str(cls.key)], check=True)
        public = Path(str(cls.key) + '.pub').read_text().split()
        cls.public = ' '.join(public[:2])
        cls.other_key = Path(cls.temp.name) / 'unapproved-unit-test'
        subprocess.run([evidence.ssh(), '-q', '-t', 'ed25519', '-N', '', '-f', str(cls.other_key)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.policy_path = self.root / 'policy.json'
        self.policy = dict(schema=1, repository_id=evidence.REPO_ID, owner_id=evidence.OWNER,
                           inbox=1, public_disclosure_approved=True, keys=[self.public], revoked=[])
        self.write_policy()
        self.env = patch.dict(os.environ, GITHUB_SHA='d' * 40, GITHUB_RUN_ID='99', GITHUB_RUN_ATTEMPT='1',
                              GITHUB_REPOSITORY=evidence.REPO, GITHUB_REPOSITORY_ID=str(evidence.REPO_ID),
                              GITHUB_EVENT_NAME='workflow_run',
                              GITHUB_WORKFLOW_REF=evidence.REPO + '/' + evidence.WORKFLOW + '@refs/heads/main')
        self.env.start()
        self.addCleanup(self.env.stop)
        self.api = LiveFixture()
        self.policy['disclosure_sha256'] = evidence.digest(evidence.collect(self.api.full))
        self.write_policy()
        self.state = self.root / 'state'

    def write_policy(self):
        self.policy_path.write_bytes(evidence.canonical(self.policy))

    def transport(self, stage):
        return evidence.StagedEvidence(stage, self.policy_path, self.state)

    def prepare(self):
        before = list(self.api.writes)
        with self.assertRaises(evidence.ChallengeReady):
            promotion.promote(self.api, self.api.run, self.transport('prepare'))
        self.assertEqual(self.api.writes, before)
        return self.challenge('preflight')

    def challenge(self, phase):
        return evidence.parse((self.state / phase / 'challenge.json').read_bytes())

    def payload(self, challenge):
        now = int(time.time())
        return dict(schema=1, challenge=challenge, collection_start=now, collection_end=now,
                    expires=now + evidence.TTL, controls=evidence.collect(self.api.full))

    def response(self, challenge, change=None):
        payload = self.payload(challenge)
        if change:
            change(payload)
        return evidence.sign(payload, self.key)

    def post(self, challenge, response=None):
        wrapper = dict(challenge_sha256=evidence.digest(challenge), response=response or self.response(challenge))
        comment = dict(id=100 + len(self.api.comments), user={'id': evidence.OWNER},
                       body=evidence.canonical(wrapper).decode(), created_at='2026-10-17T00:00:00Z',
                       updated_at='2026-10-17T00:00:00Z')
        self.api.comments.append(comment)
        return comment

    def stage(self):
        with self.assertRaises(evidence.ChallengeReady):
            promotion.promote(self.api, self.api.run, self.transport('stage'))
        return self.challenge('final')

    def complete(self):
        self.post(self.prepare())
        self.post(self.stage())
        return promotion.promote(self.api, self.api.run, self.transport('finish'))

    def test_both_live_gates_and_immutable_readback(self):
        self.assertEqual(self.complete(), 'published')
        pre, final = (self.challenge(p) for p in ('preflight', 'final'))
        self.assertNotEqual(pre['nonce'], final['nonce'])
        self.assertEqual(pre['binding'], final['binding'])
        self.assertEqual(pre['binding']['intent'], self.api.intent)
        self.assertEqual(pre['binding']['linux']['id'], 13)
        self.assertTrue(self.api.release['immutable'])

    def test_signed_source_run_attempt_nonce_phase_policy_and_trusted_sha_binding(self):
        c = self.prepare()
        for field in ('trusted_sha', 'repository', 'repository_id', 'owner_id', 'workflow', 'run_id',
                      'attempt', 'phase', 'nonce', 'policy_sha256', 'inbox', 'binding'):
            with self.subTest(field=field):
                def change(value):
                    value['challenge'][field] = 'wrong'
                response = self.response(copy.deepcopy(c), change)
                with self.assertRaises(ValueError):
                    evidence.verify(response, c, set(), self.policy_path)
        self.assertEqual(self.api.writes, [])

    def test_invalid_tampered_revoked_and_wrong_namespace_signatures(self):
        c = self.prepare()
        valid = self.response(c)
        unapproved = evidence.sign(self.payload(c), self.other_key)
        with self.assertRaisesRegex(ValueError, 'signature'):
            evidence.verify(unapproved, c, set(), self.policy_path)
        tampered = copy.deepcopy(valid)
        raw = bytearray(base64.b64decode(''.join(tampered['signature'].strip().splitlines()[1:-1])))
        raw[-1] ^= 1
        tampered['signature'] = '-----BEGIN SSH SIGNATURE-----\n' + base64.b64encode(raw).decode() + '\n-----END SSH SIGNATURE-----\n'
        with self.assertRaisesRegex(ValueError, 'signature'):
            evidence.verify(tampered, c, set(), self.policy_path)
        with patch.object(evidence, 'NAMESPACE', 'wrong-namespace'):
            wrong = self.response(c)
        with self.assertRaisesRegex(ValueError, 'signature'):
            evidence.verify(wrong, c, set(), self.policy_path)
        self.policy['revoked'] = ['SHA256:' + base64.b64encode(hashlib.sha256(
            base64.b64decode(self.public.split()[1])).digest()).decode().rstrip('=')]
        self.write_policy()
        with self.assertRaisesRegex(ValueError, 'revoked'):
            evidence.verify(valid, c, set(), self.policy_path)
        self.assertEqual(self.api.writes, [])

    def test_freshness_expiry_future_slow_and_replay(self):
        c = self.prepare()
        valid = self.response(c)
        now = int(time.time())
        for fault in ('expired', 'future', 'slow', 'boolean'):
            def change(value):
                if fault == 'expired':
                    value['collection_start'] -= 200
                    value['collection_end'] -= 200
                    value['expires'] -= 200
                if fault == 'future': value['collection_end'] += 60
                if fault == 'slow': value['collection_end'] += 46
                if fault == 'boolean': value['collection_start'] = True
            with self.subTest(fault=fault), self.assertRaises(ValueError):
                evidence.verify(self.response(c, change), c, set(), self.policy_path, now)
        used = set()
        evidence.verify(valid, c, used, self.policy_path)
        with self.assertRaisesRegex(ValueError, 'replay'):
            evidence.verify(valid, c, used, self.policy_path)
        with self.assertRaisesRegex(ValueError, 'Expired'):
            evidence.verify(valid, c, set(), self.policy_path, now + evidence.TTL + 1)

    def test_incomplete_live_inventory_missing_bypasses_and_unauthorized_actors(self):
        for fault in ('missing-array', 'branch-actor', 'stable-actor', 'inventory', 'metadata', 'branch-rules'):
            with self.subTest(fault=fault):
                c = self.prepare()
                def change(value):
                    controls = value['controls']
                    if fault == 'missing-array': controls['rulesets'][0].pop('bypass_actors')
                    if fault == 'branch-actor': controls['rulesets'][1]['bypass_actors'] = [{'actor_type': 'User', 'actor_id': evidence.OWNER}]
                    if fault == 'stable-actor': controls['rulesets'][0]['bypass_actors'] = [{'actor_type': 'RepositoryRole', 'actor_id': 5, 'bypass_mode': 'always'}]
                    if fault == 'inventory': controls['inventory'].pop()
                    if fault == 'metadata': controls['rulesets'][0]['enforcement'] = 'disabled'
                    if fault == 'branch-rules': controls['branches']['dev'].pop()
                self.post(c, self.response(c, change))
                with self.assertRaises(ValueError):
                    promotion.promote(self.api, self.api.run, self.transport('stage'))
                self.assertEqual(self.api.writes, [])
                self.api.comments.clear()
                (self.state / 'preflight/challenge.json').unlink()

    def test_immutability_disabled_missing_at_either_gate_and_consumed_readback(self):
        for phase in ('preflight', 'final'):
            for missing in (False, True):
                with self.subTest(phase=phase, missing=missing):
                    c = self.prepare()
                    if phase == 'final':
                        self.post(c)
                        c = self.stage()
                    def change(value):
                        value['controls']['immutable'] = {} if missing else {'enabled': False, 'enforced_by_owner': False}
                    self.post(c, self.response(c, change))
                    before = list(self.api.writes)
                    with self.assertRaises(ValueError):
                        promotion.promote(self.api, self.api.run, self.transport('stage' if phase == 'preflight' else 'finish'))
                    self.assertEqual(self.api.writes, before)
                    self.api = LiveFixture()
                    self.state = self.root / ('state-' + phase + str(missing))
        self.api = LiveFixture()
        self.state = self.root / 'readback'
        self.assertEqual(self.complete(), 'published')
        self.api.release.pop('immutable')
        self.state = self.root / 'consumed'
        with self.assertRaisesRegex(ValueError, 'immutable'):
            self.complete()

    def test_edited_duplicate_conflicting_and_timeout_responses_zero_writes(self):
        c = self.prepare()
        for fault in ('edited', 'duplicate', 'conflict', 'wrong-owner', 'timeout'):
            self.api.comments.clear()
            comment = self.post(c)
            if fault == 'edited': comment['updated_at'] = 'edited'
            if fault in ('duplicate', 'conflict'):
                self.post(c)
            if fault in ('wrong-owner', 'timeout'):
                comment['user']['id'] += 1
            with self.subTest(fault=fault), patch.object(evidence.time, 'monotonic', side_effect=[0, 1, 2000]), patch.object(evidence.time, 'sleep'):
                with self.assertRaises(ValueError):
                    promotion.promote(self.api, self.api.run, self.transport('stage'))
            self.assertEqual(self.api.writes, [])

    def test_final_rejects_preflight_replay_and_edited_accepted_comment(self):
        pre = self.prepare()
        posted = self.post(pre)
        final = self.stage()
        self.post(final, evidence.parse(posted['body'].encode())['response'])
        writes = list(self.api.writes)
        with self.assertRaisesRegex(ValueError, 'binding'):
            promotion.promote(self.api, self.api.run, self.transport('finish'))
        self.assertEqual(self.api.writes, writes)
        transport = self.transport('stage')
        self.api.comments.pop()
        (self.state / 'preflight-accepted.json').unlink()
        transport.gate('preflight', self.api, pre['binding'])
        posted['updated_at'] = 'edited'
        with self.assertRaisesRegex(ValueError, 'edited'):
            transport.fresh()

    def test_lost_publish_response_rerun_consumed_same_receipt_and_no_writes(self):
        self.api.ambiguous_publish = True
        with self.assertRaises(TimeoutError):
            self.complete()
        receipt = self.api.assets['release-promotion.json']['data']
        writes = list(self.api.writes)
        self.state = self.root / 'rerun'
        os.environ['GITHUB_RUN_ATTEMPT'] = '2'
        # The test helper's prepare asserts zero writes; preserve prior history separately.
        self.api.writes.clear()
        self.assertEqual(self.complete(), 'consumed')
        self.assertEqual(self.api.assets['release-promotion.json']['data'], receipt)
        self.assertEqual(self.api.writes, [])
        self.assertTrue(writes)

    def test_interrupted_upload_new_attempt_resumes_without_replacement(self):
        self.api.fail_upload = 'release-promotion.json'
        self.post(self.prepare())
        with self.assertRaises(TimeoutError):
            self.stage()
        original = copy.deepcopy(self.api.assets)
        self.api.fail_upload = None
        self.api.writes.clear()
        self.state = self.root / 'retry'
        os.environ['GITHUB_RUN_ATTEMPT'] = '2'
        self.assertEqual(self.complete(), 'published')
        for name, value in original.items():
            self.assertEqual(self.api.assets[name], value)

    def test_expiry_during_upload_stops_later_writes_and_fresh_attempt_recovers(self):
        self.post(self.prepare())
        transport = self.transport('stage')
        uploads = []
        def expire_after_committed_upload(path, method='GET', body=None, **kwargs):
            value = self.api(path, method, body, **kwargs)
            if method == 'POST' and path.startswith('https://uploads.github.com/'):
                uploads.append(path)
                transport.expires = int(time.time()) - 1
            return value
        with self.assertRaisesRegex(ValueError, 'expired'):
            promotion.promote(expire_after_committed_upload, self.api.run, transport)
        self.assertEqual(len(uploads), 1)
        self.assertTrue(self.api.release['draft'])
        original = copy.deepcopy(self.api.assets)
        self.api.writes.clear()
        self.state = self.root / 'fresh-after-expiry'
        os.environ['GITHUB_RUN_ATTEMPT'] = '2'
        self.assertEqual(self.complete(), 'published')
        for name, asset in original.items():
            self.assertEqual(self.api.assets[name], asset)
            self.assertFalse(any(write[1] == 'POST' and write[0].endswith('?name=' + name)
                                 for write in self.api.writes))
        self.assertFalse(any(write[1] == 'DELETE' for write in self.api.writes))

    def test_authority_change_during_owner_wait_produces_zero_writes(self):
        c = self.prepare()
        self.post(c)
        self.api.branch['commit']['sha'] = 'f' * 40
        with self.assertRaises(ValueError):
            promotion.promote(self.api, self.api.run, self.transport('stage'))
        self.assertEqual(self.api.writes, [])

    def test_unenrolled_policy_and_absent_transport_fail_before_writes(self):
        with self.assertRaisesRegex(ValueError, 'transport'):
            promotion.promote(self.api, self.api.run)
        self.policy['public_disclosure_approved'] = False
        self.write_policy()
        with self.assertRaisesRegex(ValueError, 'approval'):
            promotion.promote(self.api, self.api.run, self.transport('prepare'))
        self.assertEqual(self.api.writes, [])

    def test_bounded_json_cli_and_fixed_endpoints(self):
        for data in (b'{"a":1,"a":2}', b'NaN', b'x' * (evidence.MAX_MESSAGE + 1), b'[' * 2000):
            with self.assertRaises(ValueError):
                evidence.parse(data)
        with self.assertRaisesRegex(ValueError, 'limit'):
            evidence.bounded_process([sys.executable, '-c', 'print("x" * 100000)'], limit=32)
        for path in ('/releases', '/git/refs', 'https://attacker.invalid', '/rulesets/1'):
            with self.assertRaisesRegex(ValueError, 'endpoint'):
                evidence.gh_api(path, {'body': 'no'}, inbox=1)

    def test_challenge_archive_digest_inventory_and_expansion_limits(self):
        c = self.prepare()
        def archive(name='challenge.json', payload=None):
            buffer = io.BytesIO()
            with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as z:
                z.writestr(name, payload or evidence.canonical(c))
            return buffer.getvalue()
        for fault in (None, 'digest', 'inventory', 'size'):
            raw = archive('../challenge.json' if fault == 'inventory' else 'challenge.json',
                          b'x' * 17000 if fault == 'size' else None)
            a = dict(id=1, name='owner-challenge-99-1-preflight', expired=False, size_in_bytes=len(raw),
                     workflow_run={'id': 99, 'repository_id': evidence.REPO_ID},
                     digest='sha256:' + hashlib.sha256(raw).hexdigest())
            if fault == 'digest': a['digest'] = 'wrong'
            with patch.object(evidence, 'gh_api', side_effect=[{'total_count': 1, 'artifacts': [a]}, raw]):
                if fault:
                    with self.assertRaises(ValueError): evidence.download_challenge(99, 1, 'preflight')
                else:
                    self.assertEqual(evidence.download_challenge(99, 1, 'preflight'), c)

    def test_no_intent_no_human_request_and_no_writes(self):
        self.api.files = []
        self.assertEqual(promotion.promote(self.api, self.api.run, self.transport('prepare')), 'no-intent')
        self.assertFalse(self.state.exists())
        self.assertEqual(self.api.writes, [])

    def test_unrelated_inactive_actor_disclosure_denied_before_sign_post_or_publish(self):
        c = self.prepare()
        original = self.api.full
        for enforcement in ('active', 'disabled', 'evaluate'):
            for actor_type in ('User', 'Team', 'Integration', 'App'):
                with self.subTest(enforcement=enforcement, actor_type=actor_type):
                    extra = dict(id=3, source=evidence.REPO, source_type='Repository', target='tag',
                                 enforcement=enforcement,
                                 conditions={'ref_name': {'include': ['refs/tags/unrelated-*'], 'exclude': []}},
                                 rules=[], bypass_actors=[dict(actor_type=actor_type, actor_id=123, bypass_mode='always')])
                    def full(path):
                        if path.startswith('/rulesets?'):
                            return original(path) + [evidence.project(extra, evidence.SUMMARY)]
                        if path == '/rulesets/3': return copy.deepcopy(extra)
                        return original(path)
                    with patch.object(self.api, 'full', side_effect=full):
                        # Reproduce F-01: existing release-path validation accepts this unrelated Ruleset.
                        promotion.protected_branches(full, strict=True)
                        snapshot = evidence.collect(full)
                        with self.assertRaisesRegex(ValueError, 'actor disclosure'):
                            evidence.validate_disclosure(snapshot, self.policy)
                        self.api.comments.clear()
                        self.post(c)  # Authenticated but disallowed complete snapshot also fails at publisher.
                        with self.assertRaisesRegex(ValueError, 'actor disclosure'):
                            promotion.promote(self.api, self.api.run, self.transport('stage'))
                        posts = []
                        def helper_api(path, body=None, **kwargs):
                            if body is not None:
                                posts.append(path)
                                raise AssertionError('Public POST must not occur')
                            if path == 'user': return dict(id=evidence.OWNER, login='oangsa')
                            if path == '/issues/1': return dict(number=1, state='open')
                            if path == '/actions/runs/99':
                                return dict(path=evidence.WORKFLOW, event='workflow_run', head_branch='main',
                                            head_sha='d' * 40, repository={'id': evidence.REPO_ID},
                                            head_repository={'id': evidence.REPO_ID}, run_attempt=1, status='in_progress')
                            return full(path)
                        args = SimpleNamespace(approve_public_evidence=True, key=str(self.key), run_id=99,
                                               trusted_sha='d' * 40, source_sha=c['binding']['source']['head_sha'],
                                               version=c['binding']['intent']['version'])
                        with patch.object(evidence, 'load_policy', return_value=(self.policy, [self.public])), \
                                patch.object(evidence, 'gh_api', side_effect=helper_api), \
                                patch.object(evidence, 'download_challenge', return_value=c), \
                                patch('builtins.input', return_value=f'APPROVE preflight {args.version}'), \
                                patch('builtins.print'), patch.object(evidence, 'sign') as signing:
                            with self.assertRaisesRegex(ValueError, 'actor disclosure'):
                                evidence.attend(args)
                            signing.assert_not_called()
                        self.assertEqual(posts, [])
                        self.assertEqual(self.api.writes, [])

    def test_disclosure_exact_snapshot_unknown_fields_and_confidential_values_fail_closed(self):
        snapshot = evidence.collect(self.api.full)
        before = copy.deepcopy(snapshot)
        evidence.validate_disclosure(snapshot, self.policy)
        self.assertEqual(snapshot, before)  # Never redact or modify strict evidence.
        for fault in ('rule-field', 'condition', 'branch-field', 'immutable-field', 'inventory-field',
                      'ruleset-field', 'actor-field', 'actor-boolean', 'new-empty-ruleset'):
            value = copy.deepcopy(snapshot)
            if fault == 'rule-field': value['rulesets'][0]['rules'].append({'type': 'unknown', 'secret': 'confidential'})
            if fault == 'condition': value['rulesets'][0]['conditions']['confidential'] = 'internal-team-name'
            if fault == 'branch-field': value['branches']['dev'][0]['confidential'] = 'private'
            if fault == 'immutable-field': value['immutable']['unexpected'] = True
            if fault == 'inventory-field': value['inventory'][0]['unexpected'] = 'private'
            if fault == 'ruleset-field': value['rulesets'][0]['unexpected'] = 'private'
            if fault.startswith('actor-'):
                actor = dict(actor_type='RepositoryRole', actor_id=5, bypass_mode='always')
                if fault == 'actor-field': actor['name'] = 'private'
                else: actor['actor_id'] = True
                value['rulesets'][0]['bypass_actors'] = [actor]
            if fault == 'new-empty-ruleset': value['rulesets'].append(copy.deepcopy(value['rulesets'][0]))
            with self.subTest(fault=fault), self.assertRaises(ValueError):
                evidence.validate_disclosure(value, self.policy)
            self.assertEqual(self.api.writes, [])
        # Even an accidentally approved digest cannot disclose identity-bearing actor categories.
        value = copy.deepcopy(snapshot)
        value['rulesets'][0]['bypass_actors'] = [dict(actor_type='Team', actor_id=1, bypass_mode='always')]
        with self.assertRaisesRegex(ValueError, 'actor disclosure'):
            evidence.validate_disclosure(value, dict(self.policy, disclosure_sha256=evidence.digest(value)))

    def test_disclosure_change_requires_new_review_and_both_gate_checks(self):
        c = self.prepare()
        self.post(c)
        final = self.stage()
        def change(payload):
            payload['controls']['rulesets'][0]['conditions']['confidential'] = 'new-category'
        self.post(final, self.response(final, change))
        before = list(self.api.writes)
        with self.assertRaisesRegex(ValueError, 'disclosure snapshot'):
            promotion.promote(self.api, self.api.run, self.transport('finish'))
        self.assertEqual(self.api.writes, before)
        self.assertTrue(self.api.release['draft'])
        changed = evidence.collect(self.api.full)
        changed['immutable']['enforced_by_owner'] = True
        with self.assertRaisesRegex(ValueError, 'disclosure snapshot'):
            evidence.validate_disclosure(changed, self.policy)
        evidence.validate_disclosure(changed, dict(self.policy, disclosure_sha256=evidence.digest(changed)))

    def test_nested_source_linux_attempt_and_intent_binding(self):
        c = self.prepare()
        for field in ('source-sha', 'source-run', 'source-attempt', 'linux-sha', 'linux-attempt', 'intent', 'receipt'):
            def change(payload):
                binding = payload['challenge']['binding']
                if field == 'source-sha': binding['source']['head_sha'] = 'f' * 40
                if field == 'source-run': binding['source']['id'] += 1
                if field == 'source-attempt': binding['source']['run_attempt'] += 1
                if field == 'linux-sha': binding['linux']['head_sha'] = 'f' * 40
                if field == 'linux-attempt': binding['linux']['run_attempt'] += 1
                if field == 'intent': binding['intent']['id'] = 'different'
                if field == 'receipt': binding['receipt_sha256'] = '0' * 64
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'binding'):
                evidence.verify(self.response(copy.deepcopy(c), change), c, set(), self.policy_path)

    def test_finish_requires_same_attempt_and_receipt_and_final_timeout_leaves_draft(self):
        self.post(self.prepare())
        self.stage()
        before = list(self.api.writes)
        os.environ['GITHUB_RUN_ATTEMPT'] = '2'
        with self.assertRaisesRegex(ValueError, 'changed'):
            promotion.promote(self.api, self.api.run, self.transport('finish'))
        os.environ['GITHUB_RUN_ATTEMPT'] = '1'
        with patch.object(evidence.time, 'monotonic', side_effect=[0, 1, 2000]), patch.object(evidence.time, 'sleep'):
            with self.assertRaisesRegex(ValueError, 'timeout'):
                promotion.promote(self.api, self.api.run, self.transport('finish'))
        self.assertEqual(self.api.writes, before)
        self.assertTrue(self.api.release['draft'])

    def test_mutation_expiry_and_late_conflict_cannot_write(self):
        c = self.prepare()
        self.post(c)
        transport = self.transport('stage')
        transport.gate('preflight', self.api, c['binding'])
        transport.expires = int(time.time()) - 1
        with self.assertRaisesRegex(ValueError, 'expired'): transport.fresh()
        transport.expires = int(time.time()) + 60
        self.post(c)
        with self.assertRaisesRegex(ValueError, 'conflict'): transport.fresh()
        self.assertEqual(self.api.writes, [])

    def test_missing_post_publish_immutable_readback_is_quarantined(self):
        self.post(self.prepare())
        self.post(self.stage())
        def missing_readback(path, *args, **kwargs):
            value = self.api(path, *args, **kwargs)
            if path == '/releases/50' and not value['draft']:
                value = dict(value)
                value.pop('immutable', None)
            return value
        with self.assertRaisesRegex(ValueError, 'immutable readback'):
            promotion.promote(missing_readback, self.api.run, self.transport('finish'))
        self.assertFalse(self.api.release['draft'])
        self.assertTrue(self.api.release['immutable'])

    def test_governance_allowlist_and_branch_lane_cover_only_reviewed_files(self):
        from hotfix_policy import GOVERNANCE_PATHS
        for path in ('.github/owner-evidence-policy.json', 'scripts/owner_evidence.py',
                     'scripts/test-owner-evidence.py', 'docs/OWNER-EVIDENCE.md', 'docs/SANDBOX-RELEASE-LIFECYCLE.md'):
            self.assertIn(path, GOVERNANCE_PATHS)
        self.assertNotIn('src/updates.rs', GOVERNANCE_PATHS)
        self.assertNotIn('Cargo.toml', GOVERNANCE_PATHS)

    def test_workflow_is_owner_variable_gated_with_exact_owner_policy(self):
        workflow = (evidence.ROOT / '.github/workflows/release-promotion.yml').read_text()
        self.assertNotIn('false &&', workflow)
        self.assertIn("vars.RELEASE_PROMOTION_ENABLED == 'true'", workflow)
        self.assertIn("github.event.workflow_run.event == 'push'", workflow)
        self.assertIn("github.event.workflow_run.conclusion == 'success'", workflow)
        self.assertIn("github.event.workflow_run.head_repository.full_name == github.repository", workflow)
        self.assertIn('timeout-minutes: 20', workflow)
        self.assertIn('cancel-in-progress: false', workflow)
        self.assertNotIn('issues: read', workflow)
        self.assertIn('--stage automatic', workflow)
        self.assertIn('permission-administration: write', workflow)
        self.assertIn('repositories: RoLauncher', workflow)
        self.assertNotIn('--stage stage', workflow)
        self.assertNotIn('\n      administration:', workflow)
        policy, active = evidence.load_policy()
        self.assertEqual(policy['inbox'], 35)
        self.assertIs(policy['public_disclosure_approved'], True)
        self.assertEqual(policy['disclosure_sha256'],
                         '9a6ade9194ff6d87897e07de05e280b367351f823074537d6db66e42b58cd6fa')
        self.assertEqual(policy['keys'], [
            'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIfhxAsJYk67VBohpkfYVBKdEkJV1/6IASKEBesgTAoW'
        ])
        self.assertEqual(active, policy['keys'])


if __name__ == '__main__':
    unittest.main()
