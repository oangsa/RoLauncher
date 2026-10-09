# RoLauncher 1.5.1

Upcoming patch; local builds use a development identity.

Fix cropped instance screenshots on high-resolution Windows displays. The capture worker measures and renders the verified Roblox client in physical pixels, including on scaled 2.8K, 4K and ultrawide displays. Dimensions are read for every capture; a frame resized during capture is skipped rather than delivered cropped.

The old 4096-pixel dimension cutoff is replaced by a 128 MiB pixel-buffer limit. Capture remains restricted to one verified client window, and encoded attachments retain the existing 8 MiB limit. No settings or account-data migration is required.

Validation: regression fixtures cover physical 2.8K and ultrawide client dimensions after resize, DPI context restoration, ownership rejection and bounded allocations. Live Roblox screenshot rendering on mixed-DPI monitors still requires device testing.
