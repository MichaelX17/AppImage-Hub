# Step 07 — Responsiveness, Performance, and Interaction QA

## Objective
Validate the complete experience and address regressions, performance issues, and interaction gaps.

## Instructions
1. Run the app where possible and test these viewport categories:
   - Wide desktop window
   - Medium window
   - Narrow window
   - Short-height window
   - Resize continuously between categories
2. Test both Compact enabled and disabled.
3. Test a long app name, long repository URL, missing icon, missing version, many app entries, and empty list.
4. Test every toolbar action, every app-card action, every source mode, Browse, Save, Cancel, and validation/error states.
5. Inspect for:
   - Clipped controls/text
   - Horizontal overflow
   - Unintended nested scrolling
   - Jitter/flicker during resize
   - State resets when changing responsive modes
   - Low-contrast text and invisible focus
   - Inconsistent spacing or control heights
6. Review UI update/render paths for expensive filesystem/network work, repeated allocations, unnecessary cloning, and excessive repaint requests.
7. If background work is needed, use safe message passing and ensure progress/results are surfaced in the UI. Do not introduce concurrency complexity without need.
8. Add or update tests for pure layout decisions, validation, and state transitions where feasible.
9. Record test environment limitations honestly.

## Acceptance criteria
- All viewport categories pass manual/code-level review.
- All existing workflows remain functional.
- No serious clipping, overflow, or state-loss bugs remain.
- No new avoidable blocking operation exists in the UI path.
- Any known limitations are documented in `Progress.md`.
