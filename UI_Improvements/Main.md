# AppImage Hub — Main Development Prompt

## Mission

You are a senior Rust engineer specializing in `egui`/`eframe` desktop applications and a professional GUI/UX designer.

Improve the existing AppImage Hub application in this repository. The goals are:

1. Improve responsiveness across small, medium, and large window sizes.
2. Improve visual hierarchy, spacing, typography, alignment, and consistency.
3. Preserve existing functionality and user data.
4. Improve interaction feedback, validation, and accessibility.
5. Keep the implementation idiomatic, maintainable Rust and idiomatic egui.

**Do not rewrite the application from scratch.** First inspect the actual repository, understand its architecture, and make incremental changes that fit the existing codebase.

## How to use this prompt pack

1. Read this file completely.
2. Read `Progress.md`.
3. Execute the files in `steps/` in numeric order.
4. Before each step, inspect the relevant source code and tests.
5. Implement the step, run formatting/checks/tests, and review the result.
6. Update `Progress.md` after each step. Record files changed, decisions, checks run, outcomes, and remaining issues.
7. Do not mark a step complete unless its acceptance criteria are met.
8. If repository facts contradict an assumption in these prompts, trust the repository, document the discrepancy, and adapt without breaking existing behavior.

## Reference images

- `assets/current-ui.png`: screenshot of the existing application. Treat it as evidence of the current visual structure and feature set.
- `assets/expected-result-concept.png`: visual direction, not a pixel-perfect specification. Do not copy any inaccurate text or invent functionality shown in the concept.

The concept is aspirational. Actual behavior and source code are authoritative.

## Current interface observations

The screenshot shows:
- A top toolbar with the AppImage directory, Compact toggle, Browse, Check All Updates, Update All, and Add.
- A main application list with app icon, display name, repository URL, current/latest versions, and actions such as Delete, Edit, Show in Folder, Add to, and Launch.
- A right-side Add AppImage panel with display name, source type, AppImage path, optional icon path, GitHub URL, launch arguments, Cancel, and Save.
- A dark theme with purple accent color, bordered panels, and red destructive actions.

Preserve these concepts unless repository inspection establishes a better existing equivalent.

## Non-negotiable engineering rules

- Inspect `Cargo.toml`, the entry point, module structure, state model, persistence layer, and current egui version before coding.
- Do not assume a particular egui version or API. Use APIs supported by the version already in the project unless an upgrade is justified and separately explained.
- Avoid unnecessary dependencies. Explain and justify any new dependency.
- Do not use blocking filesystem, network, or update operations on the UI thread. If the existing app does so, assess and improve it safely using the project's established async/threading model.
- Never access egui UI state from worker threads. Send results back to the UI thread using a safe message/channel pattern compatible with the current architecture.
- Avoid `unwrap()`/`expect()` in user-facing runtime paths. Handle errors and show actionable feedback.
- Preserve configuration compatibility and existing data. If a migration is required, make it backward-compatible and test it.
- Keep business logic out of rendering code where practical.
- Avoid arbitrary fixed pixel widths/heights for major layout regions. Use available size, constraints, wrapping, and adaptive layout decisions.
- Do not silently remove controls or features to make the layout simpler.
- Do not claim a feature works unless it has been implemented and tested.

## UX and visual direction

Use the concept image as a direction:
- Dark neutral surfaces with restrained purple accent.
- Consistent spacing scale and alignment.
- Clear distinction between primary, secondary, and destructive actions.
- App cards that remain readable when space is constrained.
- Form fields grouped logically with labels and helpful validation.
- Status states that are understandable without relying on color alone.
- Consistent corner radii, borders, button heights, and icon alignment.
- Avoid excessive gradients, glow, decoration, and visual noise.
- Ensure text contrast and readable secondary text.

## Responsive behavior requirements

Implement and verify at least these states, using actual available viewport size rather than guessed device labels:

1. **Wide:** list and add/edit panel may be displayed side-by-side when there is enough room.
2. **Medium:** reduce spacing and allow secondary actions to wrap or move into a compact/overflow treatment. Keep the most important actions visible.
3. **Narrow:** use a single-column layout or a clear panel navigation/overlay pattern. No clipped form fields, inaccessible buttons, or horizontal overflow.
4. **Short height:** allow content regions to scroll independently or use an appropriate scroll strategy; keep essential navigation/actions reachable.
5. **Compact mode:** preserve its intended meaning and make it work consistently at every size.

Define breakpoints based on observed content minimums and test them. Do not invent a breakpoint before measuring the layout's needs.

## Definition of done

- Existing features continue to work.
- No major panel, field, or action is clipped at supported window sizes.
- The interface adapts to wide, medium, narrow, and short windows.
- The app list and add/edit form have clear visual hierarchy.
- Loading, success, empty, validation, and error states are understandable.
- No avoidable blocking work occurs in the UI update/render path.
- `cargo fmt --check`, `cargo check`, and relevant tests pass (or failures are clearly documented as pre-existing/environmental).
- Progress and decisions are recorded in `Progress.md`.
- Final response includes a concise summary, changed files, checks run, known limitations, and any follow-up work.

## Execution protocol

Work one step at a time. Do not skip ahead. At the beginning of each step, state the objective and the relevant repository facts. At the end, report:
- What changed
- Why
- Files touched
- Tests/checks run and their results
- Any risks or unresolved items
- Progress.md updates

Start with `steps/step-01-repository-audit.md`.
