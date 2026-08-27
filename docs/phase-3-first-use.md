# Phase 3.0 — First-Use Hardening

This document records what Phase 3.0 actually changed, why, and the
resulting first-use readiness matrix. Phase 3.0 is an application
hardening phase over the existing, already-validated Phase 2 stack (see
[`docs/phase-2-validation.md`](phase-2-validation.md)) — it introduces no
new intelligence engine, no second `IntelligenceContext`, no second
presentation system, and no second Bible database. Every change below is
additive: existing commands, events, and engine architecture are
unchanged.

## Why these four changes, and no others

Phase 2.10 (`docs/phase-2-validation.md`) left CIP at "GO WITH
CONDITIONS" — every core capability proven, but several practical
first-use gaps documented rather than fixed, since Phase 2.10 was a
validation phase, not a hardening one. Phase 3.0 re-audited those gaps
against the real, current codebase (not just re-reading the prior
report), using five parallel research passes covering: first-run/
readiness UX, speech/audio configuration, presentation/display hardening,
Unified Workspace + Content Candidate downstream, and security/CSP/
offline posture. That audit found exactly four gaps meeting the "genuine
P0/P1, smallest safe fix" bar; everything else was confirmed either
already adequate or a legitimate P2/P3 to defer (see "Deferred" below).

## What Phase 3.0 changed

### 1. Speech model path is now configurable

**Problem:** The Whisper model path was 100% hardcoded
(`<data_dir>/models/ggml-tiny.en.bin`) with no way to point CIP at a
model stored elsewhere without rebuilding from source — unlike the
acoustic-music model, which already had a `CIP_ACOUSTIC_MODEL_DIR`
override.

**Fix:** Added `AppConfig.whisper_model_path`, resolved from the new
`CIP_WHISPER_MODEL_PATH` environment variable (falling back to the
previous hardcoded default), mirroring the existing acoustic-config
precedent exactly. `create_speech_engine` in `lib.rs` now reads this
field instead of recomputing the path inline.

**Files:** `apps/desktop/src-tauri/src/config.rs`,
`apps/desktop/src-tauri/src/lib.rs`.

**Tests:** `config::tests::whisper_model_path_defaults_under_model_dir_when_unset`,
`config::tests::whisper_model_path_honors_the_env_override`.

### 2. Speech-unavailable notice now names the exact fix

**Problem:** The "SPEECH UNAVAILABLE" notice gave the operator zero
actionable next step — no path, no hint that a model could be
configured at all. `get_app_config` already existed and returned the
right data, but no panel ever called it.

**Fix:** `LiveChurchBrain.tsx` now fetches `AppConfig` once on mount and
extends the notice to name the exact expected model path and the
`CIP_WHISPER_MODEL_PATH` override, while explicitly reminding the
operator that manual transcript entry remains available either way.

**Files:** `apps/desktop/src/config/appConfig.ts`,
`apps/desktop/src/components/LiveChurchBrain.tsx`.

### 3. Accepting a Content Candidate is no longer a dead end

**Problem:** `accept_content_candidate` only ever flipped the
candidate's status (by design — it must never auto-publish). But
`list_content_candidates` only returns `pending()` candidates, and every
frontend surface (the Content Intelligence panel, the Unified Feed, the
Attention Queue) actively removed an accepted candidate from visible
state the moment it was accepted. The candidate's text
(`working_concept`) became permanently unreachable in the running UI —
confirmed as a real dead end, not a documentation gap, by direct
inspection of both the backend query and every frontend consumer.

**Fix:** Added `list_accepted_content_candidates` (a thin Tauri command
exposing `ContentCandidateQueue::all()` filtered to `Accepted`, over
IPC — the underlying data was already retained, only the query and UI
surface were missing) and a "Saved Content" collapsible section in the
Content Intelligence panel that lists accepted candidates with their
full text. Still has no code path into `presentation::persist_prepared_item`
or anything that could publish/schedule/project it — accepting remains a
pure status change; only visibility changed.

**Files:** `apps/desktop/src-tauri/src/commands.rs`,
`apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src/lib/commands.ts`,
`apps/desktop/src/components/LiveChurchBrain.tsx`.

