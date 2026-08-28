# Phase 3.8.1 — Service Replay Intelligence + Professional
# Live-Service Operator Workspace

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `9eb1ea2` (Phase 3.8, "Offline Service Replay +
  Professional Church Operator Workspace")
- Working tree at start: clean

Full audit findings live in `docs/phase-3-8-1-audit.md`, written and
completed **before** any implementation began. This document covers what
was actually built afterward and the resulting evidence/gate.

## Why this phase exists

The user tested the Phase 3.8 Windows build on a real HP EliteBook with a
real ~52-minute sermon transcript (Pastor Poju Oyemade, WOFBEC 2026) and
reported two defects:

1. The transcript was reduced to only 2 segments.
2. While replay ran, the Service Replay screen showed no Bible detections,
   sermon insights, theme, key points, or attention items - only a plain
   activity log and diagnostics.

Both are confirmed, root-caused, and fixed below. Neither required any
backend change.

## Root cause of the 2-segment collapse

`segmentTranscript` (Phase 3.8's `replay.ts`) returned blank-line-delimited
paragraphs verbatim, with no upper bound on size, whenever there was more
than one such paragraph. A real transcript with only two genuine
paragraph-gaps (common when most line breaks inside the file are single
newlines) collapsed to exactly two unbounded segments - each potentially
containing half a 52-minute sermon.

**Fix**: every paragraph (whether there is one or many) is now split into
sentences and re-grouped into bounded-size chunks (a ceiling of 220
characters, roughly two to three spoken sentences) instead of being
returned as one unbounded block or split down to one sentence per segment.
An optional timestamp-cue parser was also added: if the transcript
contains at least two standalone timecode lines (the common
subtitle/export convention - `00:00:04.560 --> 00:00:13.920`,
`[00:00:04 - 00:00:13]`, or a single `00:00:04:`), segments are built from
those cues instead, carrying the real timestamp label through to the UI.

## Root cause of the missing intelligence display

Traced end-to-end (full detail in `docs/phase-3-8-1-audit.md` section C):
every command Service Replay calls per segment
(`process_test_transcript`, `analyze_bible_transcript`,
`analyze_sermon_transcript`) already **returns** real results and
**emits** the exact same events (`SuggestionCreated`, `ScriptureDetected`,
`SermonFindingDetected`, `SermonStateChanged`, etc.) that the Live Service
tab (`LiveChurchBrain.tsx`) already subscribes to and renders correctly.
The defect was never a missing backend capability - `ServiceReplay.tsx`
simply never fetched or subscribed to any of it. While an operator watched
a replay run, they were necessarily looking at the one screen that
*didn't* render intelligence, not the one that did.

**Fix**: `ServiceReplay.tsx` now mounts the same read model
`LiveChurchBrain.tsx` uses for the active service - the same
`commands.list*`/`commands.get*` calls, the same `liveEvents.on*`
subscriptions, and the same unmodified presentational components
(`WorkspaceHeader`, `SystemStatusStrip`, `AttentionQueue`,
`IntelligenceFeed`, `PresentationCard`) - so Scripture detections, Sermon
Intelligence (theme/key points), Needs Attention, and the Presentation
queue are all visible on the Service Replay screen while a replay is
actively running, not only on a separate tab. `LiveChurchBrain.tsx` itself
was not modified.

## Architecture reused (no new pipeline)

Confirmed via diff against `9eb1ea2` (all empty):

```
apps/desktop/src-tauri/src/events.rs, apps/desktop/src/events/
apps/desktop/src-tauri/src/lib.rs
core/intelligence/, presentation/renderer/, core/presentation/
database/migrations/, database/src/migrations.rs
apps/desktop/src-tauri/capabilities/, apps/desktop/src-tauri/tauri.conf.json
```

Every fix lives in `replay.ts` (pure segmentation logic),
`ServiceReplay.tsx` (mounting pre-existing commands/events/components),
and one new Rust test. Zero new Tauri commands, zero new database
migrations, zero new intelligence engines - exactly the same discipline
Phase 3.8 established.

## Transcript segmentation (revised)

`segmentTranscript` now:

1. Checks for a cue-based (timestamped) transcript first - at least two
   standalone timecode lines segment the text by cue, preserving each
   cue's real timestamp label.
2. Otherwise splits on blank lines into paragraphs (or treats the whole
   text as one paragraph if there are none), then chunks every paragraph's
   sentences into bounded groups (≤ 220 characters each), never returning
   an unbounded block and never splitting all the way down to one sentence
   per segment for a long paragraph.

Twelve unit tests in `replay.test.ts` cover both branches, including a
dedicated regression test reproducing the exact reported defect (two large
paragraphs must not collapse to two giant segments) and cue-line parsing
for both `-->`-style and bracketed single-timestamp conventions.

## Real-time intelligence surfacing

`ServiceReplay.tsx` fetches, on the active service becoming set:
`suggestions` (pending + approved), `musicFindings`, `sermonFindings`,
`sermonState`, `sermonFoundation`, `crossDomainCorrelations`,
`contentCandidates`, `serviceTransitions`, `serviceAnomalies`,
`preparedItems`, and the presentation display state - then subscribes to
every corresponding `liveEvents.on*` listener so each update lands live
while replay runs, exactly as it already does on the Live Service tab.
`buildUnifiedFeed`/`buildAttentionQueue` (unmodified) derive the same
Needs Attention list. New replay-local state (`currentlyHearing`,
`elapsedMs`) shows what segment is playing and how long the run has been
going, satisfying the "SERVICE STATUS / CURRENTLY HEARING" part of the
professional workspace visual model without inventing a new data source.

## "Service already active" handling

`startTestService`/`startReplay` now detect the specific backend error
text (`"a service is already active - end it before starting a new one"`)
and render an inline banner with an "End Service" button, instead of only
the raw error string.

## Presentation, History, offline behavior

Unchanged: the same Prepare → Preview → Display → Stop lifecycle
(`PresentationCard`, reused verbatim), the same History screens, no new
network capability, no HTTP client in the dependency graph, transcript
import remains the plain browser File API with zero new Tauri
permissions.

## Tests

- Rust workspace: **785 passed, 0 failed** (up from 784 - 1 new
  `phase_3_8_1_service_replay_progressive_intelligence_acceptance` test).
  `cargo fmt --check`, `clippy -D warnings`: clean.
- Whisper feature (`cip-ai-speech --features whisper`): 7 passed, 0
  failed.
- Frontend: **203 passed, 0 failed** (up from 199). `typecheck`, `build`,
  `lint`: clean (4 warnings total - the same 3 pre-existing baseline
  warnings, plus 1 new `set-state-in-effect` warning in
  `ServiceReplay.tsx`, the same category/pattern as the already-accepted
  one in `LiveChurchBrain.tsx`'s own `activeServiceId` reset effect).

The new Rust acceptance test feeds 16 sequential segments (a longer,
project-authored synthetic sermon - the user's real transcript was never
supplied verbatim, only a list of the Scripture references it contains,
so it could not be reproduced) and specifically proves **progressive**
delivery: the accumulated finding count is asserted to grow at two or
more distinct points during the sequence, never only in one final batch -
the concrete regression this phase fixes. No reference or finding is
hardcoded as an expected result anywhere in the test.

## Visual/UX verification

No screenshot capability exists in this environment. The Xvfb smoke test
below proves the rebuilt Linux binary launches, opens its database, and
runs offline - it does not constitute visual verification of the
Service Replay layout, and no such claim is made. Real visual/UX
verification requires the user's own Windows machine.

## Offline verification (Environment B / Xvfb)

Fresh launch: `pilot-evidence/3.8.1/xvfb/cip-xvfb-3-8-1-run1-fresh.log` -
11 migrations applied, full BSB dataset imported, `environment:
Production`. Idempotent re-launch against the same data directory:
`cip-xvfb-3-8-1-run2-idempotent.log` - 0 migrations, all 31,086 verses
already present.

## Windows artifact

Rebuilt: `Church Intelligence Platform_0.1.0_x64-setup.exe`, SHA-256
`a5c17483876a1ddfa2d3c2e1b5a6a7ef4c1c73e86e0949b146dd91f0c0f3cf2f`,
7,127,913 bytes, confirmed genuinely x64 via `file(1)` against the
embedded `cip-desktop.exe`. `release-manifest.json` updated to this
commit.

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above.
- **Environment B (Xvfb)**: full pass, detailed above. Linux runtime/smoke
  only - never physical or Windows evidence.
- **Environment C (real Windows hardware)**: **not performed** against
  this rebuilt artifact. No physical Windows machine was accessible to
  Claude Code in this container. The user's own prior Windows testing
  (which reported the defects this phase fixes) was against the Phase 3.8
  build, not this one - it is real Environment C evidence of the *prior*
  build's defects, not of this fix.

## Known limitations

- The aspirational ground-up visual/UX redesign described across the
  Phase 3.8 spec's sections 4-7/20-26/42 (color language, responsive card
  layout, full Diagnostics-panel migration of every technical detail) was
  deliberately not attempted this phase either - only Service Replay's own
  real-intelligence display was added, reusing existing Live Service
  components verbatim, consistent with "reuse what already works, fix what
  is actually broken."
- Pause/resume/stop/restart remain pure frontend scheduler state with no
  backend interaction; they are not separately exercised by the new Rust
  acceptance test (they are unaffected by the same sequential command
  calls that test does exercise).
- This rebuilt artifact has not yet been tested on the user's real Windows
  hardware.

## Deferred work

Full ground-up UX redesign, broader crash-injection testing beyond what
Phase 3.7/3.8 already cover, and all real-hardware qualification remain
deferred pending user access to physical Windows hardware.

## Final gate

**FULL OFFLINE OPERATOR TEST: HOLD**

```
REAL MICROPHONE: NOT VERIFIED
REAL WHISPER: NOT VERIFIED
REAL PROJECTOR / SECOND DISPLAY: NOT VERIFIED
REAL CHURCH SERVICE: NOT VERIFIED
HUMAN OPERATOR UX: NOT VERIFIED
```

This stops here. Phase 3.9 does not begin automatically.
