# Phase 3.8 — Offline Service Replay + Professional Church Operator
# Workspace: Audit

## A. Git baseline

- Branch: `claude/cip-foundation-init-i85g87`
- HEAD at audit start: `d8f6ab5` (Phase 2.7.1, "Content Intelligence
  Operationalization & Church Resource Library UX")
- Working tree: clean
- Recent history reviewed: `d8f6ab5` → `4c6978f` (Phase 3.7) → `45f23b4`
  (Phase 3.6) → `60f3994`/`ce66a38` (Phase 3.5.1/3.5) confirm a continuous,
  unbroken audit-first → implement → test → document → commit discipline
  this phase continues rather than restarts.

## B. Transcript pipeline (spec section 3, "TRANSCRIPT PIPELINE")

Traced directly from source, not assumed:

1. **Final transcript text (single segment, manual/test)**:
   `commands::process_test_transcript(text)` (Bible **Suggestion** path -
   builds one `TranscriptSegment`, calls `pipeline::handle_final_transcript`,
   the exact same function real audio calls). `commands::analyze_bible_transcript(text)`
   is a **separate, pre-existing** command that produces `IntelligenceFinding`s
   via `BibleIntelligenceEngine` into `state.intelligence_findings` (the
   **Finding** path Cross-Domain/Content actually read from -
   `process_test_transcript` alone never populates this queue).
   `commands::analyze_sermon_transcript(text)` and
   `commands::analyze_music_transcript(text)` are the Sermon/Music
   counterparts - each persists its own transcript segment and queues
   findings into `state.sermon_engine`/music equivalents.
2. **Live audio segments**: `commands::handle_audio_chunk` (fed by
   `start_listening`'s `AudioEngine` sink) calls `pipeline::handle_final_transcript`
   for **Bible only** - it does **not** call Sermon, Music, or the Bible
   Finding-path (`analyze_bible_transcript`) automatically. This is a
   pre-existing architectural fact (confirmed by reading the function
   directly), not something this phase changes - Sermon/Content/Cross-
   Domain intelligence have never been automatically wired into live audio,
   only reachable via explicit manual/test commands, even before this
   phase.
3. **State updated**: `AppState.context_manager` (Scripture context,
   shared across every Bible-path call), `AppState.transcript_sequence`
   (a single shared atomic counter every text-accepting command
   increments), `AppState.sermon_engine`/`intelligence_findings`/
   `correlation_queue`/`content_candidate_queue` (all `Mutex`-guarded,
   accumulate across calls within one running process).
4. **Events emitted**: `TranscriptUpdated`, `ScriptureUpdated`,
   `SuggestionCreated`, `SermonFindingDetected`, `ContentCandidateDetected`,
   `CrossDomainCorrelationDetected` (exact existing names, unchanged).
5. **Production orchestration vs. test-only**: none of the commands in
   point 1 are "test-only" in the sense of being fake - they are the same
   production Bible Intelligence Core, Sermon Intelligence engine, and
   Content/Cross-Domain engines a live segment would reach, entered
   through a different (equally real) input path. This project has never
   had a fake/parallel "test engine."
6. **Can replay call an existing production path?** Yes - directly, with
   zero new backend code, by calling the same commands section B.1
   describes, once per segment, in order.
7. **Where would a replay adapter belong?** The frontend. Nothing in the
   backend needs to know "a replay is happening" - from the backend's
   perspective, N sequential command invocations are indistinguishable
   from N pieces of text an operator typed one after another, which is
   exactly what they honestly are.
8. **Timestamps/order/pauses**: `TranscriptSegment.sequence` is a shared
   atomic counter (real, monotonic, not fabricated); order is guaranteed
   by awaiting each command before issuing the next; pauses are trivially
   representable as `setTimeout`/interval delays between calls, entirely
   client-side.
9. **Pause/resume/stop/restart/cancel**: none of this exists today for
   any input mode - it needs to be built, but it is pure frontend
   scheduling state (a cursor into the segment array + a timer handle),
   never a backend concern, per section 13's "input adapter, not engine"
   rule.
10. **Can replay run without audio hardware?** Yes - trivially; none of
    the commands in point 1 touch `AudioEngine`/`SpeechEngine` at all.

**Conclusion: no new Tauri command, no new database table, and no new
intelligence code are required to implement Service Replay.** It is
achievable as a pure frontend scheduler over four pre-existing commands.

## C. Bible (re-confirmation)

Re-verified directly this phase (not merely cited): `database/datasets/bsb/bsb.json`
still parses to 66 distinct books, 1,189 distinct chapters, 31,086 verses,
`licensingStatus: "verified_public_domain"` (same method as Phase 2.7.1's
audit: `python3 json.load`, not a re-citation). Search/browse/verse/range
retrieval/save/reuse/presentation are all unchanged since Phase 2.7.1 -
`git diff d8f6ab5 -- core/bible/ apps/desktop/src/components/library/BibleLibrary.tsx`
is empty as of this audit. Replayed sermon references reach Bible
Intelligence through `process_test_transcript` (Suggestion path, what the
operator reviews/approves/presents) and `analyze_bible_transcript`
(Finding path, what Cross-Domain/Content can correlate against) - both
pre-existing, both real.

## D. Sermon

`SermonIntelligenceEngine` (`core/sermon`, adapted for the generic
`IntelligenceEngine` trait in `core/intelligence/src/sermon_adapter.rs`)
is pure, deterministic, in-process, offline logic with no dataset
dependency - confirmed by reading `sermon.rs`'s own module docs directly.
`commands::analyze_sermon_transcript` does **not** require an active
Sermon Foundation session (`state.active_sermon` is read as `Option`, no
early-return-if-`None` guard) - it works standalone on any text, exactly
like `process_test_transcript`. Replay can exercise the real production
Sermon Intelligence pipeline with zero new code.

## E. Music

Unchanged since Phase 2.7.1: no licensed production dataset exists, and
`MusicProvider` still has no song-enumeration method. `analyze_music_transcript`
remains available and honest (dev/test fixture only, in a non-`Production`
build). Given the sample replay transcript (section 19 of the phase 3.8
spec) contains no music-domain content, and per "do not overbuild," this
phase does not add a music step to the default replay flow - the existing
Offline Test Center's dedicated Multi-Domain scenario already demonstrates
Music Intelligence, honestly labeled, and is preserved (folded into the
renamed Service Replay screen, not deleted).

## F. Content

`commands::analyze_content_intelligence()` takes **no text argument** - it
reads the accumulated `IntelligenceContext` (built from persisted state
for the current service) and maps eligible findings into `ContentCandidate`s.
This is a "call once, after some segments have already been fed" operation,
not a per-segment one - confirmed directly from its signature. Review →
Accept → Save → Reopen is fully durable since Phase 2.7.1
(`saved_content_candidates`, unchanged this phase).

## G. Cross-Domain

`commands::analyze_cross_domain()` is the same shape as Content - no text
argument, reads accumulated findings, correlates via the existing
deterministic `CrossDomainCorrelationEngine`. No engine-to-engine call
exists or is needed; the frontend simply calls this command once after
feeding segments, exactly matching the pre-existing Offline Test Center's
own Multi-Domain scenario precedent (`runsCrossDomain: true`).

## H. Presentation

Unchanged since Phase 3.7/2.7.1 - `PresentationItem`'s `Prepared → Active
→ Stopped` state machine, `build_scripture_slide`/`persist_prepared_item`/
`prepare_to_activate`/`commit_activation`/`stop_active_item`, and startup
crash reconciliation. Replay produces Suggestions/Findings the operator
reviews on the existing Live Service tab exactly as manual entry already
does - nothing here auto-displays anything; Prepare/Preview/Display/Stop
remain explicit, operator-only actions.

## I. UX audit (spec section 4)

Current state, inspected directly:

- `App.tsx`: a simple, un-nested tab bar (`Live Service | Bible | Music |
  History | Offline Test Center`) - no router, no deep-link assumptions to
  break. This is already the "simplest architecture compatible with the
  existing app" section 6 asks for; it needs one rename, not a rebuild.
- `LiveChurchBrain.tsx` (2,266 lines) already has, from Phase 3.5/3.5.1:
  `WorkspaceHeader`, `ServiceControlBar`, `SystemStatusStrip` (a compact,
  human-language status row already showing Microphone/Speech/Bible/
  Display with semantic color/icon/text - never color-alone), plus
  `AttentionQueue`/`IntelligenceFeed`/`PresentationCard` sub-components.
  This is a real, professional, already-audited operator workspace from
  Phase 3.5.1's own dedicated UX-correction phase - re-litigating it from
  scratch would ignore that prior, deliberate work rather than build on
  it.
- **Genuine gap**: nothing anywhere in the app currently distinguishes
  "this text came from a live microphone" vs. "this text was manually
  typed" vs. "this text is a replayed transcript." Spec section 2's
  distinction does not exist yet in any form. This is the one real,
  provable UX gap this phase's audit finds - not a wholesale redesign
  need.
- `TestCenter.tsx` already has real, working infrastructure this phase
  should extend rather than replace: a readiness strip, a manual-entry
  box, five labeled scenarios with honest `expects` text, a Full Service
  runner, and an activity log - exactly the "manual transcript scenarios"
  spec section 36 says to "integrate/reorganize... into the new Service
  Replay experience," not discard.
- `Diagnostics`/`PilotDiagnosticsPanel.tsx` already exists as the
  technical-detail destination (database path, migration count, engine
  status) separate from Operator Mode - re-confirmed unchanged, no
  operator-facing screen currently leaks this detail.

## J. Gap register

| Capability | Existing | Missing | Reusable path | New code needed |
|---|---|---|---|---|
| Sequential, timed, replayable transcript ingestion | Per-segment commands exist | The replay *scheduler* (pause/resume/stop/restart/speed) | `processTestTranscript`/`analyzeBibleTranscript`/`analyzeSermonTranscript`, called in a frontend loop | Frontend only - a new `ServiceReplay` screen (built from/replacing `TestCenter.tsx`) |
| Live/Manual/Replay input-mode distinction | None | The label/disclaimer itself | N/A | Frontend only - explicit UI text, no new state needed beyond the replay screen's own |
| Transcript file import | None | A local, safe read path | Plain `<input type="file">` + `FileReader` (browser API, zero Tauri fs/dialog permission - none is installed, confirmed via `capabilities/*.json`) | Frontend only |
| Sample/demonstration transcript | None | A bundled, clearly-labeled sample | N/A | Frontend only - a constant string, labeled "SAMPLE / DEMONSTRATION TRANSCRIPT" |
| Bible/Sermon/Content/Cross-Domain via replay | Fully real per section B-G | Nothing - already reachable | Existing commands | None |
| Acceptance test proving sequential replay + persistence | Phase 3.7's single-segment pattern exists | A multi-segment version | Extend the exact same real-file-restart technique | One new Rust test |

**No migration, no new Tauri command, no new intelligence engine is
justified by this audit.** Every gap above is closable entirely in the
frontend plus one new backend test.

## K. Implementation plan

1. Rename/expand `apps/desktop/src/components/testcenter/TestCenter.tsx`
   into the Service Replay experience: add transcript paste/file-load, a
   bundled sample transcript, paragraph-based segmentation, and a
   play/pause/resume/stop/restart scheduler with a speed selector
   (0.25x/0.5x/1x/2x/4x/Instant), explicit "SERVICE REPLAY - Simulated
   live transcript" labeling and the exact required disclaimer text. Keep
   the existing five scenarios and Full Service runner as quick-launch
   presets into the same engine (no second test system).
2. `App.tsx`: rename the nav entry from "Offline Test Center" to "Service
   Replay" (same underlying section id/component, `test-center` retained
   internally to avoid an unnecessary rename churn through the codebase,
   or renamed cleanly if trivial - decided during implementation).
3. Zero backend changes to commands/events/schema.
4. New Rust acceptance test `pipeline::tests::phase_3_8_service_replay_full_offline_acceptance`:
   real file-backed DB, real BSB dataset, a short realistic multi-segment
   sermon transcript fed sequentially through `handle_final_transcript`
   (Bible) plus `sermon::analyze_and_queue`/`content_intelligence::analyze_and_queue`/
   `cross_domain::analyze_and_queue` (the same pure functions the
   `analyze_*` commands call), proving Bible detection, Sermon findings,
   Content candidates, and (if the deterministic rules genuinely match)
   Cross-Domain correlation all occur - then closes and reopens the real
   database file to prove persistence, and confirms no stale replay state
   exists (replay position/pause state is never persisted, by design).
5. Full regression, Windows/Linux rebuild, `docs/phase-3-8-service-replay-operator.md`,
   `pilot-evidence/3.8/`, final audit, single commit, push, final report.

**Hard-stop check (spec section 3's list mirrored from prior phases)**:
none apply. The BSB dataset is genuinely available and unchanged.
Sermon/Content/Cross-Domain engines have no dataset dependency and no
licensing concern. No feature proposed requires copyrighted content. No
migration is proposed at all (none is justified). No second intelligence
architecture is required - the entire design is an input-adapter
composing four pre-existing commands. Nothing requires Internet
connectivity. Existing backend contracts fully support the proposed
workflow without modification. The reference screenshot remains UX
inspiration only. Every proposed feature can be implemented honestly with
data that genuinely exists.

**Proceeding to implementation as scoped in section K.**
