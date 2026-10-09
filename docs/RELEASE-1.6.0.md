# RoLauncher 1.6.0

Upcoming feature release; local builds use a development identity.

Navigation gains short fade/slide transitions and a sliding sidebar selection pill with an active-page marker. Rapid tab changes interrupt existing transitions, and clicking the current page preserves its scroll position. Page switching remains immediately interactive. Windows' reduced-motion setting disables these transitions.

A floating acrylic sidebar, softly tinted workspace and translucent cards adopt the layered navigation and materials seen in SwiftUI, using native WinUI and Windows acrylic. Light, dark and system appearance remain available. High contrast uses solid system colors; native acrylic respects system transparency and platform fallback behavior. Tables and forms retain substantial surfaces for legibility.

Fix cropped instance screenshots on scaled high-resolution Windows displays by measuring and capturing the verified Roblox client in physical pixels. Dimensions are re-read for every capture, including after resizing or moving between monitors; frames resized during capture are skipped. A bounded 128 MiB pixel buffer replaces the 4096-pixel dimension cutoff, while encoded attachments retain the existing 8 MiB limit.

No account-data migration is required. This branch includes the independent capture-fix checkpoint; the patch-only branch remains available as a rollback path.

Validation and device-testing limits are recorded in [VALIDATION.md](VALIDATION.md). Live Roblox GPU capture and mixed-monitor behavior still require device testing.

Design references: [Apple materials guidance](https://developer.apple.com/design/human-interface-guidelines/materials), [Adopting Liquid Glass](https://developer.apple.com/documentation/TechnologyOverviews/adopting-liquid-glass), and [WinUI acrylic](https://learn.microsoft.com/en-us/windows/apps/develop/ui/in-app-acrylic).
