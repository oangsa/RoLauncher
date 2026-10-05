"""Prepare one version bump on a feature branch; write reviewed notes before pushing."""
import argparse
from pathlib import Path
import re
import subprocess
from datetime import date

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('kind', choices=['feature', 'fix', 'major'])
args = parser.parse_args()
branch = subprocess.check_output(['git', 'branch', '--show-current'], cwd=root, text=True).strip()
if not branch.startswith('feature/'):
    parser.error('Prepare releases on a feature/* branch, then open a PR into dev.')
manifest = root / 'Cargo.toml'
text = manifest.read_text()
old = re.search(r'^version = "(\d+\.\d+\.\d+)"', text, re.M)[1]
major, minor, patch = map(int, old.split('.'))
new = {'feature': (major, minor + 1, 0), 'fix': (major, minor, patch + 1), 'major': (major + 1, 0, 0)}[args.kind]
version = '.'.join(map(str, new))
notes = root / f'docs/RELEASE-{version}.md'
if notes.exists():
    parser.error('Release notes already exist; bump only once per release.')
manifest.write_text(text.replace(f'version = "{old}"', f'version = "{version}"', 1), encoding='utf-8')
lock = root / 'Cargo.lock'
lock.write_text(re.sub(r'(name = "rolauncher"\s+version = ")[^"]+', lambda m: m[1] + version, lock.read_text()), encoding='utf-8')
readme = root / 'README.md'
readme.write_text(readme.read_text(encoding='utf-8').replace(f'# RoLauncher {old}', f'# RoLauncher {version}', 1), encoding='utf-8')
changelog = root / 'CHANGELOG.md'
changelog.write_text(changelog.read_text(encoding='utf-8').replace('# Changelog\n', f'# Changelog\n\n## [{version}] — {date.today()}\n\nDescribe the changes and validation before opening the PR.\n', 1), encoding='utf-8')
desktop = root / 'desktop/CHANGELOG.txt'
desktop.write_text(f'{version} — Release notes\n\nDescribe the changes before opening the PR.\n\n' + desktop.read_text(encoding='utf-8'), encoding='utf-8')
notes.write_text(f'# RoLauncher {version}\n\nDescribe the user-visible changes, upgrade behavior and validation before opening the PR.\n', encoding='utf-8')
print(f'Prepared {old} -> {version}. Finish CHANGELOG.md, desktop/CHANGELOG.txt and {notes.name}, then run scripts/test.ps1.')
