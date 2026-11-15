# AppImage Hub — Project Context Card

> Agent-facing onboarding document distilled by read-only static analysis.
> Generated: 2026-11-15 · Crate: `appimage-manager` v0.1.0 (edition 2021)
> Source scope: `README.md`, `Cargo.toml`, `Cargo.lock` (versions only), `src/**/*.rs`, `assets/`.
> Excluded by design: `target/`, `.git/`, generated/vendored directories.

## 1. TL;DR

- Linux-only desktop GUI (`appimage-manager` v0.1.0) that imports/downloads AppImages, tracks GitHub releases for updates, and writes `.desktop` entries — Rust + eframe/egui 0.31.1.
- Single-viewport egui app; state lives in one `ManagerApp` struct; UI and IO are split by a background OS thread running a Tokio runtime, connected by two `tokio::mpsc` channels.
- Persistent config is TOML (`~/.config/appimage-manager/config.toml`); managed binaries under `~/.local/share/appimage-manager/apps/<name>-<id8>/`.
- Downloads stream to a sibling `.new` file, `chmod 0o755`, then `rename()` (atomic replace).
- `src/app.rs` is a 1,989-line god-module (state + UI orchestration + backend worker); all rendering lives in `src/ui/*`.
- No license declared (no `LICENSE`, no `license` field in `Cargo.toml`) — **UNKNOWN**.

## 2. Stack & Versions

| Crate | Version (lock) | Role |
|---|---|---|
| eframe | 0.31.1 | Native window/app shell (default features → **glow/OpenGL**, winit 0.30.13, accesskit) |
| egui | 0.31.1 | Immediate-mode UI |
| egui_extras | 0.31.1 (`all_loaders`) | Image loaders (`install_image_loaders`) |
| winit / glow | 0.30.13 / 0.16.0 | Windowing + GL renderer (wgpu 24.0.5 in lock but not enabled) |
| tokio | 1.54.1 (`full`) | Async runtime (backend thread only) |
| reqwest | 0.12.28 (`stream`) | HTTP downloads |
| octocrab | 0.43.0 | GitHub REST (latest release/assets) |
| rustls | 0.23.46 (`ring,std,tls12`) | Crypto provider installed once in `github::ensure_crypto_provider` |
| serde / toml | 1.0.229 / 0.8.23 | Config (de)serialization |
| async_downloader | 0.1.0 | **Declared but unused**; drags egui 0.34.3 + reqwest 0.13.5 duplicates |
| dirs / rfd | 5.0.1 / 0.15.4 | XDG dirs / native file dialogs |
| anyhow | 1.0.104 | Error handling throughout |
| thiserror / tracing / tracing-subscriber / chrono | 2.0.21 / 0.1.44 / 0.3.23 / 0.4.45 | Declared, **not referenced in source** |
| uuid | 1.29.2 (`v4`) | App IDs / temp dirs |
| futures-util | 0.3.34 | Stream extension for downloads |

## 3. Module Map

