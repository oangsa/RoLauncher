# RoLauncher 1.6.0

Stable feature release prepared for publication after Windows/Linux checks.

- Short fade/slide navigation and sliding sidebar highlight; rapid changes interrupt motion, current-page clicks retain scroll, and reduced motion disables transitions.
- Neutral Mica workspace, layered cards, and consistent acrylic dialogs; retain System/Light/Dark, high-contrast colors, and transparency fallbacks.
- Native Windows typography; bundled Inter regular/semibold on Linux.
- Fix cropped high-DPI Windows screenshots using current physical client dimensions; skip frames resized during capture.
- Replace the 4096-pixel cutoff with a 128 MiB buffer cap; encoded attachments remain capped at 8 MiB.

No account migration. Native Linux/Sober acceptance and real Roblox GPU/mixed-monitor capture remain pending.
See [validation](VALIDATION.md).

Design references: [Mica](https://learn.microsoft.com/en-us/windows/apps/design/style/mica), [acrylic](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic), [dialogs](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/dialogs-and-flyouts/).
