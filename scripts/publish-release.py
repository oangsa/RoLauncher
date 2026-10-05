"""Publish checked artifacts as a draft first; never replace a published release."""
import hashlib
import json
import os
from pathlib import Path
import re
from urllib.error import HTTPError
from urllib.request import Request, urlopen

root = Path(__file__).resolve().parent.parent
version = re.search(r'^version = "(\d+\.\d+\.\d+)"', (root / 'Cargo.toml').read_text(), re.M)[1]
tag = 'v' + version
repo = os.environ['GITHUB_REPOSITORY']
sha = os.environ['GITHUB_SHA']
assert repo == 'oangsa/RoLauncher', 'Publish only from the official repository'
assert os.environ.get('GITHUB_REF') == 'refs/heads/main', 'Publish only from main'
prefix = f'rolauncher-{tag}'
dist = root / 'dist'
sums = dist / f'{prefix}-SHA256SUMS.txt'
expected = {f'{prefix}-setup-x64.exe', f'{prefix}-windows-x64.zip', f'{prefix}-source.zip'}
seen = set()
for line in sums.read_text().splitlines():
    digest, filename = line.split()
    assert filename in expected and filename not in seen, 'Unexpected or duplicated release artifact'
    assert hashlib.sha256((dist / filename).read_bytes()).hexdigest() == digest, 'Release checksum mismatch'
    seen.add(filename)
assert seen == expected, 'Release artifact missing'
files = sorted(expected | {sums.name})
notes = (root / f'docs/RELEASE-{version}.md').read_text(encoding='utf-8')
assert notes.startswith(f'# RoLauncher {version}\n')
headers = {'Authorization': 'Bearer ' + os.environ['GITHUB_TOKEN'], 'Accept': 'application/vnd.github+json',
           'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'RoLauncher-release'}
api = f'https://api.github.com/repos/{repo}'

def call(path, method='GET', body=None, missing_ok=False, binary=None):
    url = path if path.startswith('https://') else api + path
    data = binary if binary is not None else json.dumps(body).encode() if body is not None else None
    request_headers = dict(headers)
    if data is not None:
        request_headers['Content-Type'] = 'application/octet-stream' if binary is not None else 'application/json'
    try:
        with urlopen(Request(url, data=data, method=method, headers=request_headers), timeout=120) as response:
            content = response.read()
            return json.loads(content) if content else None
    except HTTPError as error:
        if error.code == 404 and missing_ok:
            return None
        raise RuntimeError(f'GitHub {method} failed (HTTP {error.code}).') from None

release = call('/releases/tags/' + tag, missing_ok=True)
if release and not release['draft']:
    print(f'{tag} is already published. Existing assets and tag are preserved.')
    raise SystemExit(0)
existing_tag = call('/git/ref/tags/' + tag, missing_ok=True)
if existing_tag:
    obj = existing_tag['object']
    if obj['type'] == 'tag':
        obj = call('/git/tags/' + obj['sha'])['object']
    assert obj['type'] == 'commit' and obj['sha'] == sha, 'An existing version tag points to another commit; bump the version'
if release:
    assert release['target_commitish'] == sha, 'Do not resume a draft belonging to a different commit'
    call('/releases/' + str(release['id']), 'PATCH', {'body': notes})
else:
    release = call('/releases', 'POST', {'tag_name': tag, 'target_commitish': sha,
                   'name': f'RoLauncher {version}', 'body': notes, 'draft': True, 'prerelease': False})
upload = release['upload_url'].split('{')[0]
assert upload.startswith(f'https://uploads.github.com/repos/{repo}/releases/'), 'Unexpected upload host'
assets = {a['name']: a for a in call('/releases/' + str(release['id']) + '/assets')}
for filename in files:
    # Interrupted drafts can resume; published release assets are never modified.
    if filename in assets:
        call('/releases/assets/' + str(assets[filename]['id']), 'DELETE')
    call(upload + '?name=' + filename, 'POST', binary=(dist / filename).read_bytes())
uploaded = call('/releases/' + str(release['id']) + '/assets')
assert {a['name'] for a in uploaded} == set(files), 'Incomplete or unexpected release assets'
for asset in uploaded:
    assert asset['size'] == (dist / asset['name']).stat().st_size, 'Incomplete uploaded asset'
    if asset.get('digest'):
        assert asset['digest'] == 'sha256:' + hashlib.sha256((dist / asset['name']).read_bytes()).hexdigest()
call('/releases/' + str(release['id']), 'PATCH', {'draft': False, 'make_latest': 'true'})
print(f'Published https://github.com/{repo}/releases/tag/{tag}')
