# Step 01 — Repository Audit and Baseline

## Objective
Understand the existing Rust/egui application before changing code. Establish a reliable baseline and identify the actual causes of layout and responsiveness problems.

## Instructions
1. Inspect repository structure and read `Cargo.toml`, lockfile, README, app entry point, and relevant modules.
2. Identify exact versions of Rust, egui, eframe, and related UI crates. Do not assume APIs from newer releases.
3. Trace the application lifecycle: app initialization, state ownership, update/render loop, persistence, configuration, and error reporting.
4. Locate code responsible for:
   - Main toolbar and directory controls
   - AppImage list/cards and their actions
   - Add/Edit panel and source type selection
   - Compact mode
   - Update checks and update execution
   - File dialogs and filesystem operations
5. Identify any fixed dimensions, nested layouts, hard-coded spacing, clipping, overflow, duplicated widget code, expensive work in the UI thread, and state coupling.
6. Run baseline checks: `cargo fmt --check`, `cargo check`, and existing tests. Record failures before making changes.
7. If possible, run the app and inspect current behavior at wide, medium, narrow, and short-height window sizes. If the environment cannot launch a GUI, record that limitation and use code-level analysis.
8. Create a concise architecture map and list risks.

## Deliverables
- Architecture summary in `Progress.md`.
- Baseline check results.
- A prioritized issue list with source file/module references.
- No functional redesign or broad refactor in this step.

## Acceptance criteria
- The exact egui/eframe versions are known.
- Main UI rendering and state flow are understood.
- Relevant modules and existing tests are identified.
- Baseline failures are distinguished from regressions.
- `Progress.md` contains evidence, not guesses.
