# AppImage Hub

A modern desktop GUI for downloading, managing, updating, and launching AppImages on Linux. Built with [Rust](https://www.rust-lang.org/), [egui](https://github.com/emilk/egui), and [eframe](https://github.com/emilk/eframe).

## Features

- **First-run setup** — choose where AppImages should be stored, or use the default managed folder
- **Folder scanning** — import existing `.appimage` files from the selected storage folder on first use and startup
- **Add AppImages** — import a local AppImage, track a local file with a GitHub repository, or download directly from GitHub releases
- **Modern add/edit panel** — clean full-width form fields, category selector, validation, and launch-argument configuration
- **GitHub update tracking** — checks the latest release and updates AppImages in place
- **External change detection** — fingerprints each managed AppImage (size + mtime, confirmed with SHA-256 when they differ); a file replaced outside AppImage Hub is flagged as **Changed on disk** (or **Missing file**) within a second, and can be updated or accepted via **⋯ → Mark as Up to Date**
- **Run... actions** — when an update is available, choose **Launch** or **Update** from one dropdown
- **Update management** — check all updates with a smooth progress indicator, update individual apps, or update everything at once
- **Sorting** — sort the app list by **Name**, **Size**, **Date added**, **GitHub**, or **Local**
- **Desktop integration** — create start-menu entries and optional desktop shortcuts
- **Portable metadata** — each app folder keeps a `<name>.json` sidecar with its name, category, source, launch args, icon, version, and date added
- **Embedded metadata detection** — reads the AppImage's internal `.desktop` file, category, and icon
- **Safe downloads** — downloads to temporary files, makes them executable, and atomically replaces the previous AppImage
- **Modern UI** — centered modal dialogs, solid non-overlapping scrollbars, hover states, and fixed-size status badges

## Download

Prebuilt AppImages are available from the GitHub Releases page:

```text
AppImage-Hub-x86_64.AppImage
```

```bash
chmod +x AppImage-Hub-x86_64.AppImage
./AppImage-Hub-x86_64.AppImage
```

If FUSE is unavailable:

```bash
./AppImage-Hub-x86_64.AppImage --appimage-extract-and-run
```

## Build From Source

### Requirements

- Linux
- Rust toolchain with Cargo
- Network access for GitHub release checks and downloads
- Optional: `update-desktop-database`, `kbuildsycoca6` / `kbuildsycoca5`, and `gio` for desktop integration

### Run from source

```bash
cargo run
```

### Release build

```bash
cargo build --release
./target/release/appimage-hub
```

### Build the AppImage

```bash
./packaging/build-appimage.sh
```

The generated AppImage is written to:

```text
dist/AppImage-Hub-0.1.0-x86_64.AppImage
```

The build script prepares an `AppDir`, installs the desktop entry, icon, AppStream metadata, and custom AppImage Hub metadata, then packages everything with `appimagetool`.

## Usage

1. **First launch** — choose a storage folder with **Browse**, or use the default managed folder. Existing AppImages in the selected folder are scanned and imported automatically.
2. **Add a local AppImage** — choose **Local Only**, select the file, and adjust its metadata and launch arguments.
3. **Track a local AppImage for updates** — choose **Local + Repo** and provide the GitHub repository.
4. **Install from GitHub** — choose **GitHub Only** and provide the repository; the latest matching AppImage is downloaded.
5. **Manage apps** — use **Edit** to update metadata, **Add Shortcut** to create desktop/start-menu entries, or **Delete** to remove the app.
6. **Launch or update** — up-to-date apps show **Launch**; apps with updates show a **Run...** dropdown containing **Launch** and **Update**.
7. **Check updates** — use **Check All Updates** in the toolbar or Actions menu. The button fills with progress while checks run.
8. **Sort apps** — use the sort selector in the bottom status bar.

## App Metadata

AppImage Hub reads standard metadata from inside the AppImage:

- `.desktop` file: `Name`, `GenericName`, `Comment`, `Categories`, `StartupNotify`, `Icon`
- `.DirIcon` or the icon referenced by the desktop entry

Managed apps also keep a portable JSON sidecar beside the AppImage:

```text
<app-folder>/
├── my-app.appimage
├── my-app.json
└── icon.png
```

The sidecar includes fields such as:

```json
{
  "schema": 1,
  "id": "app-id",
  "display_name": "My App",
  "generic_name": "AppImage Application",
  "comment": "Short description",
  "categories": "Utility;",
  "startup_notify": true,
  "args": [],
  "source": {
    "type": "github_only",
    "github_repo": "owner/repo"
  },
  "current_version": "1.0.0",
  "asset_preference": "MyApp-x86_64.AppImage",
  "file": "my-app.appimage",
  "icon": "icon.png",
  "added_at": 1735689600
}
```

## Data Locations

- **Config:** `$XDG_CONFIG_HOME/appimage-manager/config.toml` (default: `~/.config/appimage-manager/config.toml`)
- **Default managed apps:** `$XDG_DATA_HOME/appimage-manager/apps/` (default: `~/.local/share/appimage-manager/apps/`)
- **Custom storage:** selected through the first-run setup or **Browse** in the toolbar
- **App metadata:** `<name>.json` in the same folder as the AppImage
- **Window state:** `~/.config/appimage-manager/window_state.json`
- **Desktop entries:** `$XDG_DESKTOP_DIR` and `$XDG_DATA_HOME/applications/`

## Architecture

| Module | Purpose |
| --- | --- |
| `src/main.rs` | eframe entry point and native window setup |
| `src/app.rs` | Core state (`ManagerApp`), commands/events, update checks, storage scanning |
| `src/config.rs` | Configuration persistence and app entries |
| `src/appmeta.rs` | Per-AppImage JSON sidecar read/write and schema |
| `src/appimage.rs` | AppImage metadata and icon extraction from embedded `.desktop` data |
| `src/github.rs` | GitHub API, release lookup, and architecture-aware asset selection |
| `src/downloader.rs` | Async downloads with progress and atomic replacement |
| `src/desktop_entry.rs` | `.desktop` generation, desktop/menu integration, cache refresh |
| `src/launcher.rs` | AppImage process launching |
| `src/permissions.rs` | Executable permission handling |
| `src/xdg.rs` | XDG paths, managed storage, and name sanitization |
| `src/ui/app_list.rs` | App cards, sorting, run/update actions, status badges |
| `src/ui/side_panel.rs` | Add/edit form and Add Shortcut panel |
| `src/ui/update_state.rs` | Bottom status bar, activity, progress, and sort selector |
| `src/ui/confirm_dialog.rs` | Centered delete confirmation modal |
| `src/ui/theme.rs` | Dark theme, colors, buttons, badges, and reusable UI helpers |

## Development

```bash
# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features

# Tests
cargo test
```

## Packaging Files

| File | Purpose |
| --- | --- |
| `packaging/build-appimage.sh` | Builds the final AppImage |
| `packaging/io.github.michaelx17.AppImageHub.desktop` | Desktop entry |
| `packaging/io.github.michaelx17.AppImageHub.metainfo.xml` | AppStream metadata |
| `packaging/.appimage-hub.json` | AppImage Hub's portable metadata |
| `packaging/appimage-hub.png` | Application icon |

## Notes

- The backend runs on a Tokio runtime and communicates with the UI through channels.
- GitHub asset selection prefers an AppImage matching the current CPU architecture.
- Downloads support cancellation and clean up temporary files on failure.
- Existing AppImages can be re-imported from their sidecar metadata.
