# Phase 3.10 — Multi-Screen Presentation Output

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `799de41` (Phase 3.9)

## Why this phase exists

The second of the five pillars the user asked for after sharing the
project's original master architecture document. After Phase 3.9
(Sermon Harvest) closed, the user explicitly confirmed continuing with
the remaining four pillars: multi-screen presentation output,
semantic/paraphrase Bible detection, real audio fingerprinting, and
multi-language support. This phase delivers the first of those four.

See `docs/phase-3-10-multi-screen-audit.md` for the full audit and design
reasoning performed before implementation.

## Design summary

Three fixed display roles - **Stage** (the primary congregation-facing
output, the only display CIP supported before this phase), **Confidence
Monitor** (an operator/platform-facing screen), and **Lobby / Overflow**
(a second room's mirror of Stage) - not an arbitrary N-window system,
because that is what was actually asked for and is honestly buildable and
verifiable in one phase.

The key architectural finding from the audit: "multi-screen" does not
mean multiple concurrent active presentation items. `presentation.rs`'s
"at most one `Active` item per service" invariant (spec section 10) is
completely unchanged - a service still ever has exactly one active item,
now optionally mirrored to more than one physical screen. Tauri's own
`Emitter::emit` already broadcasts to every listening webview with no
target filter, so `PresentationStarted`/`PresentationStopped` needed zero
changes - any screen that has opened and subscribed receives the same
live update Stage always has.

## What was built

- `presentation_display::DisplayScreen` enum (Stage/Confidence/Lobby),
  each with its own fixed window label - Stage keeps `"display"`
  unchanged, so every pre-3.10 behavior for that screen (including
  `display_presentation`'s auto-open) is byte-for-byte identical to
  before this phase.
- `open_presentation_display`/`close_presentation_display` are now
  screen-parametrized. `display_presentation` (the core "Display" action)
  continues to auto-open Stage specifically, unchanged from before;
  Confidence Monitor and Lobby are opened separately, on explicit
  operator request.
- The window-close reconciliation (stopping the active item when its
  display disappears) now only fires when the closing screen was the
  *last* one still open - closing one of several simultaneously open
  screens never blanks the others still genuinely showing content.
- `get_presentation_display_state` reports per-screen open/closed state
  instead of one boolean.
- The Confidence Monitor's extra operator-only metadata (auto-detected
  vs. manual, template name, item status) is rendered from fields already
  present in the existing broadcast payload - no new backend query, no
  fabricated data. Stage and Lobby render identically.
- `PresentationCard` gained a three-row screen control (Stage/Confidence
  Monitor/Lobby-Overflow), each independently openable/closable, replacing
  the single Open/Close Display button.

No change to the presentation domain model, no new database migration,
no new event.

## Full regression result

Backend: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
(both default and `--features whisper`): clean. `cargo test -p cip-desktop`:
256/256 passed (up from 251 - 5 new tests). Frontend: `tsc -b` (0 errors),
`oxlint` (0 errors, same 4 pre-existing warnings), `vitest` 210/210 passed
(existing test literals updated to the new `screens` array shape),
`vite build` clean.

## Windows artifact

Rebuilt with `scripts/build-windows-whisper.sh`; the new window labels,
operator labels, and unchanged command names all directly verified
present via `x86_64-w64-mingw32-strings` inspection of the extracted
binary; every prior phase's fix (whisper feature, Sermon Harvest, Bible
Display shortcut) re-confirmed unaffected - see `pilot-evidence/3.10/`.

## Known limitations

- No per-screen custom templates/branding (e.g. a church logo overlay on
  the Lobby screen) - a real, separate future feature.
- No network/NDI/OBS/HDMI-matrix output - explicitly out of scope since
  the original local-display foundation (`docs/presentation.md`),
  unchanged by this phase.
- Exactly three fixed screen roles, not an operator-configurable
  arbitrary number of screens - a deliberate scope boundary (see the
  audit doc's reasoning), not a limitation of the underlying window
  mechanism.
- The Confidence Monitor's metadata is limited to what
  `PresentationItem` already carries (template, source-suggestion
  presence, status) - it does not show a "next up" preview, since this
  application has no concept of a queued/ordered presentation sequence
  today.
- Of the five pillars the user asked for, two are now delivered (Sermon
  Harvest, multi-screen). Semantic/paraphrase Bible detection, real audio
  fingerprinting, and multi-language support remain **not started**.

## Final gate

| Item | Status |
|---|---|
| Real architecture audited before designing (no assumed single-window coupling) | DONE |
| Domain model invariant (one active item per service) confirmed unchanged, not violated | DONE |
| Zero fabricated data in the Confidence Monitor's extra metadata | DONE |
| Full regression green (backend + frontend) | DONE |
| Windows artifact rebuilt + verified | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.10: Environment A verification PASS.** Real Windows re-test
(Environment C) is the pending, decisive gate, per this project's
standing discipline.