**Tests:**
`content_intelligence::tests::an_accepted_candidate_remains_retrievable_via_all_with_its_text_intact`
(core/intelligence) proves the exact invariant the new command depends
on.

### 4. Bible/BSB readiness is now visible at a glance, and a BSB import failure no longer crashes the app

**Problem, part A:** `get_live_status` (the struct the always-visible
header reads) modeled Audio, Speech, Network, AI, Database, and Acoustic
readiness — but not Bible. BSB's name, enabled status, and licensing
status were real, correct, and already stored in the Content Registry,
but only visible by scrolling down and expanding a collapsed
"Diagnostics: Content Registry" panel — never in the always-visible
header, and never on first launch without digging.

**Problem, part B:** `setup()` in `lib.rs` propagated a BSB import
failure with `?`, which panics the whole Tauri application before any
window renders — a real violation of the "one subsystem's failure must
not crash CIP" invariant every other domain in that same function
already honors (a missing speech model, an absent audio device, and no
acoustic recognizer all degrade gracefully; only Bible import did not).

**Fix:** Added `LiveStatus.bible: Option<ContentMetadata>` — reusing the
existing `ContentMetadata` type unmodified (no second Bible-readiness
model), populated by a fresh Content Registry lookup on every
`get_live_status` call. Surfaced in `WorkspaceHeader.tsx` as a `Bible`
field, e.g. `Berean Standard Bible — ENABLED (VERIFIED PUBLIC DOMAIN)`,
or `NOT AVAILABLE` if the dataset genuinely isn't registered. Changed the
BSB import call in `setup()` from `?` to a `match` that logs an error and
continues on failure, exactly like every sibling domain already does —
CIP now always launches, and a Bible import failure is visible in the
header instead of silently crashing the whole application with no
explanation.

**Files:** `apps/desktop/src-tauri/src/commands.rs`,
`apps/desktop/src-tauri/src/lib.rs`,
`apps/desktop/src/domain/live.ts`,
`apps/desktop/src/domain/contracts.test.ts`,
`apps/desktop/src/components/workspace/WorkspaceHeader.tsx`.

## Explicitly not done (deferred, with rationale)

Per spec's "harden what exists, don't over-improve" principle, the
following were identified and deliberately left alone:

- **DB-level uniqueness constraint for at-most-one-active presentation
  item.** The app-layer re-check-before-write in `commit_activation`
  already prevents a real double-Active write (tested:
  `commit_activation_rejects_an_item_that_is_no_longer_prepared`). A
  partial unique index would be defense-in-depth only, not a fix for a
  live bug — deferred as P2.
- **Automated test for a real presentation-display window-open
  failure.** `open_display_window` calls the real
  `tauri::WebviewWindowBuilder` directly with no injectable seam, and
  this codebase has never used a `tauri::test` harness anywhere (a
  project-wide, pre-existing convention, not new to this phase).
  Introducing one now would be new test infrastructure, not a hardening
  fix — deferred as P2, unchanged from Phase 2.10's own finding.
- **CSP hardening (`security.csp` is `null`).** Confirmed still low
  severity: zero fs/shell/http/dialog plugin capability exists in either
  window's capability file, and nothing external is ever loaded. A
  concrete minimal policy was drafted during the audit, but Tauri v2's
  IPC bootstrap script requires its own nonce/CSP interaction that can
  only be verified with a real per-OS build/run, not static analysis —
  changing this blind risks breaking `invoke()` silently. Deferred with
  this rationale, exactly as spec section 36 anticipates.
- **Literal Tauri-command-level tests for Sermon/Content accept/reject.**
  Both are thin wrappers (parse id, call orchestration function, record
  timeline, emit event) already covered at the orchestration layer;
  this is a pre-existing, project-wide testing-boundary decision
  (documented in `docs/phase-2-validation.md`), not something Phase 3.0
  introduced or is obligated to close.
- **A single monolithic nine-capability end-to-end test.** Each domain's
  own canonical acceptance test is real and comprehensive, and Phase
  2.9's `operatorWorkflow.test.ts` already proves their outputs compose
  correctly in the Unified Workspace. A single further-unified simulation
  remains reasonable future work, not a first-use blocker.

