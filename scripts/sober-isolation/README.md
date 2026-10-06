# Experimental Sober isolation for RoLauncher 1.1.0 beta

Developer fixture for the old published beta only. New source builds integrate
these options into the app automatically; users do not need this adapter.

This optional launch adapter tests OS isolation without editing Sober or Roblox
binaries, configuration or memory. It has not been validated with concurrent
Roblox games on a native desktop.

1. Extract/copy this folder into your installed beta folder as `experiment-bin`.
2. Quit RoLauncher. Disable automatic rejoin for the first test.
3. In the beta folder, run:

```sh
chmod +x experiment-bin/flatpak
ROLAUNCHER_REAL_FLATPAK="$(command -v flatpak)" PATH="$PWD/experiment-bin:$PATH" ./RoLauncher
```

Start two distinct accounts. Check that both stay in a game for several minutes,
with working display/input/audio. Stop one and verify the other remains open.
Report whether the frozen-instance warning persists and whether the first client
is affected. Do not share cookies, tickets, launch URLs or raw logs.

The adapter gives each Flatpak launch private temporary/runtime directories and
SysV IPC while preserving host networking, GPU, display and audio access. Network
ports or abstract sockets can still conflict. Document-portal access is omitted
by sandbox mode; first-run setup and X11 rendering also need verification.

To stop using it, quit RoLauncher and start `./RoLauncher` normally. Existing game
clients keep running. No global Flatpak overrides or system settings are changed.
The adapter does not fix the old beta's Unknown status; newer source also improves
log ownership checks. See `docs/LINUX.md` in the source for those changes.

The adapter requires Python 3, which the Linux beta already uses. `options.json`
must stay beside `flatpak`. Both files are distributed under GPL-3.0-only.
