"""Owner-attended live control evidence; no release authority or publication API."""
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import tempfile
import threading
import time
import zipfile

REPO = 'oangsa/RoLauncher'
REPO_ID = 1406138209
OWNER = 71574088
WORKFLOW = '.github/workflows/release-promotion.yml'
NAMESPACE = 'rolauncher-production-controls-v1'
PRINCIPAL = 'github-user-71574088'
ROOT = Path(__file__).resolve().parent.parent
POLICY = ROOT / '.github/owner-evidence-policy.json'
STATE = Path('owner-evidence')
MAX_MESSAGE = 48 * 1024
MAX_API = 2 * 1024 * 1024
WAIT = 1200
TTL = 120
FIELDS = ('id', 'source', 'source_type', 'target', 'enforcement', 'conditions', 'rules')
SUMMARY = ('id', 'source', 'source_type', 'target', 'enforcement')


def require(ok, message):
    if not ok:
        raise ValueError(message)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True,
                      allow_nan=False).encode('utf-8')


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def parse(data, limit=MAX_MESSAGE):
    require(len(data) <= limit, 'Evidence size limit')
    def unique(pairs):
        result = dict(pairs)
        require(len(result) == len(pairs), 'Duplicate evidence JSON fields')
        return result
    def constant(value):
        raise ValueError('Nonfinite JSON value')
    try:
        return json.loads(data, object_pairs_hook=unique, parse_constant=constant)
    except (RecursionError, UnicodeError) as error:
        raise ValueError('Malformed evidence JSON') from error


def load_policy(path=POLICY):
    p = parse(Path(path).read_bytes())
    require(set(p) == {'schema', 'repository_id', 'owner_id', 'inbox', 'public_disclosure_approved',
                       'keys', 'revoked', 'disclosure_sha256'} and type(p['schema']) is int and p['schema'] == 1
            and type(p['repository_id']) is int and p['repository_id'] == REPO_ID
            and type(p['owner_id']) is int and p['owner_id'] == OWNER, 'Evidence policy identity')
    require(type(p['inbox']) is int and p['inbox'] > 0 and p['public_disclosure_approved'] is True,
            'Separate owner approval and reviewed public inbox enrollment required')
    require(isinstance(p['disclosure_sha256'], str)
            and re.fullmatch('[a-f0-9]{64}', p['disclosure_sha256']),
            'Reviewed exact public disclosure snapshot required')
    require(isinstance(p['keys'], list) and 0 < len(p['keys']) <= 4
            and isinstance(p['revoked'], list) and len(p['revoked']) <= 100, 'Signing keys not enrolled')
    require(all(isinstance(k, str) and re.fullmatch(r'ssh-ed25519 [A-Za-z0-9+/]{68}', k)
                for k in p['keys']) and len(set(p['keys'])) == len(p['keys']), 'Invalid approved signing key')
    require(all(isinstance(k, str) and re.fullmatch(r'SHA256:[A-Za-z0-9+/]{43}', k)
                for k in p['revoked']), 'Invalid revoked key fingerprint')
    active = []
    for key in p['keys']:
        fingerprint = 'SHA256:' + base64.b64encode(hashlib.sha256(
            base64.b64decode(key.split()[1], validate=True)).digest()).decode().rstrip('=')
        if fingerprint not in p['revoked']:
            active.append(key)
    require(active, 'All signing keys revoked')
    return p, active


def run_identity(run):
    return {k: run[k] for k in ('id', 'run_attempt', 'head_sha', 'path', 'event', 'head_branch')}


def binding_for(intent, run, linux, receipt):
    return dict(intent=intent, source=run_identity(run), linux=run_identity(linux) if linux else None,
                receipt_sha256=hashlib.sha256(receipt).hexdigest())


def project(value, fields):
    require(isinstance(value, dict) and all(k in value for k in fields), 'Incomplete Ruleset fields')
    return {k: value[k] for k in fields}


