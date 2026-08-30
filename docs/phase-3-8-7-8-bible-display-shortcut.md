# Phase 3.8.7.8 — Bible finding "Display" shortcut in Needs Attention

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `f410eb4` (Phase 3.8.7.7, overload/segmenter fix)

## Why this phase exists

The operator's real Windows screenshots comparing Live Service and Service
Replay confirmed the downstream Bible detection/persistence/presentation
pipeline genuinely works end to end on real hardware: Service Replay
detected MAT 6:9 and displayed it correctly on the presentation window.
The operator's own follow-up request, on the same session: "At the needs
attention when CIP detect Bible reference we have approve and reject can
include display there so that the operator will not need to scroll down
for 2 seconds before Bible can be displayed. Or replace approve with
display."

## Audit before implementing

Traced the real existing flow before changing anything:
`AttentionQueue.tsx`'s Bible card called `approve_suggestion` on Approve;
the operator then had to scroll to the separate Presentation card, click
Prepare (`prepare_presentation`), then click Display
(`display_presentation`) - three clicks across two UI regions for a
reference the operator already wants on screen. Confirmed
`display_presentation` (`commands.rs:2765-2805`) already opens the
display window itself if needed (`open_display_window`) and already
handles the Prepared -> Active transition
(`presentation::prepare_to_activate`/`commit_activation`) - both
pre-existing, already-tested behaviors this phase reuses unchanged, not
reimplements.

## Fix applied

Replaced "Approve" with "Display" for the Bible domain only
(`actionsFor("bible")` now returns `["display", "reject"]` instead of
`["approve", "reject"]` - the operator's own stated preference over
adding a third button). No new Tauri command, event, or persistence: the
new `"display"` action chains the three pre-existing commands
(`approveSuggestion`, `preparePresentation`, `displayPresentation`) inside
one `withBusy`-wrapped async handler in `LiveChurchBrain.tsx`'s
`handleUnifiedAction`, using `preparePresentation`'s own returned
`PresentationItem.id` to call `displayPresentation` - never relying on
React state having caught up via events in between.

Scope confirmed narrow: only the Bible domain's Needs Attention actions
changed. Music/Sermon/Service/Content/Correlation actions are untouched.
The separate diagnostics-mode "Pending Suggestions" panel (which calls
`approveSuggestion` directly, not through `actionsFor`) is untouched - it
was not part of the operator's request.

## Full regression result

Frontend: `tsc -b` clean (0 errors), `oxlint` clean (0 errors, same 4
pre-existing warnings), `vitest` 210/210 passed (unchanged count - one
existing assertion in `actions.test.ts` updated from `['approve',
'reject']` to `['display', 'reject']` for the bible domain), `vite build`
clean. Zero Rust files changed this phase - no Rust regression re-run was
needed.

## Windows artifact

- SHA-256: `5587f4ec47a80a2062995e35fc48b222fae27f5dd96722d98dea562fa3910634`
- Size: 8,587,913 bytes (up slightly from 8,587,492 - expected for a
  small frontend bundle change)
- Direct proof: `x86_64-w64-mingw32-strings` against the extracted
  `cip-desktop.exe` confirms the three chained command names
  (`approve_suggestion`, `prepare_presentation`, `display_presentation`)
  are still registered in the invoke handler's command-name table and as
  their own function symbols. The frontend orchestration logic itself
  lives in the minified JS bundle Tauri embeds into the binary, verified
  via the frontend test suite rather than disassembling minified JS.
- Runtime DLLs, model picker, worker-thread decoupling, backpressure
  instrumentation, whisper feature, segmentation/router, overload fix:
  all re-verified present and unaffected - see `pilot-evidence/3.8.7.8/`.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src/components/workspace/actions.ts,
  apps/desktop/src/components/LiveChurchBrain.tsx,
  apps/desktop/src/components/workspace/actions.test.ts,
  release/windows/*
FILES CREATED: docs/phase-3-8-7-8-bible-display-shortcut.md,
  pilot-evidence/3.8.7.8/*
FILES DELETED: NONE
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE
DATABASE / MIGRATIONS: UNCHANGED
BACKEND (Rust) LOGIC: UNCHANGED - zero Rust files touched
NON-BIBLE DOMAINS (Music/Sermon/Service/Content/Correlation): UNCHANGED
DIAGNOSTICS-MODE PENDING SUGGESTIONS PANEL: UNCHANGED
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary proof that the three chained commands remain compiled
  in, and full frontend regression (typecheck/lint/test/build).
- **Environment B (Xvfb)**: unavailable, pre-existing, unrelated.
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's own
  test: does clicking "Display" on a pending Bible finding in the Needs
  Attention queue put the correct reference on the projector in one
  click, without a separate trip to the Presentation card.

## Known limitations

- Bible-only, Needs Attention queue only - the diagnostics-mode Pending
  Suggestions panel still shows Approve, unchanged.
- This phase does not address Whisper transcription quality/hallucination
  on real hardware, which the same operator screenshots also raised -
  that remains a separate, not-yet-scoped investigation (see the
  conversation's own audit of `ai/speech/src/whisper.rs`'s hardcoded 0.75
  confidence placeholder and unconfigured `FullParams` decode thresholds).

## Final gate

| Item | Status |
|---|---|
| Real existing flow traced before implementing (not assumed) | DONE |
| Reused all three pre-existing, already-tested commands - no new backend code | DONE |
| Scope confirmed narrow (Bible domain, Needs Attention only) | DONE |
| Full frontend regression green | DONE |
| Windows artifact rebuilt + fix verified in compiled binary | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.8.7.8: Environment A verification PASS, including direct
proof the chained commands are compiled into the shipped binary. Real
Windows re-test (Environment C) is the pending, decisive gate.**
