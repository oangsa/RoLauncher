# Repository instructions

## Versioning and releases

Use MAJOR.MINOR.PATCH version numbers. Apply these rules to all future releases:

- New feature: increment MINOR and reset PATCH to zero. For example, 0.1.3 becomes 0.2.0; generally M.X.Y becomes M.(X+1).0.
- Bug fixes: increment PATCH. For example, 0.1.2 becomes 0.1.3; generally M.X.Y becomes M.X.(Y+1).
- A deliberately huge product milestone or breaking change: increment MAJOR and reset MINOR and PATCH to zero. For example, 0.4.7 becomes 1.0.0, and 1.4.7 becomes 2.0.0. N means the next major version, not an arbitrary number.
- A release containing both features and fixes uses the feature increment. A major milestone takes precedence over both.
- Bump once per release, not once per commit or individual fix. Respect an explicitly requested release version.
- Do not replace already-released artifacts with changed contents under the same version; release the change with a new version.
- Keep Cargo.toml, the root package entry in Cargo.lock, README, release notes, and artifact filenames consistent. Runtime version strings should derive from CARGO_PKG_VERSION.
- Run required checks, package the Windows executable with WebView2Loader.dll and license notices, include corresponding source and AGENTS.md, and generate SHA256 checksums.

This is the project's release convention. It is inspired by Semantic Versioning; a large milestone alone is not necessarily a breaking API change under strict SemVer.

## Roblox integration constraint

Do not read or modify Roblox process memory, inject code, patch executables or client files, or use executors. Use authenticated Roblox requests, the installed launch protocol, Windows process metadata and exit handles, and existing log files. Never expose session cookies, tickets, or authenticated launch URLs in diagnostics.
