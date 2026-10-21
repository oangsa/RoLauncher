"""Archive current source files while respecting Git exclusions."""
from pathlib import Path
import subprocess
import sys
import zipfile


def package(root, output):
    root = Path(root).resolve()
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root
    ).decode("utf-8").split("\0")
    # --exclude-standard does not exclude already tracked private files.
    ignored = subprocess.check_output(
        ["git", "ls-files", "--cached", "--ignored", "--exclude-standard", "-z"], cwd=root
    ).decode("utf-8").split("\0")
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(set(names) - set(ignored) - {""}):
            source = root / name
            if source.is_symlink() or source.resolve() != source:
                raise ValueError("Source archive cannot include external or linked files")
            if source.is_file():
                archive.write(source, name)


if __name__ == "__main__":
    package(Path(__file__).resolve().parent.parent, sys.argv[1])
