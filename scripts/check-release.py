"""Fail CI when manifest, lockfile, user-visible version or release notes disagree."""
from pathlib import Path
import re
import tomllib
from versioning import core

root = Path(__file__).resolve().parent.parent
manifest = tomllib.loads((root / 'Cargo.toml').read_text())
version = manifest['package']['version']
base = core(version)
assert all(int(part) <= 65535 for part in base.split('.')), 'Windows version components must fit 16 bits'
lock = tomllib.loads((root / 'Cargo.lock').read_text())
assert next(p for p in lock['package'] if p['name'] == manifest['package']['name'])['version'] == version
assert (root / 'README.md').read_text(encoding='utf-8').startswith(f'# RoLauncher {version}\n')
notes = (root / f'docs/RELEASE-{base}.md').read_text(encoding='utf-8')
assert notes.startswith(f'# RoLauncher {base}\n') and len(notes.strip()) > 120, 'Write useful release notes'
assert 'Describe the user-visible changes' not in notes, 'Replace generated draft release notes'
assert f'## [{base}]' in (root / 'CHANGELOG.md').read_text(encoding='utf-8'), 'Add a changelog entry'
assert (root / 'desktop/CHANGELOG.txt').read_text(encoding='utf-8').startswith(base + ' ')
print(f'Version {version}, lockfile, README, desktop changelog and release notes agree.')