| Path (lines) | Purpose | Key symbols |
|---|---|---|
| `src/main.rs` (39) | Entry: viewport 1270×802 (min 480×360), `app_id "appimage-hub"`, icon from `assets/logo.png`, persistence `~/.config/appimage-manager/window_state.json` | `main() -> eframe::Result<()>` |
| `src/app.rs` (1989) | State + orchestration + backend worker | `ManagerApp`, `BackendCommand`, `BackendEvent`, `OpKind`, `ActiveOp`, `FailedOp`, `RetryAction`, `AppDownloadState`, `AddToState`, `setup_custom_style`, `spawn_backend`, `sanitize_asset_filename` |
| `src/appimage.rs` (444) | Extract `.desktop` metadata + icon from an AppImage (runs `--appimage-extract`, falls back to `7z`) | `AppImageMetadata`, `DesktopFileMetadata`, `extract_metadata`, `extract_icon`, `parse_desktop_entry`, `map_kde_category`, `detect_image_extension`, `icon_cache_dir`, `TempDirGuard` |
| `src/config.rs` (80) | TOML persistence model | `Config`, `AppEntry`, `AppSource` (tagged `type`; `LocalOnly`/`LocalWithRepo`/`GithubOnly`), `Config::load/save/config_path` |
| `src/desktop_entry.rs` (425) | `.desktop` generation, menu/desktop install, legacy cleanup, cache refresh, self-registration | `quote_exec_arg`, `build_exec`, `desktop_filename`, `desktop_file_content`, `write_entry`, `remove_entry`, `write_menu_entry`, `write_desktop_shortcut`, `ensure_hub_desktop_entry` |
| `src/downloader.rs` (213) | Streaming download w/ cancel + atomic swap | `temp_path_for`, `atomic_replace`, `download_to_path_with_progress`, `DownloadProgressInfo` |
| `src/github.rs` (343) | Repo normalization, asset filtering/scoring | `ensure_crypto_provider`, `normalize_repo`, `ReleaseInfo`, `GithubAssetInfo`, `is_valid_appimage_asset`, `score_asset`, `fetch_available_appimages`, `fetch_latest_release_with_selector` |
| `src/launcher.rs` (11) | Direct exec (no shell) | `launch(&Path, &[String]) -> Result<ExitStatus>` |
| `src/permissions.rs` (16) | `chmod 0o755` (no-op off Unix) | `set_executable` |
| `src/xdg.rs` (107) | XDG path resolution, managed folders, name sanitizing | `desktop_dir`, `applications_dir`, `managed_apps_dir`, `sanitize_name`, `managed_app_folder(_in)`, `is_path_in_managed_apps` |
| `src/ui/mod.rs` (7) | Module decls | — |
| `src/ui/app_list.rs` (752) | App card list (responsive), progress/failed/idle action states | `render`, `render_app_card`, `CardActions`, `CardLayout`, `card_layout_params`, `render_actions`, `render_icon` |
| `src/ui/side_panel.rs` (754) | Add/Edit form + "Add to.." panel | `SidePanelMode`, `AssetSelectionMode`, `SidePanelFormState`, `KDE_CATEGORIES`, `render`, `render_add_to`, `validation_message`, `draw_accent_rail` |
| `src/ui/theme.rs` (179) | Design tokens + style + widget helpers | `BG`, `SURFACE`, `ACCENT`, `apply_style(ctx, compact)`, `card_frame`, `primary_button`, `secondary_label`, `update_available_label` |
| `src/ui/update_state.rs` (204) | Bottom "Activity" footer (in-progress ops, failed ops, retry/dismiss) | `render`, `footer_max_height` |
| `src/ui/breakpoints.rs` (51) | Width breakpoints / size classes | `BP_LARGE/MEDIUM/NARROW/TIGHT` (1200/900/640/480), `SizeClass::from_width` |
| `src/ui/confirm_dialog.rs` (44) | Delete confirmation modal | `render` |
| `src/ui/format.rs` (85) | ETA/progress formatting | `format_eta`, `format_progress` |

## 4. Architecture & Data Flow

