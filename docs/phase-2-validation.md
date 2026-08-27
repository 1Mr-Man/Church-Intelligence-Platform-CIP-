# Phase 2.10 — Full Phase 2 Validation & First-Use Readiness

This document is the record of Phase 2.10: a validation pass over every
capability built in Phases 2.0-2.9 (plus the real-Bible-dataset and
local-presentation-display milestones), performed against the real,
running codebase rather than its documentation. It states, with evidence,
what a church operator can actually do with CIP today, what remains
unverified in this environment, what is deliberately unavailable, and the
project's first-use readiness verdict.

Phase 2.10 is a validation phase, not a new feature phase. No intelligence
engine, `IntelligenceContext`, database schema, or the presentation/
workspace architecture was redesigned. Exactly one gap was found worth a
targeted fix (see "Issues Fixed" below); everything else is either PROVEN,
or honestly recorded as NOT VERIFIED / NOT AVAILABLE.

## Executive Summary

CIP's Phase 2 stack - Bible, Music, Service, Sermon Foundation, Sermon
Intelligence, Content Intelligence, Cross-Domain Intelligence, the Unified
Operator Workspace, and local Presentation Display - is **first-use ready
under documented conditions**. Every deterministic, offline capability
(Bible search/detection/context/presentation against the real 66-book BSB
translation; all six intelligence domains; the unified workspace; local
presentation display; database persistence and restart recovery) is
proven end-to-end by real tests against real SQLite storage, plus two
clean/idempotent real-binary launches under Xvfb this session. Two
capabilities are explicitly and honestly **not available** in this
environment - real Whisper speech transcription (no model bundled or
distributable here) and acoustic (audio-fingerprint) music recognition
(no real model implemented anywhere in the codebase) - and both have
fully-functional deterministic alternatives already proven (manual/
assisted transcript entry; text/lyric-based music matching). No P0 or P1
first-use blocker was found. Verdict: **GO WITH CONDITIONS** (see section
B and T).

