"""Narrow patch authority; owner review binds exact GitHub patches and prerequisites."""
import hashlib
import io
import json
import re
import struct
import tomllib
import zipfile

GOVERNANCE_PATHS = {
    '.github/workflows/ci-release.yml', '.github/workflows/linux.yml',
    '.github/workflows/beta-release.yml', '.github/workflows/release-promotion.yml',
    'scripts/release_promotion.py', 'scripts/hotfix_policy.py', 'scripts/branch-flow.py',
    'scripts/publish-release.py', 'scripts/publish-beta.py', 'scripts/test.ps1',
    'scripts/package.ps1',
    'scripts/test-release-promotion.py', 'scripts/test-hotfix-policy.py',
    'docs/RELEASE-GOVERNANCE.md', 'docs/HOTFIX-RELEASE-STRATEGY.md',
    '.github/owner-evidence-policy.json', 'scripts/owner_evidence.py',
    'scripts/test-owner-evidence.py', 'docs/OWNER-EVIDENCE.md',
    'docs/SANDBOX-RELEASE-LIFECYCLE.md',
}
HOTFIX_FIELDS = {'schema', 'id', 'version', 'channel', 'source', 'baseline_tag',
                'baseline_sha', 'base_sha', 'reviewed_sha', 'commits', 'paths', 'patch_sha256', 'review'}
WINDOWS_SYS_FEATURE_ADDITIONS = {
    'Win32_System_Diagnostics_Debug',
    'Win32_System_Diagnostics_ToolHelp',
    'Win32_System_RemoteDesktop',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_source(call, sha, data):
    """Compare every tracked source blob; never extract or execute the source ZIP."""
    tree = call(f'/git/trees/{sha}?recursive=1')
    require(tree['truncated'] is False and 0 < len(tree['tree']) < 5000, 'Incomplete source tree evidence')
    expected = {e['path']: e['sha'] for e in tree['tree'] if e['type'] == 'blob' and e['mode'] in ('100644', '100755')}
    require(all(e['type'] == 'tree' or (e['type'] == 'blob' and e['mode'] in ('100644', '100755')) for e in tree['tree']),
            'Unsupported source submodule or symlink')
    end = data.rfind(b'PK\x05\x06', max(0, len(data) - 65557))
    require(end >= 0 and end + 22 <= len(data), 'Invalid source ZIP footer')
    disk, directory_disk, local_count, count, size, offset, comment = struct.unpack_from('<4H2IH', data, end + 4)
    require(disk == directory_disk == 0 and local_count == count and 0 < count < 5000
            and size <= 2 * 1024 * 1024 and offset + size == end and end + 22 + comment == len(data)
            and data[max(0, end - 20):end - 16] != b'PK\x06\x07', 'Source ZIP directory budget exceeded')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        names = [e.filename for e in entries if not e.is_dir()]
        require(len(entries) == count and len(names) == len(set(names)) and set(names) == set(expected),
                'Source ZIP inventory differs from exact commit')
        require(sum(e.file_size for e in entries) <= 256 * 1024 * 1024, 'Source expansion budget exceeded')
        for entry in entries:
            require(entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED) and not entry.flag_bits & 1
                    and entry.external_attr >> 16 & 0o170000 != 0o120000, 'Unsupported source ZIP entry')
            if entry.is_dir():
                require(entry.file_size == 0, 'Source directory has payload')
                continue
            digest = hashlib.sha1(b'blob ' + str(entry.file_size).encode() + b'\0')
            count = 0
            with archive.open(entry) as source:
                while chunk := source.read(1024 * 1024):
                    count += len(chunk)
                    require(count <= entry.file_size, 'Source entry exceeds declared size')
                    digest.update(chunk)
            require(count == entry.file_size and digest.hexdigest() == expected[entry.filename], 'Source blob SHA mismatch')


