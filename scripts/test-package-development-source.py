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
    policy = Path(__file__).resolve().parent.parent / ".gitignore"
    (root / ".gitignore").write_text(policy.read_text() + "\ninternal.md\n")
    (root / ".env").write_text("Synthetic private configuration")
    (root / "AGENTS.md").write_text("Repository instructions")
    git("add", "AGENTS.md", ".gitignore")
    (root / "tracked-private.key").write_text("Synthetic key")
    git("add", "--force", "tracked-private.key")
    with (root / ".gitignore").open("a") as stream:
        stream.write("*.key\n")
    (root / "AGENTS.md").write_text("Current working-tree instructions")
    (root / "new-source.rs").write_text("fn main() {}")
    private_paths = [
        "docs/security/WINDOWS-MACHINE-KEY-PROVISIONING-VALIDATION.md",
        "docs/security/WINDOWS-DEVICE-IDENTITY-POC.md",
        "docs/security/SECRET-SCAN-REPORT.md", "credentials.pfx", ".env.local",
        "recovery/receipt.jsonl", "lab-config-local.json", "local-baseline-test.json",
        "private-local-public-state-test.json", ".work/notes.txt", ".agents/local.txt",
        ".codex/local.txt", ".aws/credentials", ".ssh/id_test", ".gnupg/local.txt",
        "target/cache.txt", "desktop/obj/cache.txt", "data/accounts.json",
    ]
    for name in private_paths:
        private = root / name
        private.parent.mkdir(parents=True, exist_ok=True)
        private.write_text("Synthetic private fixture")
    # Force tracking to prove exclusions also cover pre-existing Git history.
    git("add", "--force", *private_paths)
    output = Path(temporary) / "source.zip"
    module.package(root, output)
    with zipfile.ZipFile(output) as archive:
        assert set(archive.namelist()) == {".gitignore", "AGENTS.md", "new-source.rs"}
        assert archive.read("AGENTS.md") == b"Current working-tree instructions"
    assert (root / "internal.md").is_file()
    for name in private_paths:
        assert (root / name).is_file(), name
    try:
        module.package(root, output)
    except FileExistsError:
        pass
    else:
        raise AssertionError("Existing source artifact was overwritten")
    linked = root / "linked-source.rs"
    try:
        linked.symlink_to(root / "AGENTS.md")
    except OSError:
        pass  # Windows may require Developer Mode; Linux always exercises this check.
    else:
        try:
            module.package(root, Path(temporary) / "linked.zip")
        except ValueError:
            pass
        else:
            raise AssertionError("Linked source was included")
print("Development source archive excludes private files and retains current source.")
