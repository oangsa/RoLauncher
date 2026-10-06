"""Publish checked Windows/Linux artifacts as a prerelease, preserving stable/latest."""
import hashlib
import json
import os
from pathlib import Path
import re
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def checked_files(dist, version):
    prefix = f"rolauncher-v{version}"
    windows = {f"{prefix}-setup-x64.exe", f"{prefix}-windows-x64.zip", f"{prefix}-source.zip"}
    linux = {f"{prefix}-linux-x64.tar.gz"}
    digests = {}
    for suffix, expected in (("-SHA256SUMS.txt", windows), ("-linux-SHA256SUMS.txt", linux)):
        seen = set()
        for line in (dist / (prefix + suffix)).read_text().splitlines():
            digest, filename = line.split()
            if filename not in expected or filename in seen or not re.fullmatch(r"[a-f0-9]{64}", digest):
                raise ValueError("Unexpected or duplicated beta artifact")
            with (dist / filename).open("rb") as source:
                actual = hashlib.file_digest(source, "sha256").hexdigest()
            if digest != actual:
                raise ValueError("Beta checksum mismatch")
            digests[filename] = digest
            seen.add(filename)
        if seen != expected:
            raise ValueError("Beta artifact missing")
    # Combine the independently verified fragments in memory; don't overwrite them.
    files = {filename: dist / filename for filename in digests}
    files[prefix + "-SHA256SUMS.txt"] = "".join(
        f"{digests[filename]}  {filename}\n" for filename in sorted(digests)
    ).encode("ascii")
    return files


def publish(call, files, version, sha, notes):
    tag = "beta-v" + version
    existing_tag = call("/git/ref/tags/" + tag)
    obj = existing_tag["object"]
    if obj["type"] == "tag":
        obj = call("/git/tags/" + obj["sha"])["object"]
    if obj["type"] != "commit" or obj["sha"] != sha:
        raise ValueError("Beta tag must already exist and point to the checked commit")
    release = call("/releases/tags/" + tag, missing_ok=True)
    if release and not release["draft"]:
        if not release["prerelease"]:
            raise ValueError("Existing tag belongs to a stable release")
        print(f"{tag} is already published; existing assets are preserved.")
        return release["html_url"]
    marker = f"<!-- beta-commit:{sha} -->"
    body = notes + "\n\n" + marker + "\n"
    if release:
        if marker not in (release.get("body") or "") or not release["prerelease"]:
            raise ValueError("Cannot resume a draft from another commit or release channel")
        call("/releases/" + str(release["id"]), "PATCH", {"body": body})
    else:
        # The pushed tag is mandatory. Never let the API create/move a version tag.
        release = call("/releases", "POST", {
            "tag_name": tag, "name": f"RoLauncher {version} Beta — Linux/Sober",
            "body": body, "draft": True, "prerelease": True, "make_latest": "false",
        })
    upload = release["upload_url"].split("{")[0]
    if not upload.startswith("https://uploads.github.com/repos/oangsa/RoLauncher/releases/"):
        raise ValueError("Unexpected upload host")
    path = "/releases/" + str(release["id"])
    assets = {asset["name"]: asset for asset in call(path + "/assets")}
    if set(assets) - set(files):
        raise ValueError("Draft contains unexpected assets")
    for filename, source in sorted(files.items()):
        if filename in assets:
            call("/releases/assets/" + str(assets[filename]["id"]), "DELETE")
        data = source if isinstance(source, bytes) else source.read_bytes()
        call(upload + "?name=" + filename, "POST", binary=data)
    uploaded = call(path + "/assets")
    if {asset["name"] for asset in uploaded} != set(files):
        raise ValueError("Incomplete beta upload")
    for asset in uploaded:
        source = files[asset["name"]]
        data = source if isinstance(source, bytes) else source.read_bytes()
        if asset["size"] != len(data):
            raise ValueError("Incomplete beta asset")
        if asset.get("digest") and asset["digest"] != "sha256:" + hashlib.sha256(data).hexdigest():
            raise ValueError("Uploaded beta checksum mismatch")
    result = call(path, "PATCH", {"draft": False, "prerelease": True, "make_latest": "false"})
    if result["draft"] or not result["prerelease"]:
        raise ValueError("Published release is not a beta")
    return result["html_url"]


def main():
    root = Path(__file__).resolve().parent.parent
    version = re.search(r'^version = "(\d+\.\d+\.\d+)"', (root / "Cargo.toml").read_text(), re.M)[1]
    repo = os.environ["GITHUB_REPOSITORY"]
    sha = os.environ["GITHUB_SHA"]
    if repo != "oangsa/RoLauncher" or os.environ.get("GITHUB_REF") != "refs/tags/beta-v" + version:
        raise ValueError("Publish only from the official repository's matching beta tag")
    if not re.fullmatch(r"[a-f0-9]{40}", sha):
        raise ValueError("Invalid checked commit")
    files = checked_files(root / "dist", version)
    notes = (root / f"docs/RELEASE-{version}.md").read_text(encoding="utf-8")
    if not notes.startswith(f"# RoLauncher {version}\n"):
        raise ValueError("Release notes version mismatch")
    headers = {"Authorization": "Bearer " + os.environ["GITHUB_TOKEN"],
               "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
               "User-Agent": "RoLauncher-beta-release"}
    api = f"https://api.github.com/repos/{repo}"

    def call(path, method="GET", body=None, missing_ok=False, binary=None):
        url = path if path.startswith("https://") else api + path
        data = binary if binary is not None else json.dumps(body).encode() if body is not None else None
        request_headers = dict(headers)
        if data is not None:
            request_headers["Content-Type"] = "application/octet-stream" if binary is not None else "application/json"
        try:
            with urlopen(Request(url, data=data, method=method, headers=request_headers), timeout=120) as response:
                content = response.read()
                return json.loads(content) if content else None
        except HTTPError as error:
            if error.code == 404 and missing_ok:
                return None
            raise RuntimeError(f"GitHub {method} failed (HTTP {error.code}).") from None

    stable = call("/releases/latest", missing_ok=True)
    url = publish(call, files, version, sha, notes)
    after = call("/releases/latest", missing_ok=True)
    if (stable or {}).get("id") != (after or {}).get("id"):
        raise ValueError("Stable/latest changed during beta publication; check release settings")
    print("Published " + url)


if __name__ == "__main__":
    main()
