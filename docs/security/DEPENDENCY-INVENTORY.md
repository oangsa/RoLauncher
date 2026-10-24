# Dependency inventory — PR02

Evidence date: October 16, 2026. Base: `000c175fafa8ff6e763dd28bf80509cbe702eab2` (`dev`).
Application version and dependency locks are unchanged.

## Reproduce

1. Restore with the existing locked Windows/Linux build instructions; do not regenerate locks.
2. Install scanners with `python scripts/security-tools.py --directory .tools/security`.
3. Run the scanner commands in [the security workflow](../../.github/workflows/security.yml).
4. Generate the [machine inventory](dependency-inventory.json):

```powershell
. ./scripts/environment.ps1
python scripts/security.py audit --kind nuget --project desktop/RoLauncher.Desktop.csproj > target/nuget-windows.json
# Run the Linux audit with its native SDK and retain its sanitized output as target/nuget-linux.json.
python scripts/security-inventory.py --nuget-cache .tools/nuget --nuget-cache .tools/linux-nuget --rust-audit docs/security/vulnerability-baseline.json --nuget-audit desktop=target/nuget-windows.json --nuget-audit desktop.linux=target/nuget-linux.json --output docs/security/dependency-inventory.json
```

Use actual restored cache paths on another machine. Metadata collection is locked/offline and never builds or upgrades dependencies.
The JSON includes input hashes, resolved versions, direct/transitive status, declared licenses, notice hashes, dependency kinds and target exposure.

## Ecosystems

| Input | Inventory and exposure |
|---|---|
| `Cargo.toml`, `Cargo.lock` | 312 package records including RoLauncher; all lockfile targets, Windows GNU/MSVC and Linux x64 graphs |
| Rust normal/build/dev dependencies | `dependencies` records kind and target; runtime candidates exclude build edges and procedural macros; `tower` is a direct test dependency |
| Windows-specific Rust | `windows`, `windows-sys`, `wmi`, `webview2-com`; `winresource` is a build dependency |
| Linux-specific Rust | `libc`, `keyring`, `chacha20poly1305`; Secret Service/DBus and native dependencies remain platform prerequisites |
| `desktop/*.csproj`, lock | Windows App SDK and SDK build tools are direct; WebView2 and App SDK subpackages are transitive |
| `desktop.linux/*.csproj`, lock | Uno/Skia/.NET 10 graph; 96 NuGet target records across both desktop locks, 85 unique package/version pairs |
| `desktop.tests` | No PackageReference; locked `net8.0` package graph is empty; SDK/runtime remains a separate component |
| NuGet configuration | Both desktop configurations clear inherited package sources and use nuget.org; normal restore explicitly disables NuGetAudit |
| Uno SDK | `desktop.linux/global.json`: Uno.Sdk 6.7.30 (Apache-2.0); SDK 10.0.100 with `latestFeature` roll-forward, outside the package lock |

`runtime-candidate` means graph/asset reachability, not proof that every DLL ships or that vulnerable code executes.
NuGet asset evidence comes from restored `project.assets.json`; stale/different configuration assets require regeneration.
Use [vulnerability baseline](VULNERABILITY-BASELINE.md) and [license register](PROVENANCE-AND-LICENSE-REGISTER.md) for dispositions.

## Components outside dependency locks

| Component | Version/evidence | Distribution role and remaining evidence |
|---|---|---|
| WebView2 binding/loader | webview2-com/sys 0.39.1; NuGet Microsoft.Web.WebView2 1.0.3179.45 | Loader is bundled; runtime is an independently installed prerequisite; SDK notice is included |
| Windows App SDK | 1.8.260921001 and exact subpackage versions in JSON | Self-contained desktop; Microsoft terms require explicit redistribution review |
| .NET | Windows local SDK 8.0.425/runtime 8.0.31; Linux SDK 10.0.401/runtime 10.0.12; test runtime 8.0.31 | Self-contained published runtime packs are not covered completely by `dotnet list package`; record each actual release payload |
| Inno Setup | Local downloaded 6.7.3; build accepts installed 6.7+ or 7 | Build tool and installer engine; license notice exists; compiler selection is not content-pinned |
| GCC/MinGW | Optional `.tools/gcc/mingw64`; host/toolchain dependent | No runtime DLL copies found in current build script; inspect static linkage and actual imports per payload before deciding obligations |
| Linux native stack | DBus, Secret Service, GTK/WebKit, AppIndicator, X11/GL/fontconfig, portals | OS-provided, version dependent; native assets from NuGet may be bundled; no blanket redistribution clearance |
| Flatpak/Sober | Installed externally; documented Flatpak minimum 1.15.6 | Adapter/tooling only; Sober is not bundled; platform permissions are a separate assessment |
| Inter | Official 4.1 static TTFs and repository OFL notice | Linux bundled font; verify upstream archive/file hashes for release provenance |
| Actions, apt/pacman, rustup, .NET setup | Existing workflows/build scripts | Mutable tags, OS image/package repositories and bootstrap downloads are separate supply-chain inputs |

No application dependency upgrades were made. A package/license inventory is not an SBOM completeness or reproducible-build certification.

## Tool selection

- cargo-audit 0.22.2: RustSec advisories, full locked graph, source CVSS vectors and patches; selected for vulnerability evidence.
- cargo-deny 0.20.2: SPDX-expression evaluation, missing licenses and source restrictions; selected for Rust license/source evidence.
- Both declare Rust 1.88 minimum; repo Rust 1.99 is compatible; official prebuilt binaries also ran locally.
- Gitleaks 8.30.1: free MIT CLI, default rules plus Roblox rules; upstream states future maintenance is security patches only.
- Native NuGet `dotnet list package --vulnerable --include-transitive --format json`: selected instead of another service/account.

Tool archives and Action revisions are pinned; advisory feeds stay live and their revision/date is evidence, not an immutable promise.
See [RustSec](https://github.com/rustsec/rustsec/tree/cargo-audit/v0.22.2/cargo-audit), [cargo-deny](https://github.com/EmbarkStudios/cargo-deny/tree/0.20.2), [Gitleaks](https://github.com/gitleaks/gitleaks/tree/v8.30.1) and [NuGet audit documentation](https://learn.microsoft.com/en-us/nuget/concepts/auditing-packages).