def collect(call, bypass=True):
    """Complete inventory, including inactive and inherited Rulesets; no pagination guesses."""
    inventory = call('/rulesets?includes_parents=true&per_page=100')
    require(isinstance(inventory, list) and len(inventory) < 100, 'Incomplete Ruleset inventory')
    inventory = sorted((project(r, SUMMARY) for r in inventory), key=lambda r: r['id'])
    ids = [r['id'] for r in inventory]
    require(all(type(i) is int and i > 0 for i in ids) and len(ids) == len(set(ids)), 'Duplicate Ruleset IDs')
    rulesets = []
    for summary in inventory:
        raw = call('/rulesets/' + str(summary['id']))
        require(project(raw, SUMMARY) == summary, 'Ruleset inventory changed during collection')
        fields = FIELDS + (('bypass_actors',) if bypass else ())
        item = project(raw, fields)
        if bypass:
            require(isinstance(item['bypass_actors'], list), 'Missing bypass array')
        rulesets.append(item)
    branches = {}
    for branch in ('main', 'dev'):
        rules = call('/rules/branches/' + branch + '?per_page=100')
        require(isinstance(rules, list) and len(rules) < 100
                and all(r.get('ruleset_id') in ids for r in rules), 'Incomplete effective branch Rulesets')
        branches[branch] = rules
    # Re-list after details: no silently omitted or changed Ruleset during collection.
    again = call('/rulesets?includes_parents=true&per_page=100')
    require(isinstance(again, list) and len(again) < 100
            and canonical(sorted((project(r, SUMMARY) for r in again), key=lambda r: r['id'])) == canonical(inventory),
            'Ruleset inventory raced collection')
    result = dict(inventory=inventory, rulesets=rulesets, branches=branches)
    if bypass:
        immutable = call('/immutable-releases')
        require(isinstance(immutable, dict) and immutable.get('enabled') is True
                and type(immutable.get('enforced_by_owner')) is bool, 'Native release immutability disabled or missing')
        result['immutable'] = project(immutable, ('enabled', 'enforced_by_owner'))
    return result


class AdmissionOnly:
    """Stop after candidate verification, before any control or release writes."""
    stage = 'all'

    def gate(self, phase, call, binding):
        raise ChallengeReady()


class AutomaticEvidence:
    """Read live controls with a separate repository-scoped App credential."""
    stage = 'all'

    def __init__(self, app_call):
        self.app_call = app_call
        self.runtime = None
        self.binding = None
        self.checked = None

    def read(self, path, *args, **kwargs):
        require(not args and not kwargs and (
            path in ('', '/immutable-releases', '/rulesets?includes_parents=true&per_page=100')
            or re.fullmatch(r'/rulesets/[1-9][0-9]*', path)
            or path in ('/rules/branches/main?per_page=100', '/rules/branches/dev?per_page=100')),
            'App credential permits control GETs only')
        return self.app_call(path)

    def refresh(self):
        from release_promotion import protected_branches
        started = time.monotonic()
        repository = self.read('')
        require(repository['id'] == REPO_ID and repository['full_name'] == REPO
                and repository['owner']['id'] == OWNER, 'Wrong App installation repository')
        snapshot = collect(self.read)
        controls = controls_call(self.runtime, snapshot)
        protected_branches(controls, strict=True)
        require(time.monotonic() - started <= 45, 'Live control collection took too long')
        self.checked = started
        return controls

    def gate(self, phase, call, binding):
        require(phase in ('preflight', 'final'), 'Unknown control phase')
        require(self.binding is None or self.binding == binding, 'Release binding changed')
        self.binding, self.runtime = binding, call
        return self.refresh()

    def fresh(self):
        require(self.checked is not None, 'Live controls have not been checked')
        # Refresh automatically during slow uploads, without extending an old observation.
        if time.monotonic() - self.checked > 45:
            self.refresh()


def controls_call(call, snapshot):
    """Compare signed observations with live metadata, then expose complete strict evidence."""
    require(set(snapshot) == {'inventory', 'rulesets', 'branches', 'immutable'}
            and snapshot['immutable'].get('enabled') is True
            and type(snapshot['immutable'].get('enforced_by_owner')) is bool, 'Native release immutability disabled or missing')
    metadata = collect(call, bypass=False)
    signed_metadata = dict(inventory=snapshot['inventory'], branches=snapshot['branches'],
                           rulesets=[project(r, FIELDS) for r in snapshot['rulesets']])
    require(canonical(metadata) == canonical(signed_metadata), 'Signed Ruleset metadata is stale or incomplete')
    by_id = {r['id']: r for r in snapshot['rulesets']}
    require(len(by_id) == len(snapshot['rulesets']) and all(isinstance(r.get('bypass_actors'), list)
            for r in by_id.values()), 'Missing or duplicate signed bypass evidence')
    def evidence(path, *args, **kwargs):
        require(not args and not kwargs, 'Controls adapter is read-only')
        match = re.fullmatch(r'/rulesets/([1-9][0-9]*)', path)
        if match:
            require(int(match[1]) in by_id, 'Unsigned Ruleset source')
            return by_id[int(match[1])]
        return call(path)
    return evidence


