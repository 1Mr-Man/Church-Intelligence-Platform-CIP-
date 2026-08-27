# Phase 3.1 — Real Church Pilot & Hardware Validation

This document is the engineering record for Phase 3.1. Its question is
narrower and more practical than any prior phase's: **not** "does the code
pass its tests," but "can a real church install CIP, connect real
hardware, run a real service, recover from a real failure, and operate it
without a developer in the room?"

The honest answer this phase reaches is: **the software is pilot-ready;
the hardware claims are not verifiable in this environment, and this
document says so explicitly rather than fabricating them.** See section P
(Hardware Results) before anything else.

## A. Git Baseline

Phase 3.1 began at `9bd2ba8` ("Harden CIP for first use," Phase 3.0's
commit), tree clean, `HEAD` in sync with `origin/claude/cip-foundation-init-i85g87`.
No commits were skipped or rewritten. Every change this phase made is
additive: 5 files touched, 577 insertions, **0 deletions** — no existing
production code path, Tauri command, database schema, frontend component,
or architectural boundary was altered. The full list:

| File | Change |
|---|---|
| `apps/desktop/src-tauri/src/pipeline.rs` | +1 new test: the Phase 3.1 full-service simulation |
| `ai/speech/src/whisper.rs` | +1 new test: corrupt-model-file handling |
| `database/src/connection.rs` | +1 new test: unopenable database path handling |
| `apps/desktop/src-tauri/src/content.rs` | +1 new test: post-import dataset corruption detection |
| `core/intelligence/src/registry.rs` | +1 new test double + test: Service-domain engine panic isolation |

Every existing test, command, schema, and UI component from Phase 3.0
is byte-for-byte unchanged.

## B. Executive Verdict

**PILOT READY — CONDITIONAL.**

Recommendation: **GO WITH CONDITIONS.** Every software capability CIP
ships is proven end-to-end against real data, real SQLite, and a real
packaged binary. The three hardware-dependent capabilities (live
microphone capture, real Whisper transcription, physical projector/
monitor output) are **NOT AVAILABLE** in this development container —
not "untested," genuinely absent (no `/dev/snd`, no `$DISPLAY`, no
`xrandr`) — and therefore cannot be upgraded to PROVEN under any
circumstance here. A pilot church must verify those three items on its
own hardware before its first live service; CIP's manual-transcript
fallback is a fully-supported, already-proven substitute for the
microphone/Whisper path if that verification hasn't happened yet.

This is a stronger, narrower claim than Phase 2.10's "GO WITH
CONDITIONS": that phase's conditions were about configuration steps an
operator needed to take. This phase's conditions are about physical
hardware this environment cannot possess.

## C. Initial Audit & Gap Register

A 4-agent, read-only audit (packaging feasibility, failure-injection
coverage against a 24-item matrix, bounded-state/memory-growth risk, and
full-service-simulation feasibility) plus direct hardware probing produced
this gap register. Severity: P0 = blocks pilot, P1 = should fix before
pilot, P2 = should fix eventually, P3 = cosmetic/documentation only.

