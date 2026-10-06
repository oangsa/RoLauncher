"""Check beta publication guards without contacting GitHub or using credentials."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("publish_beta", Path(__file__).with_name("publish-beta.py"))
beta = importlib.util.module_from_spec(spec)
spec.loader.exec_module(beta)
SHA = "a" * 40


class GitHubFixture:
    def __init__(self, release=None, sha=SHA):
        self.release = release
        self.sha = sha
        self.assets = {}
        self.calls = []

    def __call__(self, path, method="GET", body=None, missing_ok=False, binary=None):
        self.calls.append((path, method, body))
        if path.startswith("/git/ref/tags/"):
            return {"object": {"type": "commit", "sha": self.sha}}
        if path.startswith("/releases/tags/"):
            return self.release
        if path == "/releases" and method == "POST":
            self.release = dict(body, id=1, upload_url="https://uploads.github.com/repos/oangsa/RoLauncher/releases/1/assets{?name,label}", html_url="https://github.com/oangsa/RoLauncher/releases/tag/beta-v1.1.0")
            return self.release
        if path == "/releases/1/assets":
            return list(self.assets.values())
        if path.startswith("https://uploads.github.com/"):
            name = path.split("?name=")[1]
            self.assets[name] = dict(name=name, id=len(self.assets) + 1, size=len(binary), digest="sha256:" + hashlib.sha256(binary).hexdigest())
            return self.assets[name]
        if path == "/releases/1" and method == "PATCH":
            self.release.update(body)
            return self.release
        raise AssertionError((path, method))


class BetaChecks(unittest.TestCase):
    def test_publishes_only_after_complete_upload_and_never_latest(self):
        api = GitHubFixture()
        beta.publish(api, {"test.zip": b"verified"}, "1.1.0", SHA, "notes")
        self.assertTrue(api.release["prerelease"])
        self.assertFalse(api.release["draft"])
        self.assertEqual(api.calls[-1][2]["make_latest"], "false")
        create = next(body for path, method, body in api.calls if path == "/releases")
        self.assertNotIn("target_commitish", create)
        self.assertTrue(create["draft"])
        self.assertEqual(create["make_latest"], "false")

    def test_published_assets_are_preserved(self):
        api = GitHubFixture(dict(draft=False, prerelease=True, html_url="published"))
        self.assertEqual(beta.publish(api, {}, "1.1.0", SHA, "notes"), "published")
        self.assertTrue(all(method == "GET" for _, method, _ in api.calls))

    def test_wrong_tag_commit_is_rejected_before_mutation(self):
        api = GitHubFixture(sha="b" * 40)
        with self.assertRaisesRegex(ValueError, "tag"):
            beta.publish(api, {}, "1.1.0", SHA, "notes")
        self.assertEqual(len(api.calls), 1)

    def test_other_draft_is_rejected(self):
        api = GitHubFixture(dict(draft=True, prerelease=True, body="other commit"))
        with self.assertRaisesRegex(ValueError, "another commit"):
            beta.publish(api, {}, "1.1.0", SHA, "notes")
        self.assertTrue(all(method == "GET" for _, method, _ in api.calls))

    def test_corrupt_files_are_rejected_and_fragments_are_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            dist = Path(temp)
            prefix = "rolauncher-v1.1.0"
            fragments = {}
            for suffix, endings in (("-SHA256SUMS.txt", ["-setup-x64.exe", "-windows-x64.zip", "-source.zip"]),
                                    ("-linux-SHA256SUMS.txt", ["-linux-x64.tar.gz"])):
                lines = []
                for ending in endings:
                    name = prefix + ending
                    (dist / name).write_bytes(name.encode())
                    lines.append(hashlib.sha256(name.encode()).hexdigest() + "  " + name + "\n")
                fragments[suffix] = "".join(lines)
                (dist / (prefix + suffix)).write_text(fragments[suffix])
            files = beta.checked_files(dist, "1.1.0")
            self.assertEqual(len(files), 5)
            self.assertEqual(len(files[prefix + "-SHA256SUMS.txt"].splitlines()), 4)
            for suffix, original in fragments.items():
                self.assertEqual((dist / (prefix + suffix)).read_text(), original)
            (dist / (prefix + "-linux-x64.tar.gz")).write_bytes(b"corrupt")
            with self.assertRaisesRegex(ValueError, "checksum"):
                beta.checked_files(dist, "1.1.0")


if __name__ == "__main__":
    unittest.main()
