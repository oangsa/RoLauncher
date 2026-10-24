Describe the problem and resulting behavior.

Validation: list the relevant checks and any remaining limitation.

Features and fixes target `dev` from `feature/*` or `codex/*`. Main promotion targets `main` from `dev`; ordinary merges run CI only.

For an intentional release, follow [release governance](../docs/RELEASE-GOVERNANCE.md), add one new Release Intent, and update versions and notes according to AGENTS.md. The owner merging that intent authorizes automatic publication after all gates pass.