- **Entry point:** `src/main.rs::main` → `eframe::run_native("AppImage Hub", …)` → `ManagerApp::new(cc)`.
- `ManagerApp::new` sequence: install rustls provider → `setup_custom_style` (image loaders + `ui::theme::apply`) → `desktop_entry::ensure_hub_desktop_entry()` → `Config::load()` (on error: `eprintln!` + default) → restore `side_panel_width`/`compact_mode` from eframe storage → create channels → `spawn_backend` → `repair_missing_icons()`.
- **State ownership:** one `ManagerApp` struct owns all UI state. Config is `Arc<Mutex<Config>>`, shared with the backend thread; UI modules freely read it.
- **UI → backend:** `cmd_tx: mpsc::Sender<BackendCommand>` (cap 64, `try_send` → "System busy" status on full).
- **Backend → UI:** `evt_rx: mpsc::Receiver<BackendEvent>` (cap 256), drained at the top of every `update()` via `try_recv` loop.
- `BackendCommand`: `CheckUpdate`, `DownloadAndReplace`, `InstallFromRepo`, `FetchRepoAssets`, `Launch`.
- `BackendEvent`: `RepoAssetsFetched/Failed`, `UpdateAvailable`, `UpToDate`, `DownloadStarted`, `DownloadProgress`, `DownloadComplete`, `AppInstalled`, `DownloadCancelled`, `Error`.
- **Side effects:** all network/IO inside `spawn_backend` (OS thread → `tokio::runtime::Builder::new_multi_thread` + `block_on`); owns one `Octocrab` and one `reqwest::Client`. Cancellation via `Arc<AtomicBool>` per op. `Launch` spawns a separate `std::thread`.
- **Backend → UI events:** progress uses `evt_tx.try_send` (may drop under backpressure); completion/errors use `await`.
- Retry bookkeeping: each `ActiveOp` carries `RetryAction`; failures move to `failed_ops` (capped at 20) with retry/dismiss in the footer or per-card.
- **UI threading hints:** `ctx.request_repaint_after(200ms)` only while ops are active.
- Style is re-applied every frame: `theme::apply_style(ctx, self.compact_mode)`.
- Layout class is recomputed per frame: `SizeClass::from_width(ctx.screen_rect().width())`.
- UI never owns config writes directly — mutations lock, mutate, `cfg.save()` (sync IO on UI thread).

## 5. GUI Layout & Interaction

- Single viewport, no tabs/routes; app list is the main screen.
- Panels (in order): `TopBottomPanel::top("toolbar")` (logo + "AppImage Hub"/"AIM", Add button, `Actions` menu: download folder + `Browse` (rfd), `Check All Updates`, `Update All (n)`, `Compact mode` checkbox; status line + Dismiss) → right `SidePanel::right("editor_panel")` (resizable 320–520 px; only for `Large`/`Medium`) → `TopBottomPanel::bottom("activity_footer")` (`update_state`) → `CentralPanel` (app cards, or overlay editor with "← Back to list" on `Narrow`/`Tight`) → `egui::Window "Confirm Delete"`.
- Side panel shows either the Add/Edit form (`is_form_open`) or the "Add to.." desktop/menu toggles (`add_to`).
- Cards (`app_list`): icon, name + `(GenericName)`, comment, version badge (`• tag - Update available` / `tag - Up to date`), then three action states — `Idle` (Launch, Update, Edit, Add to.., Show folder, Delete), `Downloading` (progress bar + `%` + ETA + Cancel), `Failed` (message + Retry).
- Responsive layout entirely driven by `SizeClass` (1200/900/640/480) + `CardLayout` (`stack_actions`, `action_cols`, button sizes).
- Custom styling: dark palette with violet accent (`theme`), `CornerRadius`, `card_frame`/`panel_frame`, accent rail painted in the side panel; compact mode scales spacing to 0.82.
- Input: only `Escape` closes the editor / "Add to.." panel. No other shortcuts or menu accelerators.
- Persistence: eframe storage keys `side_panel_width`, `show_side_panel`, `compact_mode`; window geometry via `window_state.json`.
- Accessibility touches: update badges use text + color, `.on_hover_text` tooltips, `.truncate()`, disabled-hover explanations.

## 6. Public API Surface (signatures only)

```rust
// main.rs
fn main() -> eframe::Result<()>

// app.rs
pub struct ManagerApp;                     // fields are pub(crate)
impl ManagerApp {
  pub fn new(cc: &eframe::CreationContext<'_>) -> Self;
  pub(crate) fn open_add_form(&mut self);
  pub(crate) fn open_edit(&mut self, entry: &AppEntry);
  pub(crate) fn submit_side_panel_form(&mut self);
  pub(crate) fn request_delete(&mut self, app_id: String);
  pub(crate) fn confirm_delete(&mut self, app_id: &str);
  pub(crate) fn open_add_to(&mut self, app: &AppEntry);
  pub(crate) fn apply_add_to(&mut self);
  pub(crate) fn close_add_to(&mut self);
  pub(crate) fn check_all_updates(&mut self);
  pub(crate) fn update_all(&mut self);
  pub(crate) fn start_update_download(&mut self, app_id: String, url: String, tag: String, asset_name: String);
  pub(crate) fn start_install_from_repo(&mut self, github_repo: String, display_name: String, asset_name: Option<String>);
  pub(crate) fn retry_failed(&mut self, idx: usize);
  pub(crate) fn cancel_op(&self, op_id: &str);
  pub(crate) fn app_download_state(&self, app_id: &str) -> AppDownloadState<'_>;
  pub(crate) fn auto_populate_from_appimage(&mut self, appimage_path: &Path);
  pub(crate) fn repair_missing_icons(&mut self);
  pub(crate) fn updates_available_count(&self) -> usize;
  pub(crate) fn is_right_panel_open(&self) -> bool;
  pub(crate) fn send_cmd(&mut self, cmd: BackendCommand);
}
impl eframe::App for ManagerApp { fn save(..); fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame); }
pub fn setup_custom_style(ctx: &egui::Context)

// config.rs
pub struct Config { pub applications: Vec<AppEntry>, pub custom_download_dir: Option<PathBuf> }
pub enum AppSource { LocalOnly, LocalWithRepo{github_repo}, GithubOnly{github_repo} }
impl Config { pub fn load() -> Result<Self>; pub fn save(&self) -> Result<()>; pub fn config_path() -> PathBuf }

// appimage.rs
pub fn extract_metadata(p: &Path, dest_icon_dir: Option<&Path>) -> Result<AppImageMetadata>
pub fn extract_icon(p: &Path, dest_icon_dir: &Path) -> Result<PathBuf>
pub fn parse_desktop_entry(&str) -> DesktopFileMetadata
pub fn map_kde_category(&str) -> (String, bool)
pub fn detect_image_extension(&[u8]) -> &'static str
pub fn icon_cache_dir() -> Result<PathBuf>

// github.rs
pub fn ensure_crypto_provider()
pub async fn fetch_latest_release_with_selector(c: &Octocrab, repo: &str, manual: Option<&str>) -> Result<ReleaseInfo>
pub async fn fetch_available_appimages(c: &Octocrab, repo: &str) -> Result<Vec<GithubAssetInfo>>
pub fn is_valid_appimage_asset(&str) -> bool
pub fn score_asset(name: &str, target_arch: &str) -> i32

// downloader.rs
pub fn temp_path_for(target: &Path) -> PathBuf
pub fn atomic_replace(downloaded: &Path, target: &Path) -> Result<()>
pub async fn download_to_path_with_progress<F>(client: &reqwest::Client, url: &str, dest: &Path,
    cancel: Arc<AtomicBool>, on_progress: F) -> Result<()> where F: FnMut(DownloadProgressInfo)

// desktop_entry.rs
pub fn desktop_file_content(app: &AppEntry) -> String
pub fn write_entry(app: &AppEntry, desktop: &Path, apps: &Path) -> Result<()>
pub fn remove_entry(app: &AppEntry, desktop: &Path, apps: &Path) -> Result<()>
pub fn write_menu_entry / write_desktop_shortcut / remove_menu_entry / remove_desktop_shortcut (app, dir) -> Result<()>
pub fn ensure_hub_desktop_entry()

// xdg.rs
pub fn desktop_dir() / applications_dir() / managed_apps_dir() -> Result<PathBuf>
pub fn sanitize_name(&str) -> String
pub fn managed_app_folder(hint: &str, id: &str) -> Result<PathBuf>
pub fn managed_app_folder_in(base: &Path, hint: &str, id: &str) -> Result<PathBuf>
pub fn is_path_in_managed_apps(&Path) -> bool

// launcher.rs / permissions.rs
pub fn launch(appimage: &Path, args: &[String]) -> Result<ExitStatus>
pub fn set_executable(path: &Path) -> Result<()>

// ui/*
pub fn app_list::render(ui, apps: &[AppEntry], mgr: &mut ManagerApp, SizeClass)
pub fn side_panel::render(ui, mgr, SizeClass) / render_add_to(ui, mgr, SizeClass)
pub fn update_state::render(ctx, mgr, SizeClass) / confirm_dialog::render(ctx, mgr)
pub fn theme::apply(ctx) / apply_style(ctx, compact: bool)
pub fn breakpoints::SizeClass::from_width(w: f32) -> SizeClass
pub fn format::format_eta(Instant, u64, Option<u64>) -> String / format_progress(u64, Option<u64>) -> String
```

