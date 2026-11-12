# AppImage Hub (AppImage Manager)

A desktop GUI for downloading, managing, updating, and launching AppImages on Linux. Built with [Rust](https://www.rust-lang.org/), [egui](https://github.com/emilk/egui), and [eframe](https://github.com/emilk/egui/tree/master/crates/eframe).

## Features

- **Add AppImages**: Import local `.AppImage` files, link to GitHub repos for updates, or download directly from a GitHub repo's latest release
- **Auto-detect latest releases**: When linked to GitHub, checks for updates against the latest release and can download/update in-place
- **Organized storage**: Apps are copied/moved into a managed directory (`~/.local/share/appimage-manager/apps/` or a custom folder)
- **Desktop integration**: Creates `.desktop` entries in `~/.local/share/applications/` (start menu) and optionally on the Desktop; refreshes menu caches where available
- **Update management**: "Check All Updates", per-app update actions, and "Update All" with progress tracking and retry on failure
- **Launch**: Launch AppImages directly from the UI with optional custom launch arguments
- **Edit & organize**: Edit display name, source/Repo linkage, icon, launch args, and manage desktop/menu entries
- **Safe updates**: Downloads to `.new` temp files, makes executable (`chmod +x`), atomically replaces the old AppImage, and updates desktop entries

## Architecture

| Module | Purpose |
| --- | --- |
| `src/main.rs` | eframe entry point, sets up native options/persistence |
| `src/app.rs` | Core state (`ManagerApp`), UI actions, backend command/event handling |
| `src/config.rs` | Config persistence (`~/.config/appimage-manager/config.toml`) for apps and custom download dir |
| `src/github.rs` | GitHub API via `octocrab`, repo normalization, latest release + asset selection (prefers AppImage for current arch) |
| `src/downloader.rs` | Async download with progress, temp-file + atomic replace logic |
| `src/permissions.rs` | Make files executable on Unix |
| `src/xdg.rs` | XDG directory resolution, managed app paths, name sanitization |
| `src/desktop_entry.rs` | `.desktop` file generation, quoting, menu/desktop integration, cache refresh |
| `src/launcher.rs` | Launch AppImages with arguments |
| `src/ui/` | egui UI: app list, side panel (add/edit), update state, confirm dialog, theme |

## Requirements

- Linux (primary target; uses XDG paths and `.desktop` integration)
- Rust toolchain (see `Cargo.toml`; uses 2021 edition)
- Network access for GitHub release lookups/downloads
- `update-desktop-database`, `kbuildsycoca6`/`kbuildsycoca5` optional for menu cache refresh
- `gio` optional for marking desktop entries as trusted

## Installation / Running from source

```bash
# Clone
git clone <repo-url>
cd AppImage-Hub

# Build and run (debug)
cargo run

# Build release
cargo run --release
# or
cargo build --release
./target/release/appimage-manager
```

## Usage

1. **Add a local AppImage**: Click `Add` → choose `Local Only`, select the `.AppImage`, set display name/args/icon as desired. The app is copied/moved into the managed folder and made executable; a menu entry is created.
2. **Add from GitHub (download latest)**: Click `Add` → `From GitHub (download)`, paste repo URL (e.g. `owner/repo` or full GitHub URL), set name. Latest release AppImage is downloaded to managed folder.
3. **Track a local AppImage for updates**: `Add` → `Local + GitHub`, point to local file and provide GitHub repo URL. App will be tracked for updates.
4. **Manage entries**: Use `Edit` on an app to change metadata/source, or `Add to..` to toggle Desktop/Menu entries. Use `Delete` (with confirmation) to remove app and clean up managed files/entries.
5. **Updates**: `Check All Updates` scans tracked apps; `Update` on individual apps or `Update All` downloads and replaces in-place.

## Data locations

- **Config**: `$XDG_CONFIG_HOME/appimage-manager/config.toml` (defaults to `~/.config/appimage-manager/config.toml`)
- **Managed apps**: `$XDG_DATA_HOME/appimage-manager/apps/` (defaults to `~/.local/share/appimage-manager/apps/`) unless a custom folder is chosen via `Browse`
- **Window state**: persisted by eframe via `~/.config/appimage-manager/window_state.json` (as configured)
- **Desktop entries**: `$XDG_DESKTOP_DIR` and `~/.local/share/applications/`

## Development

```bash
# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features

# Tests
cargo test
```

### Notes
- The app uses a background Tokio runtime + channel to perform GitHub/API/download operations off the UI thread
- Asset selection prefers an AppImage matching the current CPU architecture; falls back to any AppImage if none match
- Downloads use streaming with progress; cancelled downloads clean up temp files
- On replacement, if filename changes, the old file is removed; desktop entries are regenerated on update/install/edit