def patch_report(result, base, head, governance=False):
    require(result['status'] == 'ahead' and result['base_commit']['sha'] == base
            and result['merge_base_commit']['sha'] == base, 'Source must descend from authorized base')
    commits, files = result['commits'], result['files']
    require(0 < len(commits) == result['total_commits'] < 100 and 0 < len(files) < 100,
            'Incomplete or oversized patch evidence')
    previous = base
    seen = {base}
    for commit in commits:
        parents = [p['sha'] for p in commit['parents']]
        require(re.fullmatch('[a-f0-9]{40}', commit['sha']) and commit['sha'] not in seen
                and (0 < len(parents) <= 2 and set(parents) <= seen if governance else parents == [previous]),
                'Nonlinear or unrelated commits require separate review')
        previous = commit['sha']
        seen.add(previous)
    require(previous == head, 'Incomplete source commit history')
    records = []
    for file in files:
        path = file['filename']
        require(file['status'] in ('added', 'modified', 'removed') and 'previous_filename' not in file
                and re.fullmatch('[a-f0-9]{40}', file['sha']) and isinstance(file.get('patch'), str)
                and file['patch'], 'Missing patch or ambiguous file evidence')
        if governance:
            require(path in GOVERNANCE_PATHS, 'Non-governance change in stable base: ' + path)
        else:
            allowed = path in {'Cargo.toml', 'Cargo.lock', 'README.md', 'CHANGELOG.md', 'desktop/CHANGELOG.txt'} or (
                path.startswith(('src/', 'desktop/', 'desktop.tests/', 'desktop.linux/', 'tests/'))
                or re.fullmatch(r'scripts/test-[A-Za-z0-9_.-]+', path)
                or re.fullmatch(r'docs/RELEASE-[0-9]+\.[0-9]+\.[0-9]+\.md', path))
            require(allowed, 'Unexpected hotfix path: ' + path)
            added = '\n'.join(line[1:] for line in file['patch'].splitlines()
                              if line.startswith('+') and not line.startswith('+++'))
            require(not re.search(r'identity[_-]poc|machine[_-]?key|premium|licensing|license[_-]?backend|device[_-]?(?:activation|identity)|provisioning|stripe|payment',
                                  path + '\n' + added, re.I), 'Premium/experimental content requires rejection: ' + path)
            require(not path.endswith(('.csproj', 'packages.lock.json')), 'Dependency changes require separate authorization')
        records.append({k: file[k] for k in ('filename', 'status', 'sha', 'patch')})
    records.sort(key=lambda item: item['filename'])
    require(len({f['filename'] for f in records}) == len(records), 'Duplicate patch paths')
    digest = hashlib.sha256(json.dumps(records, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    return dict(commits=[c['sha'] for c in commits], paths=[f['filename'] for f in records], patch_sha256=digest)


def governance_history(call, baseline, base):
    if base == baseline:
        return
    report = patch_report(call(f'/compare/{baseline}...{base}?per_page=100'), baseline, base, governance=True)
    for commit in report['commits']:
        changed = call(f'/commits/{commit}?per_page=100')['files']
        require(len(changed) < 100 and all(f['filename'] in GOVERNANCE_PATHS
                and f.get('previous_filename', f['filename']) in GOVERNANCE_PATHS for f in changed),
                'Application changes hidden in stable governance history')


def hotfix_report(call, base, reviewed):
    report = patch_report(call(f'/compare/{base}...{reviewed}?per_page=100'), base, reviewed)
    previous = base
    for sha in report['commits']:
        commit = call(f'/commits/{sha}?per_page=100')
        require(commit['sha'] == sha, 'Wrong individual commit evidence')
        patch_report(dict(status='ahead', base_commit={'sha': previous}, merge_base_commit={'sha': previous},
                          total_commits=1, commits=[commit], files=commit['files']), previous, sha)
        require(call(f'/compare/{sha}...dev')['status'] == 'diverged', 'Premium dev commit cannot enter hotfix history')
        previous = sha
    return report


def normalize_windows_sys_features(before, after):
    old = before.get('target', {}).get('cfg(windows)', {}).get('dependencies', {}).get('windows-sys')
    new = after.get('target', {}).get('cfg(windows)', {}).get('dependencies', {}).get('windows-sys')
    if old == new:
        return
    require(isinstance(old, dict) and isinstance(new, dict),
            'Cargo.toml dependency changes require separate authorization')
    require(old.get('version') == new.get('version') == '0.61',
            'windows-sys dependency version changes require separate authorization')
    old_features = old.get('features', [])
    new_features = new.get('features', [])
    require(isinstance(old_features, list) and isinstance(new_features, list)
            and all(isinstance(item, str) for item in old_features + new_features)
            and len(old_features) == len(set(old_features))
            and len(new_features) == len(set(new_features)),
            'Invalid windows-sys feature list')
    old_set, new_set = set(old_features), set(new_features)
    require(old_set <= new_set and new_set - old_set <= WINDOWS_SYS_FEATURE_ADDITIONS,
            'Unapproved windows-sys feature change')
    normalized = new.copy()
    if 'features' in old:
        normalized['features'] = old['features']
    else:
        normalized.pop('features', None)
    after['target']['cfg(windows)']['dependencies']['windows-sys'] = normalized


def check_branch_flow(event, call):
    """Read-only PR admission; successful admission never authorizes publication."""
    from release_promotion import REPO, content
    if 'pull_request' not in event:
        return 'ordinary-ci'
    pr = event['pull_request']
    base, head = pr['base'], pr['head']
    branch = head['ref']
    if base['ref'] == 'dev':
        require(branch.startswith(('feature/', 'codex/')), 'Feature/codex branches target dev')
        return 'development'
    require(base['ref'] == 'main' and head['repo']['full_name'] == REPO, 'Official main PR required')
    if branch == 'dev':
        return 'feature-promotion'
    require(branch == 'codex/hotfix-release-lane' or re.fullmatch(r'hotfix/[0-9]+\.[0-9]+\.[0-9]+', branch),
            'Unauthorized main source branch')
    latest = call('/releases/latest')
    tag = latest['tag_name']
    require(not latest['draft'] and not latest['prerelease'] and re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+', tag),
            'Verified published stable required')
    obj = call('/git/ref/tags/' + tag)['object']
    if obj['type'] == 'tag': obj = call('/git/tags/' + obj['sha'])['object']
    require(obj['type'] == 'commit', 'Stable tag must resolve to commit')
    baseline = obj['sha']
    if branch == 'codex/hotfix-release-lane':
        governance_history(call, baseline, head['sha'])
        require(tomllib.loads(content(call, 'Cargo.toml', head['sha']))['package']['version'] == tag[1:],
                'Governance migration must retain stable version')
        return 'governance-only'
    numbers = list(map(int, tag[1:].split('.')))
    numbers[2] += 1
    require(branch == 'hotfix/' + '.'.join(map(str, numbers)), 'Hotfix branch must target next patch')
    governance_history(call, baseline, base['sha'])
    reviewed = head['sha']
    final = call(f'/commits/{reviewed}?per_page=100')
    files = final['files']
    if len(files) == 1 and files[0]['filename'].startswith('.github/release-intents/'):
        require(files[0]['status'] == 'added' and len(final['parents']) == 1, 'Fresh intent-only final commit required')
        reviewed = final['parents'][0]['sha']
    report = hotfix_report(call, base['sha'], reviewed)
    return json.dumps(dict(report, semantic_review='Owner must inspect full blobs, bug scope and cherry-pick prerequisites'), sort_keys=True)


def verify_hotfix(call, intent, pr, merge, path, source, content):
    require(set(intent) == HOTFIX_FIELDS and type(intent['schema']) is int and intent['schema'] == 2
            and intent['source'] == 'hotfix', 'Invalid hotfix intent contract')
    for field in ('baseline_sha', 'base_sha', 'reviewed_sha'):
        require(isinstance(intent[field], str) and re.fullmatch('[a-f0-9]{40}', intent[field]), 'Invalid hotfix identity')
    tag, baseline, base, reviewed = (intent[k] for k in ('baseline_tag', 'baseline_sha', 'base_sha', 'reviewed_sha'))
    require(isinstance(tag, str) and re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag),
            'Published stable baseline required')
    numbers = list(map(int, tag[1:].split('.')))
    numbers[2] += 1
    require(intent['version'] == '.'.join(map(str, numbers)), 'Hotfix must increment published stable PATCH only')
    release = call('/releases/tags/' + tag)
    require(release['tag_name'] == tag and not release['draft'] and not release['prerelease'], 'Unpublished baseline')
    obj = call('/git/ref/tags/' + tag)['object']
    if obj['type'] == 'tag': obj = call('/git/tags/' + obj['sha'])['object']
    require(obj['type'] == 'commit' and obj['sha'] == baseline, 'Stable baseline tag mismatch')
    latest = call('/releases/latest')
    require(not latest['draft'] and not latest['prerelease']
            and latest['tag_name'] in (tag, 'v' + intent['version']), 'Stale hotfix baseline')
    head = pr['head']['sha']
    require(pr['head']['ref'] == 'hotfix/' + intent['version'] and pr['base']['sha'] == base
            and [p['sha'] for p in merge['parents']] == [base, head], 'Hotfix merge must preserve authorized base and head')
    final = call(f'/commits/{head}?per_page=100')
    require(merge['commit']['tree']['sha'] == final['commit']['tree']['sha'], 'Merged tree differs from reviewed hotfix head')
    require([p['sha'] for p in final['parents']] == [reviewed]
            and [(f['filename'], f['status']) for f in final['files']] == [(path, 'added')],
            'Final hotfix commit must add only the reviewed intent')
    governance_history(call, baseline, base)
    report = hotfix_report(call, base, reviewed)
    require(all(report[k] == intent[k] for k in report), 'Intent does not match exact reviewed patch/commits')
    review = intent['review']
    require(isinstance(review, dict) and set(review) == {'bug', 'dependencies', 'premium_excluded'}
            and review['premium_excluded'] is True
            and all(isinstance(review[k], str) and len(review[k].strip()) >= 20 for k in ('bug', 'dependencies')),
            'Owner semantic/dependency review required; partial cherry-picks need prerequisites reviewed')
    for name in ('Cargo.toml', 'Cargo.lock'):
        before = tomllib.loads(content(call, name, base))
        after = tomllib.loads(content(call, name, source))
        if name == 'Cargo.toml':
            require(before['package']['version'] == tag[1:] and after['package']['version'] == intent['version'],
                    'Hotfix manifest version mismatch')
            after['package']['version'] = before['package']['version']
            normalize_windows_sys_features(before, after)
        else:
            old = next(p for p in before['package'] if p['name'] == 'rolauncher')
            new = next(p for p in after['package'] if p['name'] == 'rolauncher')
            require(old['version'] == tag[1:] and new['version'] == intent['version'], 'Hotfix lock version mismatch')
            new['version'] = old['version']
        require(before == after, 'Dependency changes require separate authorization')
    return intent
