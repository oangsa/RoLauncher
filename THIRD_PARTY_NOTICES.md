# Third-party notices

RoLauncher is distributed under GPL-3.0-only. Launch-protocol and tracker behavior, private access-code parsing, log-watcher concepts, and OS singleton coordination were referenced from Roblox Account Manager by ic3w0lf22 and contributors:

- Repository: https://github.com/ic3w0lf22/Roblox-Account-Manager
- Reference revision: c2b13d146d5db609d499870145e8d2dff2506581
- License: GNU General Public License version 3
- This is a new Rust application and is not the original Account Manager.

Rust dependencies and versions are listed in Cargo.lock. Their source packages include individual license notices. Important components include Rust/Tokio/Axum/Reqwest (MIT/Apache-2.0 ecosystem licenses), windows/windows-sys and WebView2 bindings (MIT or Apache-2.0 as specified by each package), and ring's component licenses.

WebView2Loader.dll is Microsoft's WebView2 SDK loader, redistributed from webview2-com-sys. Microsoft WebView2 Runtime is a separate prerequisite for interactive sign-in. The SDK's accompanying license is included with the release package.

GNU builds may include GCC runtime and winpthreads DLLs. These carry their upstream GPL/LGPL licenses and applicable runtime exceptions, included with the release package. Source and license references: https://gcc.gnu.org/onlinedocs/libstdc++/manual/license.html and https://www.mingw-w64.org/.

The WinUI 3 desktop uses Microsoft's Windows App SDK, Windows SDK projections/build tools and .NET runtime. Versions and package integrity hashes are pinned in desktop/packages.lock.json. The self-contained desktop includes the relevant package license files, nuspec metadata and runtime third-party notices in desktop/licenses. These components retain their respective Microsoft/MIT and other upstream terms; see the included notices.

Distribute the complete Rust and XAML/C# application source, Cargo.lock and desktop package lock files with any binary redistribution under GPL-3.0. Do not redistribute build tool archives or unrelated personal credentials.
