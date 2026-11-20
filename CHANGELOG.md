# Changelog

All notable changes to AppImage Hub.

## 0.2.1 — 2026-11-20

Desktop-integration patch release: correct task-manager icon grouping for
pinned AppImages and broader metadata extraction.

### Fixed

- Generated `.desktop` shortcuts now write `StartupWMClass`, so launching a
  pinned app no longer opens a second task-manager icon. The exact value is
  read from the AppImage's embedded desktop entry, `Exec=` program name, or
  `AppRun`, can be edited in the add/edit form, and is backfilled for existing
  apps on startup. Installed updates re-read the class from the new build
  unless the user overrode it.
- Metadata extraction now handles desktop entries stored as root symlinks and
  AppImages whose SquashFS payload is hidden behind an embedded compressed
  stream (e.g. DeepSeek Harness, Zen Browser), without executing the AppImage.

## 0.2.0 — 2026-11-19

Production-hardening release. No UI redesign; behavior is otherwise unchanged.

### Security

- AppImages downloaded from GitHub are no longer executed to read metadata.
  Non-executing `7z` extraction is used; local files the user explicitly picks
  may still run once.
- `.desktop` generation sanitizes all values centrally; custom categories can
  no longer inject keys, icon paths with control characters are dropped, and
  `%` is escaped in `Exec` arguments.
- GitHub token storage: OS keyring with an Argon2id-encrypted `0600` fallback;
  the token is never written beside a portable AppImage. Legacy `AIHT1` files
  migrate on load. Deleting the token removes both keyring entry and file.
- Downloads are bounded (2 GiB), must match `Content-Length`, are fsynced, and
  must carry an ELF header. Published `<asset>.sha256` files are verified.

### Fixed

- Config and sidecar writes are atomic; a corrupt `config.toml` is quarantined
  (preserved) and reported instead of being overwritten with defaults.
- Sidecars from a newer build are never downgraded or overwritten.
- Deleting an app removes its binary/sidecar/icon/folder for both default and
  custom storage roots, and preserves unrelated files.
- Changing the storage folder keeps existing entries in place and warns.
- Launch arguments survive add → edit → launch with shell-like quoting.
- Backend commands no longer block each other: downloads run as independent
  tasks (max 3 at once) with timeouts; Launch is direct from the UI.
- Update checks always terminate; deleting an app mid-check clears its pending
  state and the progress UI recovers.

### Internal

- `app.rs` split into `app/{actions,events,snapshot,render,toolbar,first_run,
  update_check,util}.rs`; backend split into `backend/{download,install,
  update_check,self_update}.rs`; one shared download-finalization pipeline.
- UI caches the app snapshot and file issues; no per-frame full-state clones,
  config locks, or filesystem scans.
- Added CI (fmt, clippy, tests, deny, release build), `deny.toml`, integration
  tests, MIT/Apache-2.0 licensing, and one version (`0.2.0`) everywhere.
