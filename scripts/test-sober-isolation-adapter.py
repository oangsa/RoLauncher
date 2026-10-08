"""Check the optional beta adapter with a fake Flatpak, never with a game client."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parent.parent
adapter = root / 'scripts/sober-isolation/flatpak'

with tempfile.TemporaryDirectory(prefix='isolation-adapter-') as temporary:
    directory = Path(temporary)
    entry = directory / 'adapter'
    shutil.copyfile(adapter, entry)
    shutil.copyfile(adapter.with_name('options.json'), entry.with_name('options.json'))
    entry.chmod(0o700)
    real = directory / 'real-flatpak'
    real.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
if args[0] == "run":
    assert args[-2:] == ["org.vinegarhq.Sober", "roblox-player:1+gameinfo:FIXTURE"]
    home = Path(os.environ["HOME"])
    assert "--sandbox" in args and "--unshare=ipc" in args
    assert "--share=network" in args
    assert "--device=input" in args
    assert f"--filesystem={home}" in args
    assert f"--env=HOME={home}" in args
    assert f"--env=XDG_DATA_HOME={home}/.var/app/org.vinegarhq.Sober/data" in args
else:
    assert args == ["info", "--show-ref", "org.vinegarhq.Sober"]
print("Adapter forwarded the expected arguments without splitting them.")
''')
    real.chmod(0o700)
    env = os.environ.copy()
    env.update({'ROLAUNCHER_REAL_FLATPAK': str(real),
                'HOME': str(directory / 'path with spaces/sober-instances/123')})
    args = ['run', 'org.vinegarhq.Sober', 'roblox-player:1+gameinfo:FIXTURE']
    for command in [args, ['info', '--show-ref', 'org.vinegarhq.Sober']]:
        result = subprocess.run([str(entry), *command], env=env,
                                capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, result.stderr
        assert 'gameinfo' not in result.stdout + result.stderr
    for invalid in [str(directory), 'relative/sober-instances/123',
                    str(directory / 'bad:path/sober-instances/123')]:
        env['HOME'] = invalid
        result = subprocess.run([str(entry), *args], env=env,
                                capture_output=True, text=True, timeout=10)
        assert result.returncode != 0
        assert 'gameinfo' not in result.stdout + result.stderr
    env['ROLAUNCHER_REAL_FLATPAK'] = str(entry)
    result = subprocess.run([str(entry), 'info'], env=env,
                            capture_output=True, text=True, timeout=10)
    assert result.returncode != 0

print('Adapter: protocol argument preserved, unrelated commands forwarded, unsafe homes and recursion rejected; no launch URL printed.')
