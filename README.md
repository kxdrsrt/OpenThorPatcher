# OpenThorPatcher

> _A free, simple and stable patcher for Ragnarok Online clients._

Status: **planning** (no code yet, see [docs/DESIGN.md](docs/DESIGN.md) and the roadmap in
[docs/ROADMAP.md](docs/ROADMAP.md)). MIT or Apache-2.0.

OpenThorPatcher downloads patches from a server, merges them into the client's GRF archives, manages `DATA.INI`
(including optional packs the player can switch on and off) and starts the game. It is the open successor of the
abandoned Thor Patcher: it reads the same `.thor` patches and the same `plist.txt`, so existing patch workflows keep
working, and it builds on [RustyGRF](https://github.com/kxdrsrt/RustyGRF) for everything that touches archives.

It works on its own, and it is designed to work with [OdinGuard Anti Cheat](https://github.com/kxdrsrt/OdinGuard)
(optional): it verifies that the GRFs it produced are versions the server's GRF manifest accepts, and it ships the
guard files like any other patch.

## Goals

- **Simple.** One small program, one config file, one patch list. No scripting language, no plugin system.
- **Stable.** A patch is applied completely or not at all: every GRF is written to a temporary file, verified and
  then swapped in; a backup of the previous version is kept until the next successful start.
- **Compatible.** `.thor` patches (GRF Editor, ThorGenerator, `rgrf thor create`), `plist.txt` (Thor Patcher), GRF
  0x102/0x103/0x200/0x300 including GRF Editor's encryption, through RustyGRF.
- **Safe.** HTTPS, SHA-256 for every file, a signed patch list (Ed25519), a signed self-update.
- **Player friendly.** Resumable downloads, mirrors, a progress bar that tells the truth, optional packs on/off, German
  and English first.
- **Themeable.** Operators change the look with plain HTML/CSS (like Thor Patcher's `Web/` folder).

## Not goals

No anti-cheat of its own (OdinGuard does that), no game-account system, no payment, no auto-login tricks. A launcher
mode can pass a login token that the *server operator's* web service created, nothing else.

## Documents

| | |
|---|---|
| [docs/DESIGN.md](docs/DESIGN.md) | Why a new patcher and not rpatchur, architecture, decisions |
| [docs/FORMATS.md](docs/FORMATS.md) | Patch container (`.thor`), `patchlist.json`, `plist.txt`, config, DATA.INI rules, signatures |
| [docs/ODINGUARD.md](docs/ODINGUARD.md) | How it works with OdinGuard (manifest, rollout, competitive pack switch) |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones |
