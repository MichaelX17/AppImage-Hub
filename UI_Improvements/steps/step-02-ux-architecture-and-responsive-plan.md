# Step 02 — UX Architecture and Responsive Plan

## Objective
Translate the reference screenshots and repository findings into a concrete layout plan that fits the existing codebase.

## Instructions
1. Review `assets/current-ui.png` and `assets/expected-result-concept.png`.
2. Treat the current screenshot as the feature inventory and the concept as visual direction only.
3. Map each existing control to its intended role. Do not drop functionality:
   - Directory/path and Browse
   - Compact
   - Check All Updates / Update All
   - Add
   - App identity, repository, current/latest version
   - Launch, Edit, Delete, Show in Folder, Add to (and any other existing actions)
   - Add/Edit fields, source modes, validation, Cancel, Save
4. Define information hierarchy: global navigation/actions, app identity/status, primary action, secondary actions, destructive action, form groups, and feedback.
5. Define responsive modes based on actual content constraints:
   - Wide: side-by-side list and form panel when viable.
   - Medium: narrower gutters, wrapped toolbar, compact action groups.
   - Narrow: single-column/panel navigation or overlay; preserve access to all controls.
   - Short height: scrolling and reachable actions.
6. Decide which actions remain directly visible and which can move into a clearly labeled overflow menu at constrained sizes. Do not hide critical actions without an accessible route.
7. Define a spacing, typography, border, radius, and color token proposal. Prefer existing theme infrastructure if present.
8. Write down interaction states: idle, hover, pressed, focused, disabled, loading, success, warning, error, empty.
9. Record implementation plan and any tradeoffs in `Progress.md`.

## Deliverables
A responsive behavior specification and component map in `Progress.md` (or a small design note if useful).

## Acceptance criteria
- Every existing feature/control is accounted for.
- Layout decisions are tied to available size and content minimums.
- Narrow and short-height behavior is explicitly defined.
- Visual direction is consistent with the concept without treating generated text/details as product requirements.