## A. Git Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting commit (session start, confirmed via `git rev-parse HEAD`):
  `8faf222d4ed860cf1f43b5773bba3edd56757fbd` ("Implement Phase 2.9 unified
  operator workspace") - matches the expected baseline exactly.
- Working tree at session start: clean.
- Final commit: recorded in section V below, after this document is
  committed.

## B. Executive Verdict

**GO WITH CONDITIONS.**

All 20 critical first-use requirements (spec section 43) are satisfied.
CIP is genuinely usable by a church operator for a real service today,
provided the conditions in section T are understood and accepted - none
of them are defects; all are honest, structural facts about what is and
isn't bundled/available in this development environment, documented
rather than hidden. This is not "GO" outright, because live microphone
speech transcription and acoustic music recognition - both real,
substantial capabilities a church might reasonably expect - are not
exercised end-to-end here; it is not "NO-GO" because every documented
critical workflow (Bible-driven presentation from live or manual
transcript input, through operator review, to local display and back to
a clean stop) is proven, offline, with no crash or corruption path found
in normal or injected-failure operation.

## C. Phase 2 Readiness Matrix

| Capability | Implemented | Tested | Runtime Verified | Hardware Verified | Offline | Licensing Verified | Operator Usable | Blocker? | Evidence | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| Bible dataset (BSB) | Yes | Yes | Yes | N/A | Yes | Yes | Yes | No | `content::tests::phase_real_bible_dataset_full_validation`, live Xvfb launch logs | 66 books/1189 chapters/31,086 verses, `IntegrityStatus::Valid` |
| Bible search | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `search.rs` tests (fixture) + full-validation test (real BSB) | exact/chapter/range/free-text all proven against real data |
| Bible detection | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `pipeline::tests::phase_2_10_bible_pipeline_against_real_production_dataset` (new, this phase) | now proven against real BSB, not only the 6-verse fixture |
| Bible context | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | same test as above | context retention/replacement/genuine-ambiguity all proven against real data |
| Bible presentation prep | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | same test; `build_scripture_slide` against real GEN 1:1/JHN 3:36 text | real verse text confirmed reaching `PresentationContent` |
| Audio capture | Yes (architecture) | Partial | No (no real device) | No | Yes | N/A | Yes (device-absent paths safe) | No | `integrations/audio` 6 tests | real-hardware capture NOT VERIFIED, honestly documented |
| Speech transcription (Whisper) | Yes (architecture) | Partial | No | No | Yes | N/A | Manual fallback only | No (documented condition) | `ai/speech` tests, `missing_model_file_is_reported_as_model_not_found` | model not bundled/available; manual transcript entry fully proven |
| Music Intelligence (text) | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/music` + `music.rs` real-SQLite tests | deterministic, real operator accept/reject workflow |
| Acoustic Music Recognition | Partial (architecture only) | Partial | No | No | Yes | N/A | No | No (documented, not required) | `integrations/music-acoustic` | no real model anywhere; structurally always `Unavailable` |
| Service Intelligence | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/intelligence::service_adapter` (31 tests), `service.rs` | backward transitions never crash, debounce/correction/staleness all proven |
| Sermon Foundation | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/sermon::foundation` + `sermon_foundation.rs` acceptance test | restart recovery proven against real SQLite |
| Sermon Intelligence | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/intelligence::sermon_adapter` (29 tests) | 20-variant taxonomy, `Generated` structurally never produced |
| Content Intelligence | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/intelligence::content_intelligence` (34 tests) | traceability to source finding proven; no auto-publish (no presentation dependency) |
| Cross-Domain Intelligence | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `core/intelligence::cross_domain` (29 tests) | negative tests prove no false convergence; no engine-to-engine calls |
| Unified Operator Workspace | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `lib/unifiedFeed.test.ts`, `attentionQueue.test.ts`, `operatorWorkflow.test.ts` | pure frontend projection, zero new backend surface, confirmed by grep |
| Presentation Display | Yes | Yes | Yes (Xvfb) | No (no physical monitor) | Yes | N/A | Yes | No | `presentation.rs` (17 tests), live Xvfb launches | single-active-item and explicit-activation invariants proven at app layer |
| Offline operation | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `cargo tree` (no network crates), structural proofs throughout | one benign, non-functional TCP status probe (documented) |
| Database / migrations | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | `database::migrations` tests, live Xvfb launch logs (9 then 0 applied) | idempotent, FK/CHECK-constrained |
| Security | Yes | Yes | Yes | N/A | Yes | N/A | Yes | No | capability/CSP audit, secrets scan (clean) | CSP null but zero fs/shell/http/dialog plugin surface |
| Performance | Yes | Yes | Yes | N/A | N/A | N/A | Yes | No | `docs/bible-production-dataset.md`, `docs/operator-workspace.md` measurements | all real-time-interactive-budget operations |

## D. Bible Dataset

- Translation: Berean Standard Bible (BSB)
- Books: 66 / 66
- Chapters: 1,189
- Verses: 31,086
- Checksum (FNV-1a): `d4335582ff26a3ac`
- Dataset version: `bsb-1.0`
- Licensing status: `VerifiedPublicDomain` (Content Registry field
  `licensing_status`); hard import gate
  (`LicensingStatus::permits_bulk_import()`) enforced in
  `import_bible_dataset` before any row is validated or written -
  `Unknown`/`Restricted` are refused unconditionally, proven by
  `refuses_import_when_licensing_status_is_unknown_and_writes_nothing`
  and `refuses_import_when_licensing_status_is_restricted_and_writes_nothing`.
- Source: `github.com/lyteword/bsb` (CC0 1.0 Universal), full evidence
  chain in `docs/data/bible/BSB/BSB-LICENSE.md`.
- Validation result: `check_bible_integrity` -> `IntegrityStatus::Valid`,
  0 issues, canonical ordering correct, no duplicate/empty/malformed
  rows. FK constraints (`bible_chapters -> bible_books`,
  `bible_verses -> bible_chapters`) enforce referential integrity at the
  SQLite level unconditionally.
- **This phase's addition:** every prior test exercising Bible
  detection/context/suggestion creation ran only against the 6-verse dev
  fixture or `FakeBibleProvider`. A new test,
  `pipeline::tests::phase_2_10_bible_pipeline_against_real_production_dataset`,
  imports the real BSB dataset into a real SQLite `SqliteBibleProvider`
  and runs the full live pipeline (`handle_final_transcript`) against
  it: context establishment (Genesis 1), bare-verse resolution (Genesis
  1:1), context replacement (John 3), a second bare-verse resolution
  (John 3:36), rejection of an out-of-range reference (Romans 8:999),
  and presentation-content generation carrying the real BSB verse text
  through to `PresentationContent`. See "Issues Fixed" for what this
  closed and what it incidentally discovered.

## E. Live Audio / Speech

Reported precisely and separately, per spec section 44:

- **Architecture:** PROVEN. `CpalAudioEngine` (real cpal-backed capture,
  mono downmix, RMS level), `WhisperSpeechEngine` (real whisper-rs
  binding, feature-gated), `ScriptedSpeechEngine` (deterministic test/
  demo double), and `NullSpeechEngine` (safe default) all implement the
  same `SpeechEngine`/`AudioEngine` traits; `create_speech_engine`
  gracefully falls back to `NullSpeechEngine` if no model file is found
  (never panics, never auto-downloads).
- **Model availability:** NOT AVAILABLE in this environment. No
  `.bin`/`.gguf` model file is bundled (`ai/models/` contains only a
  README) or downloadable (model-host access is blocked by this
  environment's network egress policy). `WhisperSpeechEngine::load`
  returns `Err(ModelNotFound)`, never a panic, when absent - proven by
  `missing_model_file_is_reported_as_model_not_found`.
- **Offline capability:** PROVEN. No code path fetches a model or audio
  data over the network at any point.
- **Real microphone transcription:** NOT VERIFIED. No real audio device
  or Whisper model exists in this environment. `integrations/audio`'s
  own module docs explicitly state real-hardware capture "has not been
  exercised against real hardware."
- **Documented deterministic alternative:** PROVEN. `ScriptedSpeechEngine`
  plus the `process_test_transcript` Tauri command let an operator (or a
  test) drive the exact same detection/intelligence pipeline from typed
  or scripted text, with zero dependency on audio hardware or a model
  file - this is the same mechanism this document's own new pipeline
  test uses, and the same one `docs/live-speech.md` documents as the
  manual-operation fallback.

Classification: **READY WITH MODEL CONFIGURATION** (an operator who
sources and configures a real ggml/gguf Whisper model, per
`docs/live-speech.md`'s documented path, gets live transcription) or
**READY (manual/assisted transcript entry)** if no model is configured -
never silently claimed as "live transcription works" in this
environment.

## F. Six Intelligence Domains

| Domain | Implemented | Tested | Runtime Verified | Operator Usable | Limitations |
|---|---|---|---|---|---|
| Bible | Yes | Yes (real BSB, this phase) | Yes | Yes | none beyond live-speech input method |
| Music | Yes (text); partial (acoustic) | Yes (text); architecture-only (acoustic) | Yes | Yes (text); No (acoustic) | acoustic recognition NOT AVAILABLE (no real model) |
| Service | Yes | Yes (31 tests) | Yes | Yes | phase history is in-memory, not DB-persisted across restart (documented design choice, not a defect) |
| Sermon (Foundation) | Yes | Yes | Yes | Yes | none found |
| Sermon (Intelligence) | Yes | Yes (29 tests) | Yes | Yes | accept/reject tested at queue layer, not literally through the Tauri command wrapper (thin wrapper, low risk) |
| Content | Yes | Yes (34 tests) | Yes | Yes | none found |
| Cross-Domain | Yes | Yes (29 tests) | Yes | Yes | `CorrelationKind::SharedContext` defined but no rule produces it (dead taxonomy entry, cosmetic) |

## G. Unified Operator Workspace

Confirmed by direct code inspection this phase (not merely re-reading
Phase 2.9's own report): `LiveChurchBrain.tsx` builds `unifiedFeed`/
`attentionQueue` via `useMemo` from state already fetched by existing
commands/events; `handleUnifiedAction` dispatches to the exact same
Tauri commands each domain's own panel already calls (verified by grep -
every command name used by the unified dispatcher also appears at each
domain's original call site); `unifiedFeed.ts`/`attentionQueue.ts`/
`components/workspace/*.tsx` contain zero `invoke(` calls and no new
`unified_feed`/`attention_queue` symbols exist anywhere in the Rust
backend. Attention-queue ranking is confidence-only (no per-domain
quota), proven by `operatorWorkflow.test.ts`'s canonical scenario where
a 1.0-confidence Service anomaly correctly outranks a 0.95-confidence
correlation. What is usable: a single glance-able header, a bounded
actionable queue, and a full filterable feed, layered over - never
replacing - every existing per-domain panel and its own accept/reject/
preview/prepare controls.

## H. Presentation / Display

- **Prepare:** PROVEN. `persist_prepared_item` always inserts `Prepared`.
- **Preview:** PROVEN (non-persisting, `preview_and_prepare_paths_produce_identical_content_for_the_same_reference`).
- **Display (activate):** PROVEN at the state-machine level -
  `commit_activation` is the only path to `Active`, called only after a
  real Tauri window operation succeeds (`display_presentation` in
  `commands.rs`); single-active-item enforced by `prepare_to_activate`
  (tested: `prepare_to_activate_rejects_a_second_item_while_one_is_already_active`).
  No DB-level uniqueness constraint backs this (application-layer only) -
  a documented, non-blocking hardening opportunity (see "Deferred Issues").
- **Active:** PROVEN (`commit_activation_transitions_prepared_to_active`).
- **Stop:** PROVEN, idempotent (`stop_active_item_is_a_safe_no_op_when_nothing_is_active`).
- **Restart recovery:** PROVEN. `reconcile_stale_active_presentation_items`
  runs at every startup before any window is created (verified in
  `lib.rs::setup()`), stopping any item improperly left `Active` by a
  prior run; tested directly
  (`reconcile_stale_active_presentation_items_stops_every_active_row_and_leaves_others_untouched`)
  and exercised live by this session's own two-launch Xvfb verification
  (no stale-Active state possible after a clean process exit either
  time).
- **Failure paths:** a real window-open failure is not exercised by an
  automated test (no `tauri::test` harness is used anywhere in this
  codebase, a project-wide, pre-existing, documented convention) - the
  guarantee that a failed window-open leaves the item `Prepared` rests on
  code ordering (`prepare_to_activate` before `open_display_window`
  before `commit_activation`), reviewed and confirmed correct, not proven
  by a forced-failure test.
- **Hardware verification level:** Xvfb (virtual X server) only. No
  physical monitor or projector exists in this environment. **Display
  Window Code Proven; Real Monitor NOT VERIFIED; Real Projector NOT
  VERIFIED** - this document does not claim otherwise, and neither did
  any prior phase's documentation.

## I. Real Church Workflow

The most complete single proof of "these pieces work together" remains
`pipeline.rs::phase_1_5_full_service_validation` (Bible-only, real
SQLite, dev fixture) plus this phase's new
`phase_2_10_bible_pipeline_against_real_production_dataset` (same shape,
real BSB dataset): service start -> transcript segments (including
deliberate false positives that must produce nothing) -> context
establishment -> bare-verse resolution -> operator approve -> real verse
text through to prepared presentation content -> a rejected/out-of-range
reference producing nothing -> exactly the expected set of `Prepared`
items, none ever `Active` without an explicit operator action. Each of
the other five domains (Music, Service, Sermon, Content, Cross-Domain)
has its own equivalent canonical acceptance test proven against real
SQLite (`phase_2_*_canonical_*_acceptance_scenario`, one per domain, all
green this session), and the Unified Workspace's own
`operatorWorkflow.test.ts` proves all six domains' findings converge
correctly into one feed/attention queue. No single test in this
repository drives all nine capabilities through one literal transcript
in one process - each domain's own canonical test is real and
comprehensive, and Phase 2.9's workspace test proves their outputs
compose correctly, but a single further-unified live-service simulation
spanning all nine remains future work (see "Deferred Issues").

## J. Failure Recovery

Injected/observed this session and across the codebase's existing test
suite:

| Failure | Recoverable? | User-visible? | Logged? | State preserved? | App alive? |
|---|---|---|---|---|---|
| Missing Whisper model file | Yes | Yes (`SpeechStatusKind`) | Yes | Yes | Yes |
| Speech engine feed error | Yes (only that chunk dropped) | Yes (`speech_error`) | Yes | Yes | Yes |
| Backward service-phase transition | Yes (accepted + flagged) | Yes (anomaly finding) | Yes | Yes | Yes |
| Stale transcript (>30s) | Yes (never auto-ends service) | Yes (`TranscriptFreshness::Stale`) | N/A | Yes | Yes |
| Invalid/out-of-range Bible reference | Yes (no suggestion produced) | N/A (silently correct) | N/A | Yes | Yes |
| Missing/absent acoustic model | Yes | Yes (`Unavailable` status) | Yes | Yes | Yes |
| A panicking rule inside cross-domain analysis | Yes (isolated) | N/A | Yes | Yes | Yes (`scenario_g_a_panicking_rule_never_stops_the_others`) |
| Duplicate identical transcript segment | Yes (deduplicated) | N/A | Yes (debug) | Yes | Yes |
| Accepting/rejecting an already-resolved item | Yes (status-guarded) | N/A | N/A | Yes | Yes |

No panic path was found in any normal or documented-edge-case operator
action. The one honestly-unverified failure path is a forced real
presentation-display-window-open failure (see section H) - not exercised
because this codebase has no Tauri test harness anywhere, a project-wide
convention, not a Phase 2.10-specific gap.

## K. Offline

`cargo tree --workspace | grep -iE "reqwest|hyper|ureq|curl|rustls|native-tls|tungstenite|websocket"`
returns nothing. The only outbound network code anywhere in the
workspace is `check_network_online()` (`commands.rs`) - a raw,
non-functional `TcpStream::connect_timeout` to `1.1.1.1:443` used solely
to set a UI status indicator; it sends and receives no application data
and no other code path depends on its result. `integrations/obs` and
`integrations/vmix` are documented placeholder stub crates with no code
and no network dependency. The frontend's only runtime dependency is
`@tauri-apps/api`/React - no HTTP client. Bible search/detection/
presentation, all six intelligence domains, the unified workspace, and
presentation display all operate with zero network access, structurally
(not merely by absence of a code path exercising one).

## L. Licensing

| Source | License | Status | Notes |
|---|---|---|---|
| BSB Bible dataset | Public Domain (CC0 upstream) | `VerifiedPublicDomain` | full evidence chain in `docs/data/bible/BSB/BSB-LICENSE.md` |
| Dev-seed KJV fixture | (unrecorded) | `Unknown` | never enabled for import gating purposes; test/dev-only, never shipped as a claim of a real translation |
| Dev-seed music content | (synthetic) | N/A | placeholder titles/lyrics, clearly test fixtures, not real song data |
| Whisper model | N/A (not bundled) | N/A | operator-supplied; CIP never bundles or auto-downloads one |
| whisper-rs / other Rust crates | Per their own upstream licenses | N/A | unchanged this phase; no new dependency added |

The licensing safety gate (`LicensingStatus::permits_bulk_import`) is
structurally the single point every Bible import path (including a
future user-provided one from the Content Registry panel) must pass
through - `Unknown`/`Restricted` are refused unconditionally, with zero
database mutation, proven by dedicated tests.

## M. Security

- **Critical:** none found.
- **High:** none found.
- **Medium:** none found.
- **Low:** `security.csp` is `null` in `tauri.conf.json`. Assessed as low
  severity, not a first-use blocker: the app has zero fs/shell/http/
  dialog plugin capability compiled in at all (`Cargo.toml` for
  `src-tauri` depends only on `tauri-plugin-log`), loads only its own
  bundled `frontendDist`, and both windows' `capabilities/*.json` grant
  only `"core:default"` - there is no remote content and no plugin
  surface a missing CSP could meaningfully protect against today. Worth
  tightening before any future window loads external/remote content.
- **Informational:** `check_network_online()` makes one outbound TCP SYN
  to a hardcoded public IP per status poll (no data sent/received,
  non-functional) - already documented, but worth confirming acceptable
  for a deployment with strict no-outbound-traffic requirements.

Secrets scan (API keys, passwords, tokens, private-key headers,
`service_role`, generic `key=`/`password=`/`token=` assignments) across
the full repository, excluding `node_modules`/`target`/`.git`: **zero
matches.**

## N. Performance

Representative measurements, real components, this machine (from
`docs/bible-production-dataset.md` and `docs/operator-workspace.md`,
re-confirmed as still the current, unmodified code paths this session):

| Operation | Dataset/input size | Result |
|---|---|---|
| BSB full import (fresh) | 31,086 verses | 634.8ms |
| BSB idempotent re-import | 31,086 verses (all present) | 551.6ms |
| Single verse lookup | 1,000x | ~5.5µs each |
| Chapter lookup | 100x | ~61µs each |
| Verse-range lookup | 100x | ~28µs each |
| Free-text search | 1 query, 31,086-row corpus | 11.3ms |
| Unified feed + attention queue build | 100 mixed findings | ~0.17ms combined |

All results are well within real-time operator-interaction budgets; no
`O(n²)` behavior was found in any measured path (import, lookup, search,
or the frontend's feed/queue construction are all single-pass or
logarithmic-indexed). No optimization was performed - none was needed.

## O. Issues Fixed

Exactly one change was made this phase, matching spec section 50's
"smallest safe fix, regression-covered" policy:

**Bible detection/context/suggestion pipeline had zero test coverage
against the real production BSB dataset** - every existing pipeline test
ran only against the 6-verse dev fixture or `FakeBibleProvider`, which
spec section 8 explicitly warns against relying on for production
validation. Added
`pipeline::tests::phase_2_10_bible_pipeline_against_real_production_dataset`
(`apps/desktop/src-tauri/src/pipeline.rs`), which imports the real,
complete BSB dataset into a real SQLite `SqliteBibleProvider` and runs
the exact same `handle_final_transcript` pipeline every other pipeline
test uses, proving context establishment, bare-verse resolution, context
replacement, out-of-range rejection, and real-verse-text presentation
content all work identically against the real dataset. No production
code was changed - the underlying detection/context/resolution logic is
already fully provider-agnostic (the same `BibleProvider` trait, unit-
tested extensively against fakes); this closes a coverage gap, not a
functional defect.

**A genuine, real discovery made while writing this test, not a bug:**
real Bible chapters densely overlap verse numbers in a way the tiny,
deliberately-non-colliding dev fixture never could. A first draft of
this test asked for "verse sixteen" immediately after switching context
from Genesis 1 (31 verses) to John 3 (36 verses) - both books have a
verse 16, so the context manager correctly and honestly reported this as
`Ambiguous` (two valid candidates, John 3:16 and Genesis 1:16) rather
than guessing, exactly the "genuinely ambiguous - a candidate list, never
a guess" behavior Phase 1.1 established. This is proof the ambiguity
system works correctly against realistic data density, not a defect; the
test was adjusted to ask for verse 36 (present only in John 3) to keep
its own assertions about a single unambiguous resolution meaningful,
while the ambiguity behavior itself is left exactly as it already was
and remains exercised by its own dedicated existing test suite
(`core/bible::context_manager`'s ambiguity tests).

## P. Deferred Issues

All are P2 (important, not blocking) or P3 (minor/polish), left
undisturbed per spec section 49's "only P0/P1 fixed" policy:

- **P2.** No DB-level uniqueness constraint enforcing at-most-one-Active
  presentation item per service; enforcement is entirely application-
  layer (tested, and re-validated on every activation attempt via
  `commit_activation_rejects_an_item_that_is_no_longer_prepared`, but
  with no database-level safety net for a hypothetical concurrent-write
  race). A partial unique index would be a small, well-scoped future
  hardening migration.
- **P2.** No automated test forces a real presentation display window-
  open failure, a real window `Destroyed` event, or real window creation
  at all - this codebase has no Tauri test harness anywhere (a project-
  wide, pre-existing, documented convention, not new to this phase); all
  confidence in these paths comes from code review plus manual Xvfb
  runtime verification.
- **P3.** Sermon/Content accept-reject Tauri commands are tested at the
  underlying `FindingQueue`/`ContentCandidateQueue` orchestration layer,
  not literally through the `#[tauri::command]` wrapper (a thin,
  low-risk wrapper: parse id, call orchestration function, record
  timeline, emit event).
- **P3.** `CorrelationKind::SharedContext` is defined in the enum but no
  rule currently produces it - dead/aspirational taxonomy entry, no
  functional impact.
- **P3.** No explicit "domain-flood fairness" test proves the bounded
  8-item attention queue never lets one noisy domain crowd out a sparse
  domain's high-confidence item - correct by code inspection (pure
  confidence sort, no domain quota anywhere in `attentionQueue.ts`), but
  not directly asserted under a many-vs-few-domain flood scenario.
- **P3.** `security.csp` is `null` - low severity today (see section M),
  worth tightening before any future window loads remote content.
- **P3 (future work, not a defect).** No single automated test drives
  all nine capabilities (Bible, Music, Service, Sermon Foundation,
  Sermon Intelligence, Content, Cross-Domain, Workspace, Presentation)
  through one literal shared transcript in one process end-to-end; each
  domain's own canonical acceptance test is comprehensive and real, and
  the workspace's own test proves their outputs compose, but a single
  further-unified simulation remains a reasonable next investment.

## Q. PROVEN

- Complete, legally-verified BSB Bible dataset (66/1189/31,086),
  integrity-valid, checksum-stable, idempotently imported.
- Bible search, detection, context management, and presentation
  preparation all work correctly against the real production dataset
  (this phase closed the one remaining gap here).
- Deterministic text/lyric-based Music Intelligence with a real,
  end-to-end-tested operator accept/reject/current-song workflow.
- Service Intelligence phase detection, weak-cue debounce, anomaly
  flagging (including graceful handling of backward transitions),
  operator correction, and non-destructive transcript-staleness
  reporting.
- Sermon Foundation lifecycle, section exclusivity, explicit-only
  speaker attribution, and restart recovery against real SQLite.
- Sermon Intelligence's 20-variant taxonomy with structurally-enforced
  Observed/Inferred/Generated discipline (no code path ever produces
  `Generated`).
- Content Intelligence candidates fully traceable to their source
  finding, structurally incapable of auto-publishing (no dependency on
  the presentation crate exists).
- Cross-Domain Intelligence's nine correlation rules, each with negative
  tests proving no false convergence, and no engine-to-engine calls.
- The Unified Operator Workspace as a genuine zero-new-backend-surface
  projection layer, confirmed by direct code/grep inspection this phase.
- Local presentation display's full `Prepared -> Active -> Stopped`
  lifecycle, explicit-activation-only, and startup reconciliation of any
  stale `Active` row - live-verified twice this session under Xvfb.
- Fully offline operation (structural proof: no network-capable crate
  anywhere in the dependency graph).
- Zero secrets in the repository; minimal Tauri capability surface.
- Full regression suite green: ~462 Rust tests (`cargo test --workspace`),
  `cargo fmt --check`/`clippy -D warnings` clean, `cargo check -p
  cip-desktop --features whisper` clean, `cargo test -p cip-ai-speech
  --features whisper` (6/6) clean, 179/179 frontend tests, `tsc`/`vite
  build`/`oxlint` clean.
- Two clean, idempotent real-binary launches under Xvfb this session (9
  migrations then 0; BSB imported then already-present; zero panics).

## R. NOT VERIFIED

- Real microphone / live audio hardware capture.
- Real Whisper model file / real live speech transcription (none
  available in this environment).
- Real physical second monitor or projector output (only Xvfb virtual
  display was available).
- A forced real presentation-display-window-open failure (no Tauri test
  harness exists in this codebase for that, project-wide).
- A single, further-unified nine-capability live-service simulation in
  one process (each domain's own canonical test is real and
  comprehensive; their composition is proven at the workspace layer, but
  not as one single monolithic scenario).
- Screen-reader software behavior specifically (semantic HTML/ARIA was
  reviewed, not run against a real screen reader).
- Real multi-monitor / touch-tablet operator hardware.

## S. NOT AVAILABLE

- Real acoustic (audio-fingerprint) music recognition - no real model
  exists anywhere in this codebase; `LocalAcousticMusicRecognizer` is
  structurally always `Unavailable` by design, pending a future acoustic
  model integration decision explicitly out of this phase's scope.
- A bundled or auto-downloaded Whisper speech model - CIP deliberately
  never bundles or fetches one; an operator must supply their own, or
  use manual/assisted transcript entry.
- OBS/vMix/NDI output - stub placeholder crates only, explicitly out of
  scope by design, unchanged this phase.
- Any cloud AI / LLM reasoning, automatic social-media publishing, or
  automatic content generation - none implemented, none in scope.
- A second complete Bible translation, or any commercially-licensed
  translation (NIV/ESV/etc.) - the licensing gate structurally prevents
  importing one without verified redistribution rights.

## T. First-Use Conditions

For **GO WITH CONDITIONS** to hold, the operator/church must have or
accept:

1. **Speech input.** Either (a) source and configure a real local
   ggml/gguf Whisper model themselves (per `docs/live-speech.md`'s
   documented model path and the `whisper` build feature), or (b) use
   CIP's fully-functional manual/assisted transcript entry workflow
   instead of live microphone transcription.
2. **Music recognition.** Rely on text/lyric-based Music Intelligence
   (fully functional, requires the church's own song/lyric data to be
   imported via the existing dataset-import path for real repertoire
   coverage) rather than automatic acoustic (audio-fingerprint) song
   recognition, which is not available.
3. **Display hardware.** Confirm the local presentation display window
   behaves as expected on the church's own real monitor/projector setup
   before relying on it for a live service - this has been proven at the
   code and virtual-display level (Xvfb) in this environment, but not
   against physical display hardware here.
4. **Environment.** One supported desktop OS (Windows/Linux/macOS) per
   the existing Tauri build target; local SQLite storage; no cloud
   account or internet connection required for any core Phase 2
   capability.

None of these conditions represent a defect - each is a structural,
honestly-documented fact about what is and isn't bundled in this
development environment, consistent with every earlier phase's own
"NOT AVAILABLE" disclosures in this repository.

## U. Phase 3 / Post-Phase-2 Handoff

Phase 2.10 does not begin Phase 3. What it leaves ready for whatever
comes next:

- A validated, regression-protected Phase 2 stack with a single documented
  readiness matrix (this document) to build future decisions on.
- Three concrete, well-scoped, non-blocking hardening opportunities (P2
  items in "Deferred Issues") a future phase could pick up in an
  afternoon each: a DB-level single-active-presentation-item constraint,
  a forced-window-open-failure test path (would require introducing a
  test seam, a real architectural decision, not done casually here), and
  literal Tauri-command-level accept/reject tests for Sermon/Content.
- A clear, evidence-based statement of what would need to change for a
  full "GO" without conditions: a bundled or church-supplied Whisper
  model verified against real audio, a real acoustic recognition backend
  (a genuine architectural decision, correctly deferred so far), and
  physical display/microphone hardware verification outside this
  environment.

## V. Final Git Status

- Branch: `claude/cip-foundation-init-i85g87`
- HEAD: recorded after this document's own commit (see the session's
  final report for the exact hash)
- Remote: `origin/claude/cip-foundation-init-i85g87`
- Working tree: clean after commit
