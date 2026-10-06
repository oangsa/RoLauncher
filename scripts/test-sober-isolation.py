"""Build harmless local Flatpak fixtures and run the real Linux isolation regression.

Requires a native Linux toolchain, Flatpak, a static C compiler and D-Bus session.
Uses a disposable per-test Flatpak installation; never launches or edits Sober.
"""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
APP = 'org.rolauncher.IsolationProbe'
RUNTIME = 'org.rolauncher.ProbePlatform'

C_SOURCE = r'''
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>
static unsigned long inode(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? st.st_ino : 0;
}
static unsigned long device(const char *path) {
    struct stat st;
    return stat(path, &st) == 0 ? st.st_dev : 0;
}
int main(void) {
    const char *name = getenv("ROLAUNCHER_PROBE_NAME");
    if (!name) return 2;
    char lock[512];
    snprintf(lock, sizeof(lock), "/tmp/rolauncher-probe-%s", name);
    int owned = mkdir(lock, 0700) == 0;
    printf("{\"locked\":%s,\"tmp\":%lu,\"tmp_dev\":%lu,\"runtime\":%lu,\"runtime_dev\":%lu,\"ipc\":%lu,\"pid\":%lu,\"net\":%lu}\n",
        owned ? "true" : "false", inode("/tmp"), device("/tmp"),
        inode(getenv("XDG_RUNTIME_DIR")), device(getenv("XDG_RUNTIME_DIR")),
        inode("/proc/self/ns/ipc"), inode("/proc/self/ns/pid"), inode("/proc/self/ns/net"));
    fflush(stdout);
    getchar();
    if (owned) rmdir(lock);
    return 0;
}
'''

def main():
    target = ROOT / 'target'
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='flatpak-isolation-', dir=target) as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        env.update({
            'XDG_DATA_HOME': str(root / 'xdg-data'),
            'XDG_CONFIG_HOME': str(root / 'xdg-config'),
            'XDG_CACHE_HOME': str(root / 'xdg-cache'),
            'ROLAUNCHER_ISOLATION_FIXTURE_ROOT': str(root),
        })
        def run(*args):
            subprocess.run(args, cwd=ROOT, env=env, check=True, timeout=180)
        runtime = root / 'runtime'
        (runtime / 'usr/bin').mkdir(parents=True)
        (runtime / 'files').mkdir()
        (runtime / 'metadata').write_text(f'[Runtime]\nname={RUNTIME}\n')
        app = root / 'app'
        (app / 'files/bin').mkdir(parents=True)
        (app / 'metadata').write_text(
            f'[Application]\nname={APP}\nruntime={RUNTIME}/x86_64/1\ncommand=probe\n'
            '[Context]\nshared=network;ipc;\n')
        source = root / 'probe.c'
        source.write_text(C_SOURCE)
        run('cc', '-static', '-O2', str(source), '-o', str(app / 'files/bin/probe'))
        repo = root / 'repo'
        run('flatpak', 'build-export', '--runtime', str(repo), str(runtime), '1')
        run('flatpak', 'build-finish', str(app))
        run('flatpak', 'build-export', str(repo), str(app), '1')
        run('flatpak', 'remote-add', '--user', '--no-gpg-verify', 'fixture', str(repo))
        run('flatpak', 'install', '--user', '--noninteractive', 'fixture', RUNTIME, APP)
        run('flatpak', 'list', '--user', '--columns=application,branch')
        run('cargo', 'test', '--locked', '--offline',
            'platform::linux::tests::flatpak_runtime_isolation_separates_locks',
            '--', '--ignored', '--nocapture')

if __name__ == '__main__':
    main()
