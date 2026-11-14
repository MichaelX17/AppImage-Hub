# Step 03 — Design System and Reusable Components

## Objective
Establish consistent visual primitives before rebuilding major screens.

## Instructions
1. Inspect current style/theme code and reuse it where appropriate.
2. Define centralized tokens for:
   - Background and surface levels
   - Primary accent and semantic success/warning/error colors
   - Main, secondary, and muted text
   - Border and focus colors
   - Spacing scale
   - Corner radii and control heights
   - Typography sizes/weights
3. Keep contrast adequate. Do not communicate status by color alone; pair color with text/icon/shape.
4. Create or refine reusable helpers/components for repeated patterns, such as labeled fields, status labels, section headings, and consistent action buttons, while respecting the egui version.
5. Use consistent button hierarchy:
   - Primary: main commit action (e.g. Save, Launch where appropriate)
   - Secondary: Edit, Browse, Show in Folder
   - Destructive: Delete, with clear affordance and appropriate confirmation if the existing product behavior requires it
6. Ensure keyboard focus/interaction states are visible where supported by the existing toolkit.
7. Avoid excessive decoration and avoid introducing a large dependency for simple styling.

## Constraints
- Keep this step focused on reusable style primitives.
- Do not duplicate style constants across modules.
- Do not break current behavior or introduce a new visual framework.

## Acceptance criteria
- Theme values are centralized or consistently reused.
- Repeated controls have consistent sizing and hierarchy.
- Focus/disabled/error states are legible.
- `cargo check` and relevant tests pass.
- Record design decisions and changed files in `Progress.md`.
