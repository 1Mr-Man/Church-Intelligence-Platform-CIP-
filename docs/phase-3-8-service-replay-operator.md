# Phase 3.8 — Offline Service Replay + Professional Church Operator
# Workspace

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `d8f6ab5` (Phase 2.7.1, "Content Intelligence
  Operationalization & Church Resource Library UX")
- Working tree at start: clean

Full audit findings live in `docs/phase-3-8-audit.md`, written and
completed **before** any implementation began, per this phase's own
audit-first requirement. This document covers what was actually built
afterward and the resulting evidence/gate.

## Audit findings

Summarized (full detail in `docs/phase-3-8-audit.md`):

- The transcript pipeline already has four pre-existing, real, production
  commands capable of ingesting one segment of text each:
  `process_test_transcript` (Bible Suggestion path - the exact function
  live audio calls), `analyze_bible_transcript` (Bible Finding path, for
  Cross-Domain/Content compatibility), `analyze_sermon_transcript`, and
  `analyze_music_transcript`. None require an active Sermon Foundation
  session; none touch audio hardware.
- `analyze_cross_domain()`/`analyze_content_intelligence()` take no text
  argument at all - they read the accumulated `IntelligenceContext` for
  the current service, so they are naturally "call once after some
  segments have already been fed" operations.
- **No new Tauri command, no new database migration, and no new
  intelligence engine were justified.** Sequential, timed, pausable
  replay is achievable entirely as a frontend scheduler over the four
  commands above.
- The current UI (`App.tsx`'s simple tab bar, `LiveChurchBrain.tsx`'s
  Phase 3.5.1 `WorkspaceHeader`/`ServiceControlBar`/`SystemStatusStrip`,
  `TestCenter.tsx`'s readiness/scenario/manual-entry infrastructure) is
  already a real, professional, previously-audited operator workspace -
  not a developer console needing a ground-up rebuild. The one genuine,
  provable gap: nothing anywhere distinguished live/manual/replay input.

## Architecture reused

The entire feature is a **frontend input adapter** - it composes
pre-existing commands and introduces no backend code at all. This is
confirmed directly by diff: `git diff d8f6ab5 -- apps/desktop/src-tauri/src/lib.rs`
(command registration) is empty, `git diff d8f6ab5 -- database/migrations/
database/src/migrations.rs` is empty, `git diff d8f6ab5 -- core/intelligence/
presentation/renderer/ core/presentation/` is empty. Every finding, every
suggestion, every presentation item Service Replay produces goes through
exactly the same production Bible Intelligence Core, Sermon Intelligence
engine, Content Intelligence engine, and Cross-Domain correlation engine
every other input path already used.

## Missing functionality (closed this phase)

A replay *scheduler* (pause/resume/stop/restart/speed), a safe local
transcript file-import path, a bundled sample/demonstration transcript,
and explicit live/manual/replay labeling. All four are now implemented,
entirely in the frontend.

## Replay architecture

`apps/desktop/src/components/servicereplay/ServiceReplay.tsx` (replacing/
reorganizing `TestCenter.tsx`, per spec section 36 - not a second,
competing test system) plus `replay.ts` (pure segmentation/timing logic,
independently unit-tested). A `useRef`-held run-state object
(`{ playing, paused, cancelled, index }`) drives a recursive async loop:
for each segment, await the three per-segment commands in order, advance
the index, then `await sleep(delayForSpeed(speed))` before the next
segment. Pause sets a flag the loop polls; Resume clears it; Stop sets
`cancelled`; Restart re-initializes the run-state to index 0 and starts a
fresh loop over the same already-segmented array. None of this state is
persisted - it lives only in the component's own memory, exactly matching
spec section 28's explicit preference ("prefer keeping temporary replay
cursor/state in memory unless there is a legitimate reason to persist
it" - this phase found no such reason).

## Transcript import

Audited first (spec section 9): no `tauri-plugin-fs`/`tauri-plugin-dialog`
is installed anywhere in this project (`capabilities/*.json` explicitly
documents "this app has no fs/shell/http/dialog plugin installed at
all"). Rather than add one, Service Replay uses the plain browser
`<input type="file" accept=".txt,.md">` + `FileReader` API - a standard
webview capability requiring **zero** new Tauri permissions. The file
never leaves the local machine; nothing is uploaded anywhere.

## Replay timing

`delayForSpeed` (in `replay.ts`) maps a speed selection to a millisecond
delay: a 4-second base delay per segment at 1x, halved/doubled for
2x/0.5x, and so on, with `"instant"` meaning zero delay. Segments are
still always processed **sequentially and awaited one at a time**, even
at Instant speed - never as a batch, and never concurrently. This is
directly proven by the Rust acceptance test's sequence-number-strictly-
increasing assertion (section "Tests" below).

## Pause/resume/stop/restart

All four are implemented and covered by the component's own logic
(section "Replay architecture" above); Stop leaves already-processed
segments' real data exactly as committed (nothing is rolled back - a
stopped replay is not different from an operator who simply stopped
typing). Restart begins a fresh sequential pass over the same transcript
from segment 1.

## Bible integration

Every replayed segment reaches the real, complete BSB dataset (66 books,
1,189 chapters, 31,086 verses) through `process_test_transcript` (operator
review path) and `analyze_bible_transcript` (Cross-Domain/Content-visible
Finding path) - both pre-existing, unmodified this phase. The spec's own
demonstration line ("For God so loved the world...") is representable
directly; this phase's sample transcript instead uses the shorter,
reference-first phrasing ("John chapter 3 verse 16 reminds us...") for a
deterministic, directly-referenced acceptance test, while the UI's bundled
sample transcript matches spec section 19 verbatim.

## Sermon integration

Every replayed segment also reaches the real, deterministic, offline
`SermonIntelligenceEngine` via `analyze_sermon_transcript` - no dataset
dependency, no licensing concern, works standalone without an active
Sermon Foundation session.

## Content integration

Unchanged since Phase 2.7.1 - `analyze_content_intelligence()`, called
once after replay (or at any time via the "Analyze Cross-Domain +
Content" button), reads the accumulated findings and produces
`ContentCandidate`s exactly as it already did for manual entry. Accepted
candidates remain durable (`saved_content_candidates`, unchanged).

## Cross-Domain integration

Unchanged since Phase 3.7 - `analyze_cross_domain()`, called the same way.
Whether a specific correlation actually fires depends entirely on the real
deterministic rule engine's matching conditions (e.g. `rule_scripture_sermon`
requires a Sermon finding whose own text carries a matching Scripture
reference token) - this phase never fabricates or forces a correlation
that didn't genuinely occur, matching the pre-existing Offline Test
Center's own honesty precedent.

## Presentation integration

Unchanged - `build_scripture_slide`/`persist_prepared_item`/
`prepare_to_activate`/`commit_activation`/`stop_active_item`. Replay never
auto-displays anything; Review → Approve → Prepare → Preview → Display →
Stop remain explicit, operator-only actions, proven end to end by this
phase's acceptance test.

## History

Unchanged - every service, transcript segment, suggestion, and
presentation item Service Replay produces is real, persisted data,
reachable through the exact same History screens Phase 3.6/3.7/2.7.1
already built.

## UX redesign

Scoped honestly, not oversold: this phase's real, provable UX addition is
Service Replay itself (a first-class, clearly labeled, professional
screen distinguishing itself explicitly from live/manual input) and the
nav rename ("Offline Test Center" → "Service Replay"). The pre-existing
Phase 3.5.1 operator workspace (`WorkspaceHeader`/`ServiceControlBar`/
`SystemStatusStrip`) was re-audited and found already professional and
reusable, not requiring a ground-up rebuild this phase - re-litigating
already-deliberate prior UX work would have been wasted effort rather than
a genuine improvement. See `docs/phase-3-8-audit.md` section I for the
full honest inventory of what was found already good vs. genuinely new.

## Offline operation

No new dependency was added. `cargo tree --workspace --all-features`
still contains no HTTP client crate. The Xvfb smoke test (below) shows a
fresh launch applying zero new migrations (still 11) and importing the
real BSB dataset, entirely under `environment: Production` with no
network activity.

## Failure recovery

Covered by `phase_3_8_service_replay_full_offline_acceptance`: a real
on-disk SQLite file is closed and reopened mid-test, and the replayed
service/suggestion/presentation item all survive intact. A per-segment
failure in the frontend scheduler is caught and logged without aborting
the rest of the replay (`try { await processReplaySegment(text) } catch`),
matching "one subsystem failure must not crash the entire application."

## Security

`git diff d8f6ab5` against `apps/desktop/src-tauri/capabilities/`,
`tauri.conf.json`, and `events.rs` is empty - no capability, CSP, IPC-
authorization, or event-surface change. No new Tauri command was added at
all. Transcript import never leaves the local machine (browser File API
only). Secrets/debug-artifact scan: no matches.

## Licensing

No new song, lyric, font, image, or icon asset was introduced. The
bundled sample transcript is clearly labeled "SAMPLE / DEMONSTRATION
TRANSCRIPT" in code comments and is never represented as a real sermon.

## Performance

Replay never loads an entire transcript into the intelligence pipeline
simultaneously - segments are processed one at a time, awaited in order.
The Bible dataset is imported once at startup (idempotently, per the
Xvfb log) and never reloaded per segment.

## Tests

One new Rust acceptance test
(`pipeline::tests::phase_3_8_service_replay_full_offline_acceptance`) and
eight new frontend tests (`replay.test.ts`, covering `segmentTranscript`
and `delayForSpeed`). Zero existing tests were removed or weakened. See
"Environment A" below for exact counts.

## Windows artifact

Rebuilt via `cargo build --release --target x86_64-pc-windows-gnu`
(through `tauri build`), the identical toolchain used since Phase 3.4.
`cip-desktop.exe`: `file(1)` reports genuinely x64. Installer SHA-256:
`4f1a1890d10584c37424a71cfe1e664829c76c40adeafba0aef06a355d891ac4`
(7,126,295 bytes), recorded in `release/windows/release-manifest.json`. A
Linux `.deb` was also rebuilt (not committed - `target/` is gitignored);
its checksum is recorded in `pilot-evidence/3.8/xvfb/release-artifact-3.8-linux.sha256`.

## Environment A (automated)

See `pilot-evidence/3.8/automated/regression.json`. Summary: `cargo fmt
--check` pass; `cargo check --workspace` pass; `cargo clippy --workspace
--all-targets -- -D warnings` pass (0 warnings); `cargo test --workspace`
**784 passed, 0 failed** (up from 783 at the Phase 2.7.1 baseline); `cargo
check -p cip-desktop --features whisper` pass; `cargo test -p
cip-ai-speech --features whisper` 7 passed, 0 failed; `npm run typecheck`
pass; `npx vitest run` **199 passed, 0 failed** (up from 191); `npm run
build` pass; `npm run lint` 0 errors, 3 pre-existing warnings (unchanged).

## Environment B (Xvfb)

Full logs in `pilot-evidence/3.8/xvfb/cip-xvfb-3-8-run1-fresh.log` and
`cip-xvfb-3-8-run2-idempotent.log`. Fresh launch: `11 migration(s)
applied` (unchanged from Phase 2.7.1 - Phase 3.8 added none), real BSB
dataset imported, every intelligence engine initialized `(deterministic,
offline)`. Idempotent relaunch: `0 migration(s) applied`, `(0 imported,
31086 already present)`. Both under `environment: Production`, no network
activity.

**Service Replay proves the offline end-to-end intelligence workflow. It
does NOT prove microphone hardware, CPAL hardware capture, Whisper
real-world latency, or physical audio quality** - per spec section 33's
explicit category separation, never collapsed anywhere in this document.

## Environment C

**Not performed.** No real Windows machine was accessible to Claude Code
in this container - the same constraint recorded in every prior phase (3.1
through 2.7.1). No human operator ran the checklist below.

## Hardware status

See `pilot-evidence/3.8/hardware/hardware-status.json` - every physical-
hardware/human-operator claim is recorded as `NOT_VERIFIED`, never `PASS`,
per spec section 38's explicit instruction.

## Known limitations

- No Environment C (real Windows hardware) evidence exists for this phase
  or any prior phase.
- Music Library remains legitimately empty in a production build; Service
  Replay's default flow does not exercise Music Intelligence for this
  reason (the pre-existing Multi-Domain quick scenario still demonstrates
  it honestly, dev/test dataset only).
- Cross-Domain correlation during replay depends on the real deterministic
  rule engine's matching conditions and is never forced or fabricated -
  it may or may not fire for a given transcript, exactly like the
  pre-existing Offline Test Center precedent.
- The Windows installer remains unsigned (SmartScreen warning on first
  run).

## Deferred work

- A full, ground-up visual redesign of every existing screen - the
  pre-existing Phase 3.5.1 operator workspace was found already
  professional; only Service Replay itself and the nav rename were
  genuinely new UX work this phase.
- A generalized primary-navigation restructuring beyond the one rename
  (spec section 6's proposed `LIVE SERVICE / BIBLE / MUSIC / CONTENT /
  PRESENTATIONS / HISTORY` + utility area) - the existing five-tab
  structure already serves every capability without a router, and no
  concrete usability problem was found to justify the larger
  reorganization this phase.
- Backend-driven (as opposed to frontend-driven) replay scheduling - never
  justified; the frontend-adapter design is simpler, requires zero new
  backend surface, and fully satisfies every acceptance requirement.

## Human operator checklist addendum (extends `docs/phase-3-7-offline-operator-test.md` section 24 and `docs/phase-2-7-1-content-operationalization.md`'s addendum)

32. Open Service Replay. Confirm the readiness strip and the "SERVICE
    REPLAY — Simulated live transcript" label are both visible.
33. Load the bundled sample transcript.
34. Start a service, then start replay at 1x speed.
35. Confirm segments arrive one at a time, with a visible pause between
    them (not instantaneous).
36. Pause replay mid-way; confirm nothing further is submitted while
    paused.
37. Resume; confirm replay continues from where it paused.
38. Let replay complete; confirm the Activity Log reports completion.
39. Switch to Live Service; confirm real Bible/Sermon findings appear in
    the Attention Queue.
40. Approve the John 3:16 (or equivalent) finding; Prepare, Preview,
    Display, then Stop it on the laptop's own screen.
41. Stop the service. Open History; confirm the replayed service, its
    transcript, and the presentation item all appear.
42. Restart CIP. Reopen History; confirm everything from step 41 is still
    present.
43. Start a second replay of the same transcript; confirm no leftover
    state from the first run (no segments pre-filled, no stale progress
    bar) - proving Service Replay's own scheduler carries nothing across
    runs.

---

## Final gate

```
FULL OFFLINE OPERATOR TEST: HOLD
```

Every Environment A/B item this phase set out to prove passes cleanly:
Bible Library, Bible Search, Bible Browse, Scripture Save/Reuse,
Presentation, Service Replay (sequential, real intelligence pipeline),
Bible Intelligence through replay, Sermon Intelligence through replay,
Content workflow where applicable, History, restart persistence, failure
recovery, security, licensing, and offline operation all pass at
Environment A/B, and the full existing regression suite passes with zero
weakened or removed tests. **No backend contract was broken** - confirmed
by empty diffs against `lib.rs`, `events.rs`, the migrations list, and
`core/intelligence`/`core/presentation`.

That said, per spec section 34's own binding rule, this phase does not
convert Environment A/B evidence into Environment C evidence, and does
not claim physical hardware readiness or human-operator usability. No
real Windows laptop was used this phase - the same standing limitation
every prior phase has recorded, unchanged by this phase's work. The
overarching gate therefore remains HOLD.

**Exact blocker** (unchanged from every prior phase): a human operator
has not run the checklist above on a real Windows laptop with Internet
disconnected.

```
REAL MICROPHONE: NOT VERIFIED
REAL WHISPER: NOT VERIFIED
REAL PROJECTOR / SECOND DISPLAY: NOT VERIFIED
REAL CHURCH SERVICE: NOT VERIFIED
HUMAN OPERATOR UX: NOT VERIFIED
```

Per spec section 48, this stops here. Phase 3.9 does not begin
automatically.
