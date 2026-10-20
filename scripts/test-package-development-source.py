"""Exercise source packaging with retained private files and working-tree edits."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import zipfile

spec = importlib.util.spec_from_file_location("source_package", Path(__file__).with_name("package-development-source.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary) / "repo"
    root.mkdir()
    def git(*args):
        subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)
    git("init")
    (root / "internal.md").write_text("Private design")
    git("add", "internal.md")
    git("rm", "--cached", "internal.md")
    (root / ".gitignore").write_text("internal.md\n.env\n")
    (root / ".env").write_text("Synthetic private configuration")
    (root / "AGENTS.md").write_text("Repository instructions")
    git("add", "AGENTS.md", ".gitignore")
    (root / "AGENTS.md").write_text("Current working-tree instructions")
    (root / "new-source.rs").write_text("fn main() {}")
    output = Path(temporary) / "source.zip"
    module.package(root, output)
    with zipfile.ZipFile(output) as archive:
        assert set(archive.namelist()) == {".gitignore", "AGENTS.md", "new-source.rs"}
        assert archive.read("AGENTS.md") == b"Current working-tree instructions"
    assert (root / "internal.md").is_file()
print("Development source archive excludes private files and retains current source.")
