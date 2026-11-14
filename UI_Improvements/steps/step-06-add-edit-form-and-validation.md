# Step 06 — Add/Edit Form and Validation

## Objective
Make the Add AppImage and Edit workflows clear, responsive, and robust.

## Instructions
1. Inspect the actual form model, source modes, validation rules, file picker integration, and save/update path.
2. Group fields logically:
   - Application identity: display name
   - Source type: Local Only / Local + Repo / GitHub Only (use actual supported options)
   - Local artifact: AppImage path and optional icon path
   - Repository: GitHub URL/owner-repository field when applicable
   - Launch configuration: arguments, if supported
3. Show only fields relevant to the selected source type, without losing values unexpectedly when switching modes unless current business rules require it.
4. Make labels persistent and clear; placeholders are examples, not substitutes for labels.
5. Align Browse controls with their fields and allow fields to shrink/wrap in narrow mode.
6. Validate required fields and path/URL formats using existing domain logic. Do not invent stricter validation that rejects previously valid input without documenting why.
7. Show inline validation near the relevant field and a concise overall error when save fails. Do not show a generic red message for every state.
8. Keep Cancel and Save reachable at short heights. Prevent accidental form loss when closing/canceling if the app has unsaved-change behavior.
9. Ensure save operations and filesystem/network checks do not block the UI thread. Follow the architecture identified in Step 01.
10. Test each source type, required/optional fields, invalid paths, invalid URLs, and successful save/edit flows.

## Acceptance criteria
- Form hierarchy is clear and responsive.
- Source-specific fields behave correctly.
- Validation is actionable and correctly associated with fields.
- Browse buttons and save/cancel work.
- No UI-thread blocking is introduced.
- Existing data semantics are preserved; checks pass and `Progress.md` is updated.