## 7. Assets

| Path | Type | Role |
|---|---|---|
| `assets/logo.png` | PNG (binary, only asset) | Window icon (`include_bytes!` in `main.rs`), toolbar brand image (`include_image!` in `app.rs`), and installed as `~/.local/share/icons/hicolor/512x512/apps/appimage-hub.png` by `ensure_hub_desktop_entry` |

No fonts, themes, shaders, or config assets are bundled.

## 8. Conventions & Invariants

- Modules are private (`mod X;` in `main.rs`); `ui` is the only submodule tree; almost all `ManagerApp` methods are `pub(crate)` — UI modules call them directly.
- Errors: `anyhow::Result` + `.with_context(|| format!(...))`; user-facing messages go into `self.status: Option<String>` and render in the toolbar, not dialogs. `eprintln!` only for config-load failure.
- Render functions follow one shape: `render(ctx|ui, mgr: &mut ManagerApp, size_class: SizeClass)`; card/panel actions are collected into a struct (`CardActions`) and applied after rendering to avoid borrow conflicts.
- Every width-dependent layout decision must go through `ui::breakpoints` (explicitly documented invariant in `breakpoints.rs`).
- Theming must go through `ui::theme` constants/helpers, never ad-hoc `Color32` in views.
- `.desktop` filenames are derived from the sanitized display name; legacy `<uuid>.desktop` files are always removed when writing/removing (dual-path cleanup).
- Download invariant: never write directly to the target — use `temp_path_for` + `atomic_replace`; the temp file must be a sibling (same filesystem).
- Filesystem side effects that fail are usually best-effort (`let _ = …`) unless the user must know (status message).
- External binaries are optional and invoked via `std::process::Command` with stdout/stderr suppressed: `xdg-user-dir`, `update-desktop-database`, `kbuildsycoca6/5`, `gio`, `7z`, `xdg-open`, `notify-send`.
- Tests are inline `#[cfg(test)] mod tests`; two tests hit the real network.

## 9. Build & Run

```bash
cargo run                       # debug (profile: debug=1, deps debug=0, incremental)
cargo run --release
cargo build --release           # ./target/release/appimage-manager
cargo test                      # includes live GitHub/network tests
cargo fmt
cargo clippy --all-targets --all-features
```

- No `[features]` section, no `rust-toolchain.toml`, no `build.rs`, no `.cargo/config.toml` detected.
- Env vars consulted: `XDG_CONFIG_HOME`, `HOME`, `XDG_DATA_HOME` (via `dirs`), `DESKTOPINTEGRATION=no` (set for AppImage extraction).
- Runtime/system notes: Linux only; needs OpenGL (glow) + winit (X11 and/or Wayland both enabled by eframe defaults); AppImage execution requires FUSE or an extraction fallback; optional `xdg-user-dir`, `update-desktop-database`, `kbuildsycoca6/5`, `gio`, `7z`, `xdg-open`, `notify-send`.

## 10. Risks & Unknowns

- `RISK` — Network-dependent unit/integration tests (`fetch_latest_release_real_repo`, `download_public_appimage_and_replace`) fail offline / in sandboxed CI.
- `RISK` — `async_downloader` 0.1.0 is declared in `Cargo.toml` but never imported; it pulls duplicate `egui 0.34.3` and `reqwest 0.13.5`, bloating builds and inviting type-confusion if someone "uses" it.
- `RISK` — `thiserror`, `tracing`, `tracing-subscriber`, `chrono` are declared but unreferenced; no logging subscriber is installed, so diagnostics are `eprintln!`/status text only.
- `RISK` — `src/app.rs` (1,989 lines) mixes UI rendering, business logic, backend worker and helpers; high merge-conflict and readability cost.
- `RISK` — `appimage::extract_metadata` executes the downloaded AppImage (`--appimage-extract`) and falls back to `7z`: untrusted-code execution surface for GitHub-installed binaries (no signature verification).
- `RISK` — `notify-send` is spawned and `rfd::FileDialog` is called from the UI thread (blocking).
- `RISK` — Progress events use `try_send`; under UI lag progress bars can jump or stall.
- `RISK` — `#[allow(dead_code)]` markers (`BackendEvent`, `ActiveOp`, `FailedOp`, `downloader::download_to_path`, `github::fetch_latest_release`, several theme/breakpoint items) hide genuinely unused API surface.
- `UNKNOWN` — License: no `LICENSE` file and no `license`/`authors`/`repository` keys in `Cargo.toml`; distribution terms unclear.
- `UNKNOWN` — Behavior on non-Linux platforms: `permissions`/`desktop_entry` degrade to no-ops, `xdg::*` would error; no cfg guards beyond `unix`, untested.