| # | Gap | Severity | Disposition |
|---|---|---|---|
| 1 | No single test proves all 9 domains chained through one transcript against real SQLite | P1 | **Fixed** — new `phase_3_1_pilot_full_service_simulation` (section E) |
| 2 | Corrupt Whisper model file path untested | P1 | **Fixed** — new test, real whisper.cpp rejection proven (section F) |
| 3 | Dataset corruption *after* a successful import untested | P1 | **Fixed** — new test, real `SqliteBibleProvider` + integrity checker (section F) |
| 4 | Unopenable database path (disk/permission failure) untested | P1 | **Fixed** — new test (section F) |
| 5 | Service-domain engine panic isolation untested (Music/Sermon were, Service wasn't) | P2 | **Fixed** — cheap, closes a documentation gap (section F) |
| 6 | No real, installable package had ever been built in this environment | P1 | **Fixed** — real `.deb` built, verified, launched (section G) |
| 7 | Audio capture *startup* failure (device busy/permission denied) untested | P2 | **Deferred** — no real/mockable cpal failure path reachable without hardware; documented, not fabricated |
| 8 | Content candidate *creation* failure untested | P3 | **Deferred** — construction is infallible in-memory logic; not a reachable failure class |
| 9 | Presentation window creation/close failure untested | P3 (pre-existing, unchanged) | **Deferred** — this codebase has no `tauri::test` harness anywhere, a documented project-wide convention since Phase 2.10; unchanged this phase |
| 10 | Several intelligence queues (`FindingQueue`, `ContentCandidateQueue`, `CorrelationQueue`) are unbounded `Vec<T>` | P3 | **Deferred** — no realistic pilot-scale (1–3 hour service) risk; see section M |
| 11 | Microphone / real Whisper / physical display cannot be verified here | N/A | **Not a defect** — hardware absence, honestly reported (section P) |

No P0 was found. No architectural change was made or needed; Rule 1 of
the governing spec ("preserve existing architecture unless a concrete
Phase 3.1 defect proves otherwise") was honored because no such defect
was found.

## D. Pilot Readiness Matrix

| Capability | Implemented | Tested (real data) | Runtime Verified | Hardware Verified | Offline | Operator Usable Without a Developer | Blocker? |
|---|---|---|---|---|---|---|---|
| Install (`.deb`) | Yes | N/A | Yes (Xvfb, from the installed path) | N/A | Yes | Yes | No |
| First launch / DB init | Yes | Yes | Yes (Xvfb, RUN1) | N/A | Yes | Yes | No |
| Idempotent relaunch | Yes | Yes | Yes (Xvfb, RUN2) | N/A | Yes | Yes | No |
| BSB Bible dataset | Yes | Yes (real 66/1189/31,086) | Yes | N/A | Yes | Yes | No |
| Manual transcript entry | Yes | Yes | Yes | N/A | Yes | Yes | No |
| Microphone capture | Yes (code) | Yes (enumeration/error paths) | N/A | **NOT AVAILABLE** | N/A | Conditional | See P |
| Whisper transcription | Yes (code, opt-in feature) | Yes (missing/corrupt-model paths, real whisper.cpp) | N/A | **NOT AVAILABLE** | Yes | Conditional | See P |
| Six intelligence domains | Yes | Yes (chained, real SQLite, this phase) | Yes | N/A | Yes | Yes | No |
| Sermon foundation lifecycle | Yes | Yes (chained with taxonomy, this phase) | Yes | N/A | Yes | Yes | No |
| Content Intelligence | Yes | Yes (chained, this phase) | Yes | N/A | Yes | Yes | No |
| Cross-Domain correlation | Yes | Yes (chained, this phase) | Yes | N/A | Yes | Yes | No |
| Presentation activation | Yes | Yes (chained, this phase) | Yes (Xvfb, software window) | **NOT AVAILABLE** (no physical display) | Yes | Conditional | See P |
| Restart/crash recovery | Yes | Yes (real file-backed DB, this phase) | Yes | N/A | Yes | Yes | No |
| Licensing gate | Yes | Yes (re-verified) | Yes | N/A | Yes | Yes | No |
| Offline operation | Yes | Yes (re-verified) | N/A | N/A | Yes | Yes | No |
| Logging | Yes | Yes (Xvfb log output) | Yes | N/A | Yes | Yes | No |

Every row without a hardware dependency is unconditionally READY. Every
row with one is READY, CONDITIONAL ON REAL-HARDWARE VERIFICATION AT THE
CHURCH — never silently upgraded to unconditional.

## E. Full-Service Simulation

New test: `pipeline::tests::phase_3_1_pilot_full_service_simulation`
(`apps/desktop/src-tauri/src/pipeline.rs`). This is the single canonical
proof the spec asked for — one fictional service, chained through every
domain CIP ships, using only real production orchestration functions
(the same ones `commands.rs` calls), a real SQLite database, and no
test-only shortcuts:

1. **Service start** (`ServiceSession::start` + `persist_service`).
2. **Sermon foundation lifecycle**: sermon started with a title, a Main
   Message section opened, a speaker assigned — all persisted.
3. **Bible reference, Suggestion path**: `handle_final_transcript`
   (the real operator-facing pipeline) detects Romans 8:28, the
   suggestion is approved, and a presentation item is prepared.
4. **Bible reference, Finding path**: the real `BibleIntelligenceEngine`
   (the same engine `commands::analyze_bible_transcript` calls) produces
   a Bible-domain finding for the same reference, proving the two
   parallel Bible pathways this architecture has always had both work
   from one transcript.
5. **Sermon semantic taxonomy**: the same nine-segment scripted
   walkthrough `sermon_adapter.rs`'s own canonical test uses, run through
   the real `sermon::analyze_and_queue`, with every transcript segment
   persisted and linked via a real `SermonSegment` row. Main Point,
   Story/Illustration, Application, Takeaway, Food for Thought, and a
   Scripture cross-link are all asserted present.
6. **Music**: a real dev-seed hymnbook exact-title match via the real
   `music::analyze_and_queue`.
7. **Content Intelligence**: the real `content_intelligence::analyze_and_queue`
   reading the sermon taxonomy findings, producing at least one candidate.
8. **Cross-Domain correlation**: the real `cross_domain::analyze_and_queue`
   correctly correlates the Bible finding and the sermon's Scripture
   cross-link on their shared Romans 8:28 reference.
9. **Presentation activation**: the real `Prepared -> Active` transition,
   left deliberately Active to simulate "app closed mid-service."
10. **Simulated restart**: the database connection is dropped and the
    same file reopened exactly as a fresh launch would. The stale Active
    item is reconciled to Stopped; the sermon, its title, all nine
    segment links, and every transcript segment survive intact.

This closes the one real structural gap Phase 2's validation history had
already named (`docs/phase-2-validation.md` section I: "no single test
proves the full nine-domain workflow against real SQLite").

## F. Failure-Injection Matrix

24 scenarios audited against existing test coverage; results and new
tests below. "FOUND" means a pre-existing test already proved it; "NEW"
means this phase added one; "DEFERRED" means a genuine, documented gap
with no fix (see section C for why each is not a pilot blocker).

| # | Scenario | Status |
|---|---|---|
| 1 | No microphone (empty device list) | FOUND |
| 2 | Invalid microphone selection | FOUND |
| 3 | Audio capture startup failure | DEFERRED (P2, no hardware to reach it) |
| 4 | Speech model missing | FOUND |
| 5 | Speech model invalid/corrupt | **NEW** — `whisper::tests::corrupt_model_file_is_reported_as_transcription_failed_not_a_panic` |
| 6 | Speech engine unavailable generally | FOUND |
| 7 | Empty transcript submitted | FOUND |
| 8 | Malformed transcript | FOUND |
| 9 | Bible lookup failure (reference not found) | FOUND |
| 10 | Bible dataset unavailable/corrupt at startup | **NEW** — `content::tests::a_dataset_corrupted_after_import_is_detected_by_the_real_startup_integrity_check` |
| 11 | Music engine failure (isolated) | FOUND |
| 12 | Sermon engine failure (isolated) | FOUND |
| 13 | Service engine failure (isolated) | **NEW** — `registry::tests::a_panicking_service_engine_is_isolated_and_never_propagates` |
| 14 | Content candidate creation failure | DEFERRED (P3, not a reachable failure class) |
| 15 | Cross-domain correlation failure (panicking rule) | FOUND |
| 16 | Presentation preparation failure (invalid reference) | FOUND |
| 17 | Presentation window creation failure (real Tauri) | DEFERRED (pre-existing, project-wide: no `tauri::test` harness) |
| 18 | Display window closes unexpectedly | DEFERRED (same as #17 for the window-wiring layer; the state-transition logic it calls is tested) |
| 19 | Database unavailable/connection failure | **NEW** — `connection::tests::an_unopenable_database_path_is_reported_as_a_connection_error_not_a_panic` |
| 20 | Service restart (pause/resume) | FOUND |
| 21 | Application restart (stale state reconciliation) | FOUND (and re-proven end-to-end this phase, section E) |
| 22 | Stale Active presentation item after restart | FOUND (and re-proven end-to-end this phase, section E) |
| 23 | Invalid operator action | FOUND |
| 24 | Duplicate operator action (double-click safety) | FOUND |

20 of 24 scenarios have real automated coverage (16 pre-existing + 4 new
this phase); 4 are honestly documented as deferred, each with a concrete
reason none of which is "we didn't think of it."

## G. Packaging & Installation

A real Linux `.deb` was built this phase (`npx tauri build -b deb`,
release profile, no `[profile.release]` overrides anywhere in the
workspace): `target/release/bundle/deb/Church Intelligence Platform_0.1.0_amd64.deb`,
7.2 MB, verified with `dpkg-deb -I`/`-c`:

- Correct metadata: package `church-intelligence-platform`, version
  `0.1.0`, depends on `libwebkit2gtk-4.1-0` and `libgtk-3-0` only.
- Correct contents: `usr/bin/cip-desktop` (22 MB stripped binary),
  desktop entry, and three icon sizes — no stray files.
- **Extracted and launched from the installed path** (`dpkg-deb -x`,
  then run directly from `usr/bin/cip-desktop`, no `git`/`cargo`/`npm`/
  source tree involved) under Xvfb: clean startup, real BSB import,
  zero panics — the closest honest proof of "PACKAGING: PASS" this
  environment allows.

**RPM and AppImage were not buildable here** — `rpmbuild` is absent, and
`appimagetool`/`linuxdeploy` are normally auto-downloaded by `tauri
build` and this environment has no egress to fetch them. **Windows and
macOS installers are categorically impossible to build from this Linux
container.** These are environmental limitations of this session, not
defects in the app or its `tauri.conf.json` (which specifies
`"targets": "all"` and requests every platform equally) — Deb build
success is real signal that packaging works; the other targets are
simply NOT VERIFIED here.

## H. BSB Dataset Re-Validation

Unmodified and re-confirmed this phase (via the pre-existing
`content::tests::phase_real_bible_dataset_full_validation` plus two live
Xvfb launches): 66 books, 1,189 chapters, 31,086 verses,
`licensing_status: verified_public_domain`, `IntegrityStatus::Valid`.
The new corruption-detection test (section F, #10) additionally proves
that *if* the stored dataset were ever corrupted after import, the same
integrity check the real startup path runs would catch it — not just the
happy path.

## I. Licensing Gate

Re-verified via the existing, unmodified, still-passing negative tests:
`refuses_import_when_licensing_status_is_unrecognized_text`,
`refuses_import_when_licensing_status_is_unknown_and_writes_nothing`,
`refuses_import_when_licensing_status_is_restricted_and_writes_nothing`,
and the positive `permits_import_for_every_evidence_backed_licensing_status`.
No licensing-path code was touched this phase.

## J. Offline Re-Verification

`cargo tree --workspace --all-features` re-run this phase: the only
`http`-named crate in the graph is the `http` type-definitions crate
(pulled in transitively by Tauri's webview/IPC plumbing), not a network
client — no `reqwest`, `hyper` client, `ureq`, or `curl` binding anywhere
in the dependency tree. Unchanged from Phase 2.10/3.0's own finding.

## K. Security

No security-relevant file was touched this phase (no `tauri.conf.json`,
capability, or permission change appears in the diff — see section A).
The one still-open item from Phase 2.10/3.0 remains open and is
re-confirmed, not silently dropped: **CSP is `null`** in
`tauri.conf.json`. This was evaluated again this phase and deferred
again, for the same reason as before — the app has zero `fs`/`shell`/
`http`/`dialog` plugin surface exposed to the webview, so the practical
exposure from a null CSP is low, but "low" is not "zero," and setting an
explicit CSP is real, non-trivial work this phase's scope (real-pilot
behavioral validation, not a security hardening pass) did not include.
Recommended for Phase 3.2 (section Z).

## L. Performance

Release-mode measurements (throwaway probes, not a formal benchmark
harness, per this project's established convention):

- The entire `pipeline` test module — 17 tests, including the new
  9-domain full-service simulation (three real SQLite databases: main
  app DB, Bible provider, restart-reopened DB; a 66-book/1,189-chapter/
  31,086-verse real dataset import inside one of those tests) — completes
  in **2.56 seconds** in release mode.
- The `.deb`-extracted binary reaches a fully-initialized, ready-to-use
  state (9 migrations, full BSB import, all six intelligence domains
  initialized) in **under 2 seconds** from process start, observed twice
  under Xvfb.

Both are comfortably within the real-time-interactive budget every prior
phase's performance sections have used as the bar.

## M. Memory / Bounded-State Audit

A dedicated audit confirmed several in-memory collections remain
structurally unbounded `Vec<T>` with no `MAX_*`/eviction policy:
`FindingQueue`, `ContentCandidateQueue`, and `CorrelationQueue`
(`core/intelligence`), plus a handful of dedup-only (not truncating)
frontend arrays (`suggestions`, `musicFindings`, `sermonFindings`,
`crossDomainCorrelations`, `contentCandidates`, `serviceTransitions`,
`serviceAnomalies` in `LiveChurchBrain.tsx`). This is a real gap against
this codebase's own stated principle (`IntelligenceContext`'s
`DEFAULT_MAX_RECENT_FINDINGS` and the frontend's `MAX_VISIBLE_*`
constants are the established "never unbounded" pattern others don't
fully follow) — but at realistic pilot data volumes (a 1–3 hour service
producing on the order of hundreds, not tens of thousands, of findings)
none of these pose a genuine memory or performance risk. Documented here
as a deferred P3 item (section U), not fixed, consistent with this
session's "only change what a concrete defect requires" discipline —
fixing it would be architecture expansion the spec explicitly warned
against absent proof of a real problem.

## N. Crash / Restart Recovery

Proven twice this phase: once structurally (the full-service simulation,
section E, with a real file-backed SQLite database dropped and reopened
mid-Active-presentation-item), and once at the process level (two clean
Xvfb launches of the release binary against the same data directory,
RUN1 fresh / RUN2 idempotent, section G). No panic, no crash, no stale
"on screen" state survives a restart in either case.

## O. Logging

No logging-message code was changed this phase. Live Xvfb output was
reviewed for actionability and found already clear: every startup line
names what happened and its concrete outcome ("9 migration(s) applied,"
"31086 imported, 0 already present," "acoustic recognizer status:
Unavailable (configured model directory does not exist: `<path>`)"),
consistent with Phase 3.0's hardening of exactly this kind of message.
No gap found; none introduced.

## P. Hardware Results

This is the section the spec singled out as most important. Every claim
below is exactly what could be verified in this container, stated
without euphemism:

| Capability | Status | Why |
|---|---|---|
| Real microphone enumeration/selection | PROVEN | Code paths tested against a real, honestly-empty device list and a real "unknown device" rejection (`integrations/audio`) |
| Real microphone audio capture | **NOT AVAILABLE** | This container has no `/dev/snd` — no audio hardware exists to attempt capture against, not merely "untested" |
| Whisper model loading (missing/corrupt file) | PROVEN | Both paths tested against the real whisper.cpp binding this phase (section F, #5) |
| Whisper live transcription of real speech | **NOT AVAILABLE** | Requires both a real model file (not bundled, by design — see `docs/live-speech.md`) and real microphone audio, neither obtainable here |
| Presentation window state machine (Prepared/Active/Stopped) | PROVEN | Real SQLite, real orchestration functions, this phase and prior phases |
| Presentation window rendering under a virtual display | PROVEN (Xvfb only) | Two clean launches this phase, zero panics, log-verified full startup sequence |
| Presentation output on a physical monitor/projector | **NOT AVAILABLE** | No `$DISPLAY`, no `xrandr`, no physical display hardware in this container |

**No claim in this table, or anywhere else in this document, has been
upgraded from NOT AVAILABLE to VERIFIED or PROVEN.** A pilot church must
independently confirm the three NOT AVAILABLE rows on its own hardware
before its first live service. This is exactly what makes GO WITH
CONDITIONS the correct recommendation rather than an unconditional GO.

## Q. Out-of-Scope Confirmation

No out-of-scope feature (cloud AI, OBS/vMix/NDI output, a mobile app, a
SaaS/multi-tenant mode, biometric speaker recognition, or a real
acoustic-fingerprint music recognition model) was added, started, or
modified this phase. `integrations/obs`, `integrations/vmix`, and
`integrations/web` remain empty stub crates exactly as before (0 tests,
0 lines of logic — confirmed by this phase's own regression run, section
R). No compatibility fix was needed for any of them.

## R. Regression Summary

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo clippy -p cip-ai-speech --all-targets --features whisper -- -D warnings`: clean.
- `cargo test --workspace` (default features): **753 passing, 0 failing.**
- `cargo test -p cip-ai-speech --features whisper`: **7 passing, 0 failing.**
- `cargo test -p cip-desktop --features whisper`: **192 passing, 0 failing.**
- `cargo tree --workspace --all-features`: no network-capable crate (section J).
- Frontend `tsc -b` (typecheck): clean.
- Frontend `oxlint`: 0 errors, 2 pre-existing warnings unrelated to this
  phase's changes (both frontend-only, both predate this session).
- Frontend `vitest run`: **179 passing, 0 failing.**
- Frontend `vite build`: succeeds, produces a working production bundle.

Every existing test from Phase 3.0 and earlier still passes unmodified —
zero regressions.

## S. Xvfb Runtime Verification

Three real-binary launches under a virtual X server this phase, each
reviewed for a clean startup sequence and zero panics:

1. **RUN1 (fresh, source-built release binary)**: fresh `$HOME`, 9
   migrations applied, real BSB dataset imported (31,086 verses, 0
   already present), all engines initialized, whisper/acoustic correctly
   report unavailable with actionable reasons.
2. **RUN2 (idempotent, same data directory)**: 0 migrations applied,
   BSB already present (0 imported, 31,086 already present) — proves
   idempotency, not just a fresh-install happy path.
3. **RUN3 (the `.deb`-extracted, installed-path binary)**: identical
   clean startup sequence, run from `usr/bin/cip-desktop` with no
   source tree, `git`, `cargo`, or `npm` involved — the strongest
   available proof of "an operator, not a developer, can run this."

This is explicitly **software window validation under a virtual
display**, never conflated with physical display validation — see
section P.

## T. Issues Fixed

No production defect was found this phase (section C: no P0). What was
"fixed" is test-coverage debt, not application behavior:

- 4 genuine failure-injection gaps closed with new tests (section F).
- 1 genuine "no single end-to-end proof" gap closed with the new
  full-service simulation (section E).
- 1 real, installable Linux package produced and launched for the first
  time in this environment (section G).

## U. Deferred Issues

| Item | Severity | Why deferred |
|---|---|---|
| Audio capture startup failure untested | P2 | No real or mockable cpal failure path is reachable without audio hardware, absent here |
| Content candidate creation failure untested | P3 | Construction is infallible in-memory logic; not a real failure class |
| Presentation window creation/close failure untested at the Tauri layer | P3 | Pre-existing, project-wide: no `tauri::test` harness anywhere in this codebase |
| Several intelligence queues are unbounded `Vec<T>` | P3 | No realistic risk at pilot-service data volumes (section M) |
| CSP is `null` in `tauri.conf.json` | P2 | Zero fs/shell/http/dialog plugin surface limits real exposure; real fix is out of this phase's scope |
| RPM/AppImage/Windows/macOS installers not built | N/A (environmental) | Missing tooling / no egress / wrong host OS — not a code defect |

## V. PROVEN

Every capability in the Pilot Readiness Matrix (section D) marked fully
READY, plus:

- The complete nine-domain, one-transcript, real-SQLite workflow
  (section E) — the one gap `docs/phase-2-validation.md` had explicitly
  and honestly left open since Phase 2.
- A real, installable `.deb` package that launches cleanly from its
  installed path with no developer tooling present (section G).
- Crash/restart recovery at both the structural and process level
  (section N).
- 20 of 24 failure-injection scenarios (section F).

## W. NOT VERIFIED

- RPM, AppImage, Windows, and macOS installers (tooling/environment
  limitations of this session, not code defects — section G).
- A forced real Tauri presentation-window-open failure (no test harness
  exists anywhere in this codebase for that — pre-existing, unchanged).

## X. NOT AVAILABLE

- Real microphone audio capture (no `/dev/snd` in this container).
- Real Whisper transcription of real speech (needs both a model file
  and real microphone audio; neither is obtainable here).
- Physical monitor/projector output (no `$DISPLAY`, no `xrandr`, no
  physical display hardware).

These three are the hard ceiling on this phase's verdict (section B).

## Y. Pilot Conditions

Before a church relies on CIP for a live service, in addition to
Phase 3.0's existing first-use conditions (see `docs/first-use.md`):

1. **Confirm microphone capture** on the church's own hardware, or plan
   to use manual transcript entry (fully supported, proven, not a
   degraded fallback).
2. **Confirm Whisper transcription**, if used, with a real model file
   the church has sourced themselves (CIP never bundles or downloads
   one, by design) and real audio — this has only ever been proven at
   the missing-file and corrupt-file error-handling level here, never
   against real transcription.
3. **Confirm the presentation window on the church's actual monitor or
   projector** before the first live service — this has only been
   proven under a virtual display here.
4. Install via the platform's real installer once one exists for that
   platform (only `.deb` was built this phase); until then, a developer
   or technically comfortable operator can still build from source
   following `docs/development.md`.

## Z. Phase 3.2 Handoff

Recommended next work, not started automatically per this phase's own
governing instructions:

1. **CSP hardening** — replace `null` with an explicit, minimal policy
   now that this phase re-confirmed the plugin surface is small enough
   to make that tractable.
2. **RPM/AppImage packaging**, and Windows/macOS builds, once a CI
   environment with the right tooling/egress/host OS is available.
3. **A real pilot church engagement** — the one verification step no
   amount of further engineering in this environment can substitute
   for: real microphone, real Whisper model, real projector, real
   operator, real service.
4. Consider bounding `FindingQueue`/`ContentCandidateQueue`/
   `CorrelationQueue` (section M) if real pilot usage ever approaches
   the volumes where it would matter — not before.

## AA. Final Git Status

One commit, "Validate Phase 3.1 real church pilot readiness," pushed to
`claude/cip-foundation-init-i85g87`. Tree clean afterward; branch in
sync with `origin`. No force push.
