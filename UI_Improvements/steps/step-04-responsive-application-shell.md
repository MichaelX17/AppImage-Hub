# Step 04 — Responsive Application Shell

## Objective
Make the top-level window layout adapt to available width and height.

## Instructions
1. Locate the root egui layout and determine how available content size is obtained in the project's egui version.
2. Replace brittle fixed-width assumptions for major regions with adaptive layout logic.
3. Implement the responsive modes from Step 02:
   - Wide: toolbar and main content use available space; list and editor may sit side-by-side.
   - Medium: controls wrap or compact without overlapping.
   - Narrow: switch to a single-column or explicit panel-navigation pattern. Ensure the user can reach both app list and Add/Edit form.
   - Short height: use scroll areas thoughtfully; keep important actions reachable.
4. Keep the toolbar from overflowing. Group related controls and use compact labels/overflow only where necessary and discoverable.
5. Make Compact mode a deliberate density setting, not a substitute for responsive layout. Verify it works at every width.
6. Prevent nested scroll areas from fighting each other. Choose clear scroll ownership for the app list and form.
7. Handle window resizing continuously without layout flicker or state loss.
8. Preserve keyboard and pointer usability.

## Acceptance criteria
- No overlap or clipping in the root layout at tested sizes.
- All toolbar actions remain accessible.
- Add/Edit panel remains reachable in narrow mode.
- Short-height windows can reach bottom actions.
- Compact mode does not cause controls to disappear.
- `cargo check` and relevant tests pass; update `Progress.md`.
