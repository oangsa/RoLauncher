"""Validate Windows/Linux release fragments and combine their checksums in memory."""
import hashlib
import re


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
                raise ValueError("Unexpected or duplicated release artifact")
            with (dist / filename).open("rb") as source:
                actual = hashlib.file_digest(source, "sha256").hexdigest()
            if digest != actual:
                raise ValueError("Release checksum mismatch")
            digests[filename] = digest
            seen.add(filename)
        if seen != expected:
            raise ValueError("Release artifact missing")
    files = {filename: dist / filename for filename in digests}
    files[prefix + "-SHA256SUMS.txt"] = "".join(
        f"{digests[filename]}  {filename}\n" for filename in sorted(digests)
    ).encode("ascii")
    return files