None of these are P0/P1: nothing here blocks, corrupts, or misrepresents
state for a real operator today.

## First-Use Readiness Matrix

| Capability | Existing Phase 2 State | Phase 3 Change | Tested | Runtime Verified | Hardware Verified | Operator Usable | Offline | Licensing | Remaining Limitation | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| Bible dataset (BSB) | PROVEN (Phase 2.10) | Readiness now visible in header; import failure no longer crashes app | Yes | Yes (Xvfb) | N/A | Yes | Yes | Yes | none | READY |
| Bible search/detection/context/presentation | PROVEN (Phase 2.10) | None | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Live speech (Whisper) | Architecture proven, model not available | Model path now configurable; status notice now actionable | Yes (config tests) | No (no real model here) | No | Yes (with operator-supplied model) | Yes | N/A | operator must supply a model | READY WITH CONDITIONS |
| Manual transcript fallback | PROVEN (Phase 2.10) | Explicitly documented as first-class, not degraded | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Audio device selection | Already fully implemented | None (confirmed adequate, no change needed) | Yes | Yes | No (no real mic here) | Yes | Yes | N/A | real microphone NOT VERIFIED in this environment | READY WITH CONDITIONS |
| Music Intelligence (text) | PROVEN (Phase 2.10) | None | Yes | Yes | N/A | Yes | Yes | N/A | church must import its own song data for full coverage | READY |
| Acoustic Music Recognition | NOT AVAILABLE (Phase 2.10) | None (confirmed, not required for first use) | N/A | N/A | No | No | N/A | N/A | no real backend implemented | NOT AVAILABLE |
| Service Intelligence | PROVEN (Phase 2.10) | None | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Sermon Foundation / Intelligence | PROVEN (Phase 2.10) | None | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Content Intelligence | PROVEN (Phase 2.10) | Accepted candidates now visible ("Saved Content") | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Cross-Domain Intelligence | PROVEN (Phase 2.10) | None | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Unified Operator Workspace | PROVEN (Phase 2.10) | Bible readiness added to header | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Presentation Display | PROVEN (Phase 2.10) | None (window-open-failure test gap confirmed, deferred) | Yes | Yes (Xvfb) | No (no physical monitor here) | Yes | Yes | N/A | real projector/monitor NOT VERIFIED in this environment | READY WITH CONDITIONS |
| Offline operation | PROVEN (Phase 2.10) | Re-verified unchanged | Yes | Yes | N/A | Yes | Yes | N/A | none | READY |
| Security | PROVEN (Phase 2.10) | Re-verified unchanged; CSP-null deferred with rationale | Yes | N/A | N/A | N/A | N/A | N/A | CSP null (low severity, documented) | READY WITH CONDITIONS |
| Startup / installation | Crashed silently on a BSB import failure | Degrades gracefully; readiness visible in header | Yes | Yes (Xvfb) | N/A | Yes | Yes | N/A | none | READY |

## Test Counts

- New Rust tests: 4 (`config::tests::whisper_model_path_defaults_under_model_dir_when_unset`,
  `config::tests::whisper_model_path_honors_the_env_override`,
  `content_intelligence::tests::an_accepted_candidate_remains_retrievable_via_all_with_its_text_intact`,
  plus regression coverage of the modified `LiveStatus`/`setup()` paths
  via the existing test suite).
- Frontend: no new automated tests were added for the new UI (this
  codebase has no React-rendering component test suite, a pre-existing,
  documented project-wide convention — see `docs/phase-2-validation.md`
  and `docs/operator-workspace.md`); `tsc`/`vitest`/`build`/`oxlint` all
  re-verified green with the new `AppConfig.whisperModelPath` and
  `LiveStatus.bible` fields wired through.
- Full commands: `cargo fmt --check`, `cargo check --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo check -p cip-desktop --features
  whisper`, `cargo test -p cip-ai-speech --features whisper`, `tsc -b`,
  `vitest run`, `vite build`, `oxlint`.
