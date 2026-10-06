"""Exercise archive containment, required files, executable modes and rollback."""
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("linux_update", Path(__file__).with_name("apply-update-linux.py"))
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)


def archive(path, extra=None):
    with tarfile.open(path, "w:gz") as package:
        for name in ("RoLauncher", "desktop/RoLauncher.Desktop", "LICENSE", "AGENTS.md"):
            info = tarfile.TarInfo("rolauncher-v1.1.0/" + name)
            info.size = 3
            info.mode = 0o755 if name.startswith(("RoLauncher", "desktop/")) else 0o644
            package.addfile(info, io.BytesIO(b"new"))
        if extra:
            package.addfile(extra, io.BytesIO(b"x" * extra.size) if extra.isfile() else None)


class UpdateTests(unittest.TestCase):
    def test_complete_archive_preserves_executables(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / "update.tar.gz"
            archive(source)
            package = updater.extract_package(source, root / "stage", "1.1.0")
            self.assertEqual((package / "RoLauncher").read_bytes(), b"new")
            if os.name != "nt":
                self.assertTrue((package / "RoLauncher").stat().st_mode & 0o111)

    def test_traversal_links_wrong_root_and_duplicates_are_rejected(self):
        for name, kind in (("rolauncher-v1.1.0/../../escape", tarfile.REGTYPE),
                           ("/absolute", tarfile.REGTYPE), ("other/app", tarfile.REGTYPE),
                           ("rolauncher-v1.1.0/link", tarfile.SYMTYPE),
                           ("rolauncher-v1.1.0/hardlink", tarfile.LNKTYPE),
                           ("rolauncher-v1.1.0/RoLauncher", tarfile.REGTYPE)):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                source = root / "update.tar.gz"
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = "../../escape"
                archive(source, entry)
                with self.assertRaises(ValueError):
                    updater.extract_package(source, root / "stage", "1.1.0")
                self.assertFalse((root / "escape").exists())

    def test_failed_relaunch_restores_previous_application_and_preserves_data(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            root, data = folder / "app", folder / "data"
            root.mkdir(); data.mkdir()
            (root / "RoLauncher").write_text("old")
            (data / "accounts.json").write_text("unchanged")
            source = folder / "update.tar.gz"
            archive(source)
            with source.open("rb") as file:
                digest = updater.hashlib.file_digest(file, "sha256").hexdigest()
            config = dict(root=str(root), data=str(data), archive=str(source), hash=digest,
                          version="1.1.0", parent=111, desktop=222, port=38471)
            poll = Mock(); poll.poll.return_value = [(1, 1)]
            with patch.object(updater.sys, "stdin", io.StringIO(json.dumps(config) + "\ncommit\n")), \
                 patch.object(updater.sys, "stdout", io.StringIO()), \
                 patch.object(updater.os, "pidfd_open", return_value=7, create=True), \
                 patch.object(updater.os, "close"), patch.object(updater.select, "poll", return_value=poll, create=True), \
                 patch.object(updater.select, "POLLIN", 1, create=True), \
                 patch.object(updater.subprocess, "Popen", side_effect=OSError):
                with self.assertRaises(OSError):
                    updater.main()
            self.assertEqual((root / "RoLauncher").read_text(), "old")
            self.assertEqual((data / "accounts.json").read_text(), "unchanged")


if __name__ == "__main__":
    unittest.main()
