# RoLauncher 1.2.1

RoLauncher 1.2.1 fixes the update banner's action button alignment.

## Fix

- Center the **Download**, **Cancel download** and **Relaunch** button vertically by balancing the banner's action spacing. The shared layout also applies to the Linux preview.

## Download and upgrade

Use **Check updates → Download → Relaunch**, or download `rolauncher-v1.2.1-setup-x64.exe`. Accounts and settings are retained and backed up during the update. The portable Windows package is `rolauncher-v1.2.1-windows-x64.zip`; the Linux preview is `rolauncher-v1.2.1-linux-x64.tar.gz`.

Packages include license notices and AGENTS.md, with corresponding source in `rolauncher-v1.2.1-source.zip` and SHA256 checksums in `rolauncher-v1.2.1-SHA256SUMS.txt`. Previously published releases are preserved. Linux remains a preview with the limitations documented in [Linux setup](https://github.com/oangsa/RoLauncher/blob/v1.2.1/docs/LINUX.md).

## Validation

The Windows UI fixture captures and measures the button's vertical center in Download, Cancel download and Relaunch states. Required Rust, desktop, installer, package and Linux checks gate publication.