def validate_disclosure(snapshot, policy):
    """Separate public-disclosure authority: approve exact bytes, never filter evidence.

    The reviewed digest covers ALL nested fields, inactive/inherited Rulesets and
    effective branch rules. Any new category or value requires policy review.
    Actor validation additionally forbids identity-bearing actor categories even
    if someone tries to enroll their digest without changing this reviewed code.
    """
    require(isinstance(snapshot, dict)
            and set(snapshot) == {'inventory', 'rulesets', 'branches', 'immutable'}
            and isinstance(snapshot['inventory'], list) and isinstance(snapshot['rulesets'], list),
            'Public disclosure snapshot contract')
    for summary in snapshot['inventory']:
        require(isinstance(summary, dict) and set(summary) == set(SUMMARY),
                'Unexpected public inventory fields')
    for ruleset in snapshot['rulesets']:
        require(isinstance(ruleset, dict) and set(ruleset) == set(FIELDS) | {'bypass_actors'}
                and isinstance(ruleset['bypass_actors'], list), 'Unexpected public Ruleset fields')
        for actor in ruleset['bypass_actors']:
            require(isinstance(actor, dict)
                    and set(actor) == {'actor_type', 'actor_id', 'bypass_mode'}
                    and actor['actor_type'] == 'RepositoryRole'
                    and type(actor['actor_id']) is int and actor['actor_id'] == 5
                    and actor['bypass_mode'] == 'always', 'Public bypass actor disclosure denied')
    approved = policy.get('disclosure_sha256')
    require(isinstance(approved, str) and re.fullmatch('[a-f0-9]{64}', approved)
            and digest(snapshot) == approved, 'Public disclosure snapshot differs from reviewed policy')


def ssh():
    executable = shutil.which('ssh-keygen')
    if not executable and os.name == 'nt':
        executable = 'C:/Windows/System32/OpenSSH/ssh-keygen.exe'
    require(executable, 'OpenSSH ssh-keygen required')
    return executable


def sign(payload, key):
    data = canonical(payload)
    require(len(data) <= 32 * 1024, 'Signed payload size')
    with tempfile.TemporaryDirectory() as folder:
        message = Path(folder) / 'evidence.json'
        message.write_bytes(data)
        # Inherit terminal: encrypted keys can request their passphrase locally.
        result = subprocess.run([ssh(), '-Y', 'sign', '-f', str(Path(key).resolve()), '-n', NAMESPACE, str(message)],
                                timeout=60, stdout=subprocess.DEVNULL)
        require(result.returncode == 0, 'Local SSH signing failed')
        signature = Path(str(message) + '.sig').read_text(encoding='ascii')
    return dict(payload_base64=base64.b64encode(data).decode(), signature=signature)


