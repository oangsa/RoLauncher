# RoLauncher 1.6.0

Upcoming feature release; local builds use a development identity.

Navigation gains short fade/slide transitions and a sliding sidebar selection pill with an active-page marker. Rapid tab changes interrupt existing transitions, and clicking the current page preserves its scroll position. Page switching remains immediately interactive. Windows' reduced-motion setting disables these transitions.

A neutral Windows Mica base and translucent content layers bring depth to the workspace without a decorative color gradient. The sidebar uses a restrained neutral surface; menus and flyouts retain native acrylic. All account editors, preset reviews, confirmations and utility dialogs share the workspace’s title hierarchy, spacing, rounded borders and compact actions, with one acrylic surface using the native WinUI dialog template. Windows uses native WinUI typography; Linux bundles Inter with real regular and semibold faces across pages, controls and dialogs. Light, dark and system appearance remain available. High contrast uses system colors; native materials respect system transparency and platform fallback behavior.

Fix cropped instance screenshots on scaled high-resolution Windows displays by measuring and capturing the verified Roblox client in physical pixels. Dimensions are re-read for every capture, including after resizing or moving between monitors; frames resized during capture are skipped. A bounded 128 MiB pixel buffer replaces the 4096-pixel dimension cutoff, while encoded attachments retain the existing 8 MiB limit.

No account-data migration is required. This branch includes the independent capture-fix checkpoint; the patch-only branch remains available as a rollback path.

Validation and device-testing limits are recorded in [VALIDATION.md](VALIDATION.md). Live Roblox GPU capture and mixed-monitor behavior still require device testing.

Design references: [Windows Mica and content layering](https://learn.microsoft.com/en-us/windows/apps/design/style/mica), [Windows acrylic](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic), and [Windows dialogs](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/dialogs-and-flyouts/).
