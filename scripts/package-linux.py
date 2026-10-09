"""Package Linux x64 with dependency notices and a SHA256 checksum; never overwrite."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parent.parent


def notices(directory):
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
        "--filter-platform", "x86_64-unknown-linux-gnu"], cwd=root))
    licenses = directory / "licenses"
    licenses.mkdir(exist_ok=True)
    shutil.copy2(root / "licenses/inter-OFL.txt", licenses)
    for package in metadata["packages"]:
        source = Path(package["manifest_path"]).parent
        destination = licenses / f'{package["name"]}-{package["version"]}'
        for notice in source.iterdir():
            if notice.is_file() and notice.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                destination.mkdir(exist_ok=True)
                shutil.copy2(notice, destination)
    (licenses / "dependencies.json").write_text(json.dumps([
        {key: p.get(key) for key in ("name", "version", "license", "repository")}
        for p in metadata["packages"]], indent=2))
    assets = json.loads((root / "desktop.linux/obj/project.assets.json").read_text())
    desktop_licenses = directory / "desktop/licenses"
    desktop_licenses.mkdir(exist_ok=True)
    records = []
    for name, library in assets["libraries"].items():
        if library["type"] != "package":
            continue
        package = next((Path(folder) / library["path"] for folder in assets["packageFolders"]
                        if (Path(folder) / library["path"]).is_dir()), None)
        if package is None:
            raise RuntimeError("Missing NuGet package for license collection")
        spec = ET.parse(next(package.glob("*.nuspec"))).getroot()
        ns = {"n": spec.tag.split("}")[0][1:]} if "}" in spec.tag else {}
        prefix = "n:" if ns else ""
        license = spec.find(f"{prefix}metadata/{prefix}license", ns)
        records.append({"package": name, "license": license.text if license is not None else None})
        destination = desktop_licenses / name.replace("/", "-")
        files = [p for p in package.rglob("*") if p.is_file()
                 and p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "THIRDPARTY"))]
        if license is not None and license.get("type") == "file":
            files.append(package / license.text)
        for notice in set(files):
            destination.mkdir(exist_ok=True)
            # Preserve subpaths where packages have more than one license with the same name.
            target = destination / notice.relative_to(package)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(notice, target)
    (desktop_licenses / "dependencies.json").write_text(json.dumps(records, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=root / "dist")
    parser.add_argument("--notices-only", action="store_true")
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    name = f"rolauncher-v{version}"
    for file in ("RoLauncher", "desktop/RoLauncher.Desktop", "AGENTS.md", "LICENSE"):
        if not (directory / file).is_file():
            raise RuntimeError(f"Missing Linux package file: {file}")
    notices(directory)
    if args.notices_only:
        return
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output / f"{name}-linux-x64.tar.gz"
    sums = args.output / f"{name}-linux-SHA256SUMS.txt"
    if archive.exists() or sums.exists():
        raise FileExistsError("Release artifacts already exist; bump the version")
    with archive.open("xb") as output, tarfile.open(fileobj=output, mode="w:gz") as package:
        package.add(directory, arcname=name)
    with archive.open("rb") as source:
        checksum = hashlib.file_digest(source, "sha256").hexdigest()
    sums.write_text(f"{checksum}  {archive.name}\n", encoding="ascii")
    print(f"Packaged {archive.name} with licenses and SHA256")


if __name__ == "__main__":
    main()