## 11. CONTEXT CARD

AppImage Hub (crate `appimage-manager`, v0.1.0, edition 2021) is a Linux-only Rust desktop application for importing, downloading, updating and launching AppImages, written with eframe/egui 0.31.1 on the default glow (OpenGL) renderer over winit 0.30. HTTP and GitHub access use reqwest 0.12 and octocrab 0.43 inside a dedicated backend thread that runs a multithreaded Tokio runtime; serde + toml persist settings; anyhow carries errors; rfd provides native file dialogs; uuid v4 generates app IDs; rustls (ring) is installed as the process-wide crypto provider once at startup. No license is declared anywhere.

The binary starts at `src/main.rs`: it builds a ViewportBuilder (1270×802, min 480×360, app id `appimage-hub`, icon from `assets/logo.png`), points eframe persistence at `~/.config/appimage-manager/window_state.json`, and constructs `ManagerApp`. That constructor initializes crypto, installs the dark theme and image loaders, registers the app's own `.desktop` entry and hicolor icon, loads `config.toml`, restores side-panel width and compact mode from eframe storage, spawns the backend, and repairs missing icons.

State is a single `ManagerApp` struct in `src/app.rs` (fields `pub(crate)`), holding an `Arc<Mutex<Config>>`, a `Sender<BackendCommand>` and `Receiver<BackendEvent>`, hash maps for active operations, available updates and latest known versions, plus form, panel and status state. Each frame `update()` re-applies the style, derives a `SizeClass` from window width, drains backend events, then renders the toolbar, the right side panel (or an overlay on narrow widths), the bottom activity footer, the app-card list and the delete confirmation window. UI mutations are plain method calls on `ManagerApp`; anything with IO, network or a thread is forwarded as a `BackendCommand` and answered asynchronously with `BackendEvent`s, with cancellation signalled by per-operation `Arc<AtomicBool>` flags. Configuration writes and desktop-entry writes happen synchronously under the config mutex. Downloads go to a sibling `.new` file, get `chmod 0755`, and are `rename`d over the target.

Module layout: `config` (Config/AppEntry/AppSource + TOML IO), `github` (repo normalization, asset filtering, architecture scoring), `downloader` (streaming with progress/cancel and atomic replace), `appimage` (executes the AppImage to extract `.desktop` metadata and icons, 7z fallback, KDE category mapping), `desktop_entry` (Freedesktop quoting, entry generation, menu/desktop install, cache refresh, self-registration), `xdg` (path resolution, managed folders, name sanitizing), `launcher`, `permissions`, and `ui` split into `app_list`, `side_panel`, `update_state`, `confirm_dialog`, `theme`, `breakpoints`, `format`.

For a typical change open, first, `src/app.rs` (state, commands, events and the backend worker), then `src/ui/app_list.rs` (per-card actions) or `src/ui/side_panel.rs` (add/edit form and validation). Changing config fields means `src/config.rs` plus the form in `side_panel.rs` and the mapping in `open_edit`/`submit_side_panel_form`. Changing update/asset selection means `src/github.rs` plus the `CheckUpdate` branch of `spawn_backend`. Changing downloads means `src/downloader.rs` plus the `DownloadAndReplace`/`InstallFromRepo` branches. Styling changes belong in `src/ui/theme.rs`; responsive rules in `src/ui/breakpoints.rs`.