def verify(response, challenge, consumed, policy_path=POLICY, now=None):
    now = int(time.time()) if now is None else now
    p, keys = load_policy(policy_path)
    require(set(response) == {'payload_base64', 'signature'} and isinstance(response['payload_base64'], str)
            and isinstance(response['signature'], str) and len(response['signature']) < 2048,
            'Malformed signed response')
    require(len(canonical(response)) <= MAX_MESSAGE, 'Evidence response size')
    data = base64.b64decode(response['payload_base64'], validate=True)
    require(len(data) <= 32 * 1024, 'Evidence payload size')
    with tempfile.TemporaryDirectory() as folder:
        allowed = Path(folder) / 'allowed_signers'
        allowed.write_text(''.join(PRINCIPAL + ' namespaces="' + NAMESPACE + '" ' + key + '\n' for key in keys), encoding='ascii')
        signature = Path(folder) / 'signature'
        signature.write_text(response['signature'], encoding='ascii')
        result = subprocess.run([ssh(), '-Y', 'verify', '-f', str(allowed), '-I', PRINCIPAL,
                                 '-n', NAMESPACE, '-s', str(signature)], input=data,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        require(result.returncode == 0, 'Owner signature invalid or revoked')
    value = parse(data, 32 * 1024)
    require(set(value) == {'schema', 'challenge', 'collection_start', 'collection_end', 'expires', 'controls'}
            and type(value['schema']) is int and value['schema'] == 1, 'Evidence payload contract')
    check_challenge(challenge, p, now)
    require(canonical(value['challenge']) == canonical(challenge), 'Evidence challenge binding mismatch')
    start, end, expires = (value[k] for k in ('collection_start', 'collection_end', 'expires'))
    require(all(type(t) is int for t in (start, end, expires)) and challenge['created'] <= start <= end <= now + 5
            and end - start <= 45 and expires == start + TTL and now <= expires, 'Expired, future or slow evidence')
    require(challenge['nonce'] not in consumed, 'Evidence replay rejected')
    require(isinstance(value['controls'], dict), 'Invalid controls object')
    validate_disclosure(value['controls'], p)
    consumed.add(challenge['nonce'])
    return value


def check_challenge(c, policy, now=None):
    now = int(time.time()) if now is None else now
    require(set(c) == {'schema', 'purpose', 'repository', 'repository_id', 'owner_id', 'workflow',
                      'trusted_sha', 'run_id', 'attempt', 'binding', 'policy_sha256', 'inbox',
                      'phase', 'nonce', 'created', 'deadline'} and type(c['schema']) is int and c['schema'] == 1
            and c['purpose'] == NAMESPACE and c['repository'] == REPO and c['repository_id'] == REPO_ID
            and type(c['repository_id']) is int and c['owner_id'] == OWNER and type(c['owner_id']) is int
            and c['workflow'] == WORKFLOW and c['inbox'] == policy['inbox']
            and c['policy_sha256'] == digest(policy), 'Challenge identity/policy mismatch')
    require(all(type(c[k]) is int and c[k] > 0 for k in ('run_id', 'attempt'))
            and isinstance(c['trusted_sha'], str) and re.fullmatch('[a-f0-9]{40}', c['trusted_sha'])
            and isinstance(c['nonce'], str) and re.fullmatch('[a-f0-9]{64}', c['nonce'])
            and c['phase'] in ('preflight', 'final'), 'Challenge run/phase identity')
    require(type(c['created']) is int and type(c['deadline']) is int and c['deadline'] == c['created'] + WAIT
            and c['created'] <= now + 5 and now <= c['deadline'], 'Challenge expired')
    b = c['binding']
    require(set(b) == {'intent', 'source', 'linux', 'receipt_sha256'}
            and re.fullmatch('[a-f0-9]{64}', b['receipt_sha256']), 'Challenge source contract')
    for run in (b['source'], b['linux']):
        if run is None:
            continue
        require(set(run) == {'id', 'run_attempt', 'head_sha', 'path', 'event', 'head_branch'}
                and all(type(run[k]) is int and run[k] > 0 for k in ('id', 'run_attempt'))
                and re.fullmatch('[a-f0-9]{40}', run['head_sha']) and run['event'] == 'push', 'Challenge source run')


class ChallengeReady(Exception):
    """Workflow uploads the challenge before starting the next trusted step."""


class StagedEvidence:
    def __init__(self, stage, policy_path=POLICY, state=STATE):
        require(os.environ.get('GITHUB_REPOSITORY') == REPO
                and os.environ.get('GITHUB_REPOSITORY_ID') == str(REPO_ID)
                and os.environ.get('GITHUB_EVENT_NAME') == 'workflow_run'
                and os.environ.get('GITHUB_WORKFLOW_REF') == REPO + '/' + WORKFLOW + '@refs/heads/main',
                'Trusted publisher environment required')
        self.stage, self.policy_path, self.state = stage, policy_path, Path(state)
        self.expires = 0
        self.accepted = None

    def fresh(self):
        require(int(time.time()) <= self.expires, 'Live evidence expired before write; rerun for fresh gates')
        if self.accepted:
            call, c, comment_id, body_hash = self.accepted
            comment = call('/issues/comments/' + str(comment_id))
            require(comment['user']['id'] == OWNER and comment['created_at'] == comment['updated_at']
                    and hashlib.sha256(comment['body'].encode()).hexdigest() == body_hash, 'Accepted response edited')
            matches = response_candidates(call, c)
            require(len(matches) == 1 and matches[0][0]['id'] == comment_id, 'Owner response conflict after acceptance')
        require(int(time.time()) <= self.expires, 'Evidence expired during response recheck')

    def resume(self, binding):
        # Local runner state is trusted bookkeeping, never cached bypass authority.
        state = parse((self.state / 'preflight-accepted.json').read_bytes())
        p, _ = load_policy(self.policy_path)
        check_challenge(state['challenge'], p, state['challenge']['created'])
        require(state['challenge']['phase'] == 'preflight'
                and state['challenge']['run_id'] == int(os.environ['GITHUB_RUN_ID'])
                and state['challenge']['attempt'] == int(os.environ['GITHUB_RUN_ATTEMPT'])
                and state['challenge']['trusted_sha'] == os.environ['GITHUB_SHA']
                and canonical(state['challenge']['binding']) == canonical(binding), 'Staged receipt/source changed')

    def gate(self, phase, call, binding):
        p, _ = load_policy(self.policy_path)
        repository = call('')
        require(repository['id'] == REPO_ID and repository['full_name'] == REPO
                and repository['owner']['id'] == OWNER and repository['owner']['login'] == 'oangsa'
                and repository['default_branch'] == 'main', 'Live repository identity changed')
        identity = dict(schema=1, purpose=NAMESPACE, repository=REPO, repository_id=REPO_ID, owner_id=OWNER,
                        workflow=WORKFLOW, trusted_sha=os.environ['GITHUB_SHA'], run_id=int(os.environ['GITHUB_RUN_ID']),
                        attempt=int(os.environ['GITHUB_RUN_ATTEMPT']), binding=binding, policy_sha256=digest(p), inbox=p['inbox'])
        if (self.stage, phase) in (('prepare', 'preflight'), ('stage', 'final')):
            now = int(time.time())
            c = dict(identity, phase=phase, nonce=secrets.token_hex(32), created=now, deadline=now + WAIT)
            check_challenge(c, p)
            directory = self.state / phase
            directory.mkdir(parents=True, exist_ok=True)
            path = directory / 'challenge.json'
            require(not path.exists(), 'Challenge already exists; new attempt required')
            path.write_bytes(canonical(c))
            print(f'Owner attendance required: {phase}; run {c["run_id"]}, attempt {c["attempt"]}', flush=True)
            raise ChallengeReady()
        require((self.stage, phase) in (('stage', 'preflight'), ('finish', 'final')), 'Invalid evidence stage')
        c = parse((self.state / phase / 'challenge.json').read_bytes())
        check_challenge(c, p)
        require(canonical({k: c[k] for k in identity}) == canonical(identity), 'Challenge context changed')
        used = set()
        for prior in ('preflight', 'final'):
            accepted = self.state / (prior + '-accepted.json')
            if accepted.exists():
                used.add(parse(accepted.read_bytes())['challenge']['nonce'])
        value, comment_id, body = wait_response(call, c, used, self.policy_path)
        controls = controls_call(call, value['controls'])
        from release_promotion import protected_branches
        protected_branches(controls, strict=True)
        self.expires = value['expires']
        self.accepted = (call, c, comment_id, hashlib.sha256(body.encode()).hexdigest())
        self.fresh()
        (self.state / (phase + '-response.json')).write_text(body, encoding='utf-8')
        (self.state / (phase + '-accepted.json')).write_bytes(canonical(dict(challenge=c, comment_id=comment_id,
            response_sha256=digest(value), expires=value['expires'])))
        return controls


def response_candidates(call, challenge):
    since = datetime.fromtimestamp(challenge['created'] - 5, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    comments = call(f'/issues/{challenge["inbox"]}/comments?per_page=100&since={since}')
    require(isinstance(comments, list) and len(comments) < 100, 'Incomplete owner inbox')
    matches = []
    require(len({c['id'] for c in comments}) == len(comments), 'Duplicate inbox comment IDs')
    for comment in comments:
        if comment['user']['id'] != OWNER:
            continue
        try:
            wrapper = parse(comment['body'].encode())
            if wrapper.get('challenge_sha256') == digest(challenge):
                matches.append((comment, wrapper))
        except (ValueError, TypeError, AttributeError):
            continue
    return matches


def wait_response(call, challenge, used, policy_path=POLICY):
    # Monotonic bound also stops waiting if the workstation/runner clock moves backwards.
    remaining = max(0, challenge['deadline'] - int(time.time()))
    deadline = time.monotonic() + remaining
    while time.monotonic() < deadline and int(time.time()) <= challenge['deadline']:
        matches = response_candidates(call, challenge)
        require(len(matches) <= 1, 'Duplicate or conflicting owner responses')
        if matches:
            comment, wrapper = matches[0]
            require(set(wrapper) == {'challenge_sha256', 'response'}, 'Malformed owner response wrapper')
            fresh = call('/issues/comments/' + str(comment['id']))
            require(fresh['id'] == comment['id'] and fresh['user']['id'] == OWNER
                    and fresh['body'] == comment['body'] and fresh['created_at'] == fresh['updated_at'],
                    'Edited or replaced owner response')
            return verify(wrapper['response'], challenge, used, policy_path), comment['id'], comment['body']
        time.sleep(5)
    raise ValueError('Owner attendance timeout; no publication authorization')


def gh():
    executable = shutil.which('gh')
    if not executable and os.name == 'nt':
        executable = 'C:/Program Files/GitHub CLI/gh.exe'
    require(executable, 'GitHub CLI required')
    return executable


def bounded_process(args, data=None, limit=MAX_API):
    """Drain both pipes with hard byte ceilings; never buffer unlimited CLI output."""
    process = subprocess.Popen(args, stdin=subprocess.PIPE if data is not None else subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    chunks, errors = [], []
    def read(stream, target, budget):
        size = 0
        while chunk := stream.read(8192):
            size += len(chunk)
            if size > budget:
                errors.append('GitHub CLI response size limit')
                process.kill()
                break
            target.append(chunk)
    out = threading.Thread(target=read, args=(process.stdout, chunks, limit))
    err = threading.Thread(target=read, args=(process.stderr, [], 16384))
    out.start()
    err.start()
    try:
        if data is not None:
            process.stdin.write(data)
            process.stdin.close()
        code = process.wait(timeout=60)
    except BaseException:
        process.kill()
        process.wait()
        raise
    finally:
        out.join()
        err.join()
        process.stdout.close()
        process.stderr.close()
    require(not errors and code == 0, 'GitHub CLI request failed or exceeded limit; no mutation retry')
    return b''.join(chunks)


def gh_api(path, body=None, raw=False, inbox=None):
    reads = (path in ('', 'user', '/immutable-releases', '/rulesets?includes_parents=true&per_page=100')
             or re.fullmatch(r'/rulesets/[1-9][0-9]*|/rules/branches/(main|dev)\?per_page=100'
                             r'|/actions/workflows/release-promotion\.yml'
                             r'|/actions/runs/[1-9][0-9]*|/actions/runs/[1-9][0-9]*/artifacts\?per_page=100'
                             r'|/actions/artifacts/[1-9][0-9]*/zip|/issues/[1-9][0-9]*', path))
    require((body is None and reads) or (body is not None and type(inbox) is int
            and path == f'/issues/{inbox}/comments' and set(body) == {'body'}
            and len(body['body'].encode()) <= MAX_MESSAGE), 'Owner endpoint denied')
    endpoint = 'user' if path == 'user' else 'repos/' + REPO + path
    args = [gh(), 'api', '--method', 'POST' if body is not None else 'GET', endpoint]
    if body is not None:
        args += ['--input', '-']
    output = bounded_process(args, canonical(body) if body is not None else None,
                             limit=65536 if raw else MAX_API)
    return output if raw else parse(output, MAX_API)


def download_challenge(run_id, attempt, phase):
    result = gh_api(f'/actions/runs/{run_id}/artifacts?per_page=100')
    require(result['total_count'] == len(result['artifacts']) < 100, 'Incomplete challenge artifacts')
    matches = [a for a in result['artifacts'] if a['name'] == f'owner-challenge-{run_id}-{attempt}-{phase}']
    require(len(matches) <= 1, 'Duplicate challenge artifacts')
    if not matches:
        return None
    a = matches[0]
    require(not a['expired'] and a['workflow_run']['id'] == run_id
            and a['workflow_run']['repository_id'] == REPO_ID and 0 < a['size_in_bytes'] <= 65536,
            'Challenge artifact provenance')
    data = gh_api('/actions/artifacts/' + str(a['id']) + '/zip', raw=True)
    require(a['digest'] == 'sha256:' + hashlib.sha256(data).hexdigest(), 'Challenge artifact digest')
    # A 64 KiB ZIP ceiling bounds central-directory allocation before ZipFile parses it.
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        require(len(entries) == 1 and entries[0].filename == 'challenge.json' and entries[0].file_size <= 16384
                and not entries[0].flag_bits & 1 and entries[0].compress_type in (0, 8)
                and entries[0].external_attr >> 16 & 0o170000 != 0o120000, 'Challenge archive inventory')
        with archive.open(entries[0]) as source:
            return parse(source.read(16385), 16384)


def attend(args):
    require(args.approve_public_evidence, 'Separate public disclosure approval required')
    p, _ = load_policy()
    identity = gh_api('user')
    require(identity['id'] == OWNER and identity['login'] == 'oangsa', 'Owner CLI identity required')
    require(Path(args.key).is_file(), 'Dedicated local production key required')
    inbox = gh_api('/issues/' + str(p['inbox']))
    require(inbox['number'] == p['inbox'] and 'pull_request' not in inbox and inbox['state'] == 'open', 'Approved inbox unavailable')
    attempt = None
    for phase in ('preflight', 'final'):
        deadline = time.monotonic() + WAIT
        while time.monotonic() < deadline:
            run = gh_api('/actions/runs/' + str(args.run_id))
            require(run['path'] == WORKFLOW and run['event'] == 'workflow_run' and run['head_branch'] == 'main'
                    and run['head_sha'] == args.trusted_sha and run['repository']['id'] == REPO_ID
                    and run['head_repository']['id'] == REPO_ID, 'Trusted publisher run provenance')
            attempt = run['run_attempt'] if attempt is None else attempt
            require(run['run_attempt'] == attempt and run['status'] != 'completed', 'Publisher stopped or attempt changed')
            c = download_challenge(args.run_id, attempt, phase)
            if c is None:
                time.sleep(5)
                continue
            check_challenge(c, p)
            require(c['run_id'] == args.run_id and c['attempt'] == attempt and c['phase'] == phase
                    and c['trusted_sha'] == args.trusted_sha and c['binding']['source']['head_sha'] == args.source_sha
                    and c['binding']['intent']['version'] == args.version, 'Owner expected source/version mismatch')
            print(json.dumps(c, indent=2), flush=True)
            print('This public response will disclose Rulesets, bypass actors, source, intent and run identities.', flush=True)
            require(input(f'Type APPROVE {phase} {args.version} to collect and sign fresh evidence: ').strip()
                    == f'APPROVE {phase} {args.version}', 'Owner declined gate')
            start = int(time.time())
            snapshot = collect(gh_api)
            validate_disclosure(snapshot, p)  # Before display, signing or any public POST.
            from release_promotion import protected_branches
            protected_branches(controls_call(gh_api, snapshot), strict=True)
            print(json.dumps(snapshot, indent=2), flush=True)
            require(input('Type SIGN to attest these exact live controls: ').strip() == 'SIGN', 'Owner declined controls')
            end = int(time.time())
            require(end - start <= 45, 'Collection/confirmation exceeded 45 seconds; rerun helper')
            payload = dict(schema=1, challenge=c, collection_start=start, collection_end=end,
                           expires=start + TTL, controls=snapshot)
            response = sign(payload, args.key)
            verify(response, c, set())  # Refuse a local key absent from approved policy.
            wrapper = dict(challenge_sha256=digest(c), response=response)
            body = canonical(wrapper).decode()
            require(len(body.encode()) <= MAX_MESSAGE, 'Public comment size limit')
            current = gh_api('/actions/runs/' + str(args.run_id))
            require(current['run_attempt'] == attempt and current['status'] != 'completed', 'Publisher canceled before send')
            # Intentionally one POST: lost response requires checking the inbox, never blind repost.
            posted = gh_api(f'/issues/{p["inbox"]}/comments', {'body': body}, inbox=p['inbox'])
            print('Posted ' + posted['html_url'] + '; consult publisher for acceptance.', flush=True)
            break
        else:
            raise ValueError('Owner helper attendance timeout')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', type=int, required=True)
    parser.add_argument('--trusted-sha', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--key', required=True)
    parser.add_argument('--approve-public-evidence', action='store_true')
    attend(parser.parse_args())
