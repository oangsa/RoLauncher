"""Prepare an official verified package, await process exit, swap, and relaunch.

The caller waits for 'ready' before closing. Credentials never enter this helper.
"""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import select
import shutil
import subprocess
import sys
import tarfile
import tempfile


def extract_package(archive, directory, version):
    prefix = f"rolauncher-v{version}"
    seen = set()
    total = 0
    with tarfile.open(archive, "r:gz") as package:
        for entry in package:
            path = PurePosixPath(entry.name)
            if (path.is_absolute() or ".." in path.parts or "\\" in entry.name
                    or not path.parts or path.parts[0] != prefix or entry.name in seen
                    or not (entry.isfile() or entry.isdir())):
                raise ValueError("Invalid package entry")
            seen.add(entry.name)
            total += entry.size
            if total > 1024 * 1024 * 1024 or len(seen) > 20000:
                raise ValueError("Package too large")
            destination = directory.joinpath(*path.parts)
            if entry.isdir():
                destination.mkdir(parents=True, exist_ok=True)
            else:
                destination.parent.mkdir(parents=True, exist_ok=True)
                with package.extractfile(entry) as source, destination.open("xb") as output:
                    shutil.copyfileobj(source, output)
                destination.chmod(0o755 if entry.mode & 0o111 else 0o644)
    result = directory / prefix
    for required in ("RoLauncher", "desktop/RoLauncher.Desktop", "LICENSE", "AGENTS.md"):
        if not (result / required).is_file():
            raise ValueError("Incomplete package")
    if not os.access(result / "RoLauncher", os.X_OK) or not os.access(result / "desktop/RoLauncher.Desktop", os.X_OK):
        raise ValueError("Package executables are not executable")
    return result


def main():
    config = json.loads(sys.stdin.readline(32768))
    root = Path(config["root"]).resolve(strict=True)
    data = Path(config["data"]).resolve(strict=True)
    # An in-place swap must never remove a custom account data directory.
    if root == data or root in data.parents or not (root / "RoLauncher").is_file():
        raise ValueError("Data directory must be outside the application")
    archive = Path(config["archive"]).resolve(strict=True)
    if root == archive or root in archive.parents:
        raise ValueError("Update cache must be outside the application")
    with archive.open("rb") as source:
        if hashlib.file_digest(source, "sha256").hexdigest() != config["hash"]:
            raise ValueError("Checksum mismatch")
    handles = [os.pidfd_open(config["parent"]), os.pidfd_open(config["desktop"])]
    stage = Path(tempfile.mkdtemp(prefix=".rolauncher-update-", dir=root.parent))
    backup = stage / "previous"
    swapped = False
    relaunched = False
    try:
        prepared = extract_package(archive, stage, config["version"])
        print("ready", flush=True)
        if sys.stdin.readline(16).strip() != "commit":
            return
        for handle in handles:
            poll = select.poll()
            poll.register(handle, select.POLLIN)
            if not poll.poll(120000):
                raise TimeoutError("Application did not exit")
        root.rename(backup)
        try:
            prepared.rename(root)
            swapped = True
            subprocess.Popen([str(root / "RoLauncher"), "--data-dir", str(data), "--port", str(config["port"])],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True)
            relaunched = True
        except Exception:
            if swapped:
                root.rename(prepared)
            backup.rename(root)
            raise
    finally:
        for handle in handles:
            os.close(handle)
        # Keep the previous application recoverable even if rollback itself fails.
        if not backup.exists() or relaunched:
            shutil.rmtree(stage)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Avoid dumping config, user paths or any inherited process arguments.
        print("failed", flush=True)
        sys.exit(1)
