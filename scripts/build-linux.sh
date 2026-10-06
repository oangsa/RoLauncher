#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
if [[ "${1:-}" == "--use-current-version" ]]; then
    shift
else
    python3 scripts/versioning.py
fi
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
output="${1:-$root/target/linux/rolauncher-v$version}"
# Publish executes from the project directory, so resolve custom relative outputs now.
output="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve())' "$output")"
if [[ -e "$output" ]]; then
    echo 'Build directory already exists; use a new output directory.' >&2
    exit 1
fi
cargo build --locked --release --target x86_64-unknown-linux-gnu
mkdir -p "$output/desktop"
(
    cd desktop.linux
    dotnet restore RoLauncher.Linux.csproj --locked-mode -p:Configuration=Release --configfile NuGet.Config
    dotnet publish RoLauncher.Linux.csproj --no-restore -c Release -p:Version="$version" -o "$output/desktop"
)
cp "${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/RoLauncher" "$output/"
cp README.md LICENSE AGENTS.md THIRD_PARTY_NOTICES.md "$output/"
cp -R docs licenses "$output/"
cp scripts/install-linux.py "$output/"
python3 scripts/package-linux.py --directory "$output" --notices-only
printf 'Built %s\n' "$output"
