# Roadmap

Like RustyGRF: plan first, then milestones; each milestone ends with a working, tested step.

| | Milestone | Result |
|---|---|---|
| M0 | Plan (this folder) | design, formats, OdinGuard integration reviewed by the owner |
| M1 ✅ | Core library `otp-core` | patch list (JSON + legacy plist), config, state file, download with resume + SHA-256 + Ed25519, unit tests |
| M2 ✅ (plain + encrypted tested on synthetic GRFs; the real 2.5 GB `data.grf` still to run) | Apply engine | Thor patch merge into GRFs through `rgrf-core`, temp file + verify + atomic swap + backup, DATA.INI writer; tested on the real 2.5 GB `data.grf` and encrypted GRFs |
| M3 ✅ | CLI `otp` | `otp keygen`, `otp publish` (master client → site: diff, auto-numbered `.thor`, signed list), `otp patch` (`--dry-run`); `tests/e2e.sh` |
| M4 | UI | Tauri window with themeable HTML/CSS, progress, optional packs, language; `--no-ui` mode; Windows 7 decision |
| M5 | OdinGuard integration | manifest check after patching, rollout helper in the Studio, pack switch |
| M6 | Self-update + release | signed self-update, installers, docs, screenshots |
