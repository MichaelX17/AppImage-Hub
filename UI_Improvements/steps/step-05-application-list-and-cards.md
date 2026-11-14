# Step 05 — Application List and Cards

## Objective
Improve readability and action hierarchy in the AppImage list.

## Instructions
1. Refactor app rows/cards using the repository's real data model.
2. Establish a predictable card hierarchy:
   - Icon and application name
   - Repository/source information (secondary)
   - Current and latest version
   - Update status (up-to-date, update available, checking, failed/unknown)
   - Primary action appropriate to the existing app behavior
   - Secondary and destructive actions
3. Ensure long names, URLs, and version strings do not force horizontal overflow. Use wrapping, truncation with accessible/full details, or responsive relocation.
4. At constrained widths, move less-used actions into a discoverable overflow menu rather than squeezing every button.
5. Keep destructive actions visually distinct and avoid accidental activation. Preserve existing confirmation semantics or add confirmation only when consistent with product behavior.
6. Keep icon dimensions and text baselines consistent. Handle missing/invalid icons gracefully.
7. Add clear empty-list and loading/error states if missing.
8. Preserve all existing actions and their underlying behavior.

## Acceptance criteria
- Cards are scannable and consistent.
- Long content does not break layout.
- Every existing action remains reachable at narrow widths.
- Status is understandable without color alone.
- Missing icons and missing version/source metadata are handled gracefully.
- Tests/checks pass and `Progress.md` is updated.
