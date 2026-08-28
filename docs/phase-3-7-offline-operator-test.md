# Phase 3.7 — Full Offline Operator Test Mode & System Verification

Starting HEAD: `45f23b4` (Phase 3.6, "Church Knowledge Libraries & Service
History"). Branch: `claude/cip-foundation-init-i85g87`.

**Mission**: prove CIP is a *testable offline church-operator application*,
not just "software that is technically installed." A church operator must
be able to sit at a normal Windows laptop with no Internet, no microphone,
no Whisper model, and no projector, and still exercise the complete
non-hardware-dependent CIP workflow: Bible Library, Bible Search, Bible
Browse, Saved Scripture, Manual Transcript, Intelligence, Service,
Presentation on the laptop's own screen, Content and Cross-Domain where the
architecture supports it, History, and restart recovery — using the real
production architecture and the real BSB dataset, never a fabricated
substitute.

---

## 1. Baseline

Phase 3.6 left CIP with: a real, checked-in BSB (Berean Standard Bible)
production dataset (66 books, 1,189 chapters, 31,086 verses); a Bible
Library (Browse/Search/Saved tabs); a Music Library (honest empty state, no
licensed production dataset); a History view (services, transcript,
timeline, presentation history); the full Bible/Music/Sermon/Service/
Content/Cross-Domain intelligence architecture from Phases 1–3.6; and a
Windows x64 NSIS installer, cross-compiled and never launch-tested on real
hardware. No prior phase has ever had access to a real Windows machine in
this container — that constraint is unchanged for Phase 3.7 (see section 23
and the final gate).

## 2. Audit findings

The mandatory pre-coding audit (spec section 3) covered every domain. The
single most important finding, and the one hard requirement this phase's
spec calls out by name (spec section 4):

**The Bible-readiness contradiction.** `get_live_status`'s `bible` field
correctly reports BSB as installed (it resolves
`bible_production_dataset::BSB_TRANSLATION_ID` directly). But **twelve**
other call sites in `apps/desktop/src-tauri/src/commands.rs` fell back to a
compile-time literal, `DEFAULT_TRANSLATION_ID = "KJV"`
(`state.rs`), whenever the frontend omitted `translationId` — which every
real caller does, since no UI ever asks the operator to pick a translation
from a list of one. `"KJV"` is a Phase 1.2 dev/test-fixture id, registered
in the Content Registry only when `apply_dev_seed` runs
(`lib.rs`: every non-`Production` environment). A real Windows release
build always runs in `Production`, so the dev seed never applies and
`bible:KJV` is never registered there — only `bible:BSB` is.

Because `is_translation_selectable`/`ensure_translation_selectable`
deliberately **fail open** for an unregistered id (an unbookkept
translation is never treated as blocked — see that function's own docs),
this never produced an error. It silently queried `translation_id = 'KJV'`
against a database that only has `'BSB'` rows, returning **empty results**.
That is exactly the reported symptom shape: Diagnostics says "BSB —
local," while Bible Library search/browse says "no results," and — this
audit's own discovery, beyond what the baseline symptom described — Bible
*detection itself* (live microphone **and** manual transcript) silently
fails to validate any reference a pastor or operator actually says or
types, because `handle_audio_chunk`/`process_test_transcript` fed the same
stale literal into `handle_final_transcript`'s `BibleProvider` lookups.

The twelve affected call sites: `preview_presentation`, `preview_scripture`,
`prepare_presentation`, `create_manual_presentation`, `search_bible`,
`list_bible_books` (the six already partly addressed before this audit's
full sweep), plus **six more this audit found**: `handle_audio_chunk` (the
live-microphone pipeline), `process_test_transcript` (the manual-transcript
pipeline — directly relevant to this phase's core deliverable),
`edit_suggestion`, `resolve_ambiguous_reference` (two internal call sites),
and `correct_scripture_context`.

Other domains audited (Music, Sermon, Service, Content, Cross-Domain,
Presentation, History, Windows installation) confirmed Phase 3.6's own
findings still hold and needed no further root-cause fix — see sections
5–12 below for what each domain's audit concluded.

## 3. Architecture reused (no new intelligence engine)

Per spec section 2 and section 29, this phase introduces **zero** new
intelligence engines, Bible providers, presentation engines, or history
architectures. Everything the Offline Test Center (section 13) and the new
acceptance test (section 19) exercise is a call into an *already-existing*
production function:

- `commands::process_test_transcript` → `pipeline::handle_final_transcript`
  → `cip_core_service::process_transcript_segment` (the exact same Bible
  Intelligence Core a real microphone segment reaches).
- `commands::analyze_sermon_transcript`, `analyze_music_transcript`,
  `analyze_cross_domain`, `analyze_content_intelligence` — all pre-existing
  manual-entry paths for their domains (previously reachable only from
  inside Diagnostics Mode's per-domain panels, per the Phase 3.2–2.7
  history in this repository).
- `presentation::build_scripture_slide` / `persist_prepared_item` /
  `prepare_to_activate` / `commit_activation` / `stop_active_item` — the
  same functions `preview_presentation`/`prepare_presentation`/
  `display_presentation`/`clear_presentation_display` already call.
- `persistence::persist_saved_scripture` / `list_saved_scriptures` /
  `list_services` / `get_service` / `list_presentation_items` — all
  pre-existing (the first pair from Phase 3.6).

## 4. Changes made

1. **Root-cause fix** (`apps/desktop/src-tauri/src/commands.rs`): new
   `resolve_default_translation_id(state)` / pure
   `resolve_default_translation_id_from_registry(registry)` — checks
   whether `bible:BSB` is registered first (the real production dataset),
   falling back to the dev-fixture literal only when it is not (mirroring
   `get_live_status`'s own correct resolution). All twelve call sites from
   section 2 now call this instead of the raw constant. Two new regression
   tests (`resolve_default_translation_id_prefers_bsb_when_registered_...`,
   `..._falls_back_to_the_dev_fixture_when_bsb_is_not_registered`) prove
   both branches directly.
2. **Offline Test Center** (`apps/desktop/src/components/testcenter/
   TestCenter.tsx`, new): a first-class navigation destination (added to
   `App.tsx`'s top-level nav alongside Live Service/Bible/Music/History) —
   see section 13 for what it does.
3. **Full offline operator acceptance test**
   (`apps/desktop/src-tauri/src/pipeline.rs`:
   `phase_3_7_full_offline_operator_chain_acceptance`) — see section 19.
4. No database migration (see section 14 — none was justified).
5. Windows/Linux release artifacts rebuilt (section 16).

No existing Tauri command signature, event name/payload, intelligence
engine, correlation rule, presentation state machine, or database table was
altered. `git diff` against `45f23b4` (available for review) touches only:
`commands.rs` (the fix), `pipeline.rs` (the new test), `App.tsx` (one new
nav entry + import), the new `TestCenter.tsx`, this document, evidence
files under `pilot-evidence/3.7/`, and `release/windows/*`.

## 5. Bible

**FULLY TESTABLE OFFLINE.** Bible Library (Phase 3.6) already let an
operator browse all 66 books, open chapters, search by reference or text,
open verse ranges, save Scripture, reopen saved Scripture, and delete saved
Scripture, all against the real BSB dataset. This phase's root-cause fix
(section 2/4) closes the last gap: every one of those paths, plus live and
manual Bible *detection*, now resolves against the real installed BSB
dataset in a real `Production` build instead of silently querying an
unregistered dev-fixture id. Proven end-to-end by
`phase_3_7_full_offline_operator_chain_acceptance` (section 19): search →
save → manual-transcript detection → approve → prepare → activate → stop →
restart → reopen, all against the real dataset.

## 6. Music

**PARTIALLY TESTABLE OFFLINE** — unchanged conclusion from Phase 3.6's own
audit (`docs/phase-3-6-church-libraries.md` section 24), re-confirmed here
rather than re-litigated: no licensed production song dataset exists in
this repository, so a production build's Music Library honestly shows "no
production music library installed" (never fabricated song data). The
*real* `searchMusic`/`analyzeMusicTranscript` pipeline is fully testable
offline against the small, honestly-labeled development fixture dataset
(`"Test Fixture Hymn One"` etc.) — but that fixture is registered only in
non-`Production` builds (`apply_dev_seed`), so the Offline Test Center's
Multi-Domain scenario (section 13) explicitly documents that its music step
"only matches in a development/test environment... honestly NOT in a real
production install with no licensed music library." Section 10 below
covers why no test data was added to the production build.

## 7. Sermon

**FULLY TESTABLE OFFLINE.** Sermon Intelligence (`analyze_sermon_transcript`)
is a deterministic, heuristic, offline-only text engine with no dataset
dependency at all (unlike Bible/Music) — it never needed licensed content
to begin with. The Offline Test Center's Sermon scenario and the Full
Service scenario both exercise it through the real production command.

## 8. Service

**FULLY TESTABLE OFFLINE.** Service Intelligence (`analyze_service_transcript`,
phase/anomaly tracking) is the same kind of deterministic offline text
engine as Sermon — no dataset dependency. Service *lifecycle*
(start/pause/resume/end) has been offline-testable since Phase 1.3 and is
exercised directly by the new acceptance test and by every Offline Test
Center scenario (each requires an active test service first, exactly like
real speech would).

## 9. Content

**PARTIALLY TESTABLE OFFLINE.** `analyze_content_intelligence` maps
Bible/Sermon/Music findings into `ContentCandidate`s. Since Bible and
Sermon are both fully offline-testable (sections 5, 7), any Content
candidate sourced from either is fully testable offline too — proven by the
Offline Test Center's Multi-Domain scenario, which is capable of producing
Bible- and Sermon-sourced findings for `analyze_content_intelligence` to
consider. A Music-sourced candidate is bounded by Music's own limitation
(section 6): not reachable in a production build with no licensed dataset.
This is a data-availability limit, not an architecture gap — the
Content Intelligence engine itself has no offline dependency.
`accept_content_candidate`/`listAcceptedContentCandidates` (pre-existing,
Phase 2.7/3.0, re-audited this phase and found unchanged/correct) complete
the operator review → accept → saved path; the underlying `ContentCandidate`
queue remains in-memory only, a pre-existing, deliberate, unrelated design
decision (see `pilot-evidence/3.6/validation-matrix.json`'s "Saved
Content" row) — not something this phase's offline-testability audit
requires changing.

## 10. Cross-Domain

**PARTIALLY TESTABLE OFFLINE**, for the identical reason as Content: the
deterministic correlation rule engine (`core/intelligence/src/
cross_domain.rs`) has no dataset dependency and is fully exercised offline
by `analyze_cross_domain` whenever Bible and/or Sermon findings are present
close together (the Offline Test Center's Multi-Domain scenario is built
to attempt exactly this). A correlation rule anchored on a Music finding is
bounded by the same production-dataset limitation as section 6/9.
`AssertionLevel`, `ConfidenceResult`, `EvidenceSource`, and provenance are
all preserved unmodified end-to-end — none of this phase's changes touch
`cross_domain.rs` or the correlation types.

## 11. Presentation

**FULLY TESTABLE OFFLINE**, on the laptop's own screen — never a physical
projector claim (spec section 6). Search → open → Prepare → Preview →
Display → Stop was already the production path from Phase 1.4 onward;
`phase_3_7_full_offline_operator_chain_acceptance` proves the full chain
(`build_scripture_slide` → `persist_prepared_item` → `prepare_to_activate`
→ `commit_activation` → `stop_active_item`) against the real BSB text for
James 2:2, with no audio, no microphone, and no display hardware beyond
whatever renders the operator's own window.

## 12. History

**FULLY TESTABLE OFFLINE.** Phase 3.6's audit already established that
`HistoryView.tsx` reconstructs services, transcript, timeline, suggestions,
and presentation items purely from SQLite persistence — nothing invented,
nothing added this phase. The new acceptance test extends the *proof*
depth: it closes a real file-backed SQLite connection and reopens it
(the same real-restart technique as
`service_history_survives_a_simulated_application_restart`, Phase 1.3),
confirming the completed test service, its saved Scripture, and its
presentation item's final `Stopped` status all survive — never reset to
`Active`, never lost.

## 13. Manual Transcript & Test Mode

The Offline Test Center (`apps/desktop/src/components/testcenter/
TestCenter.tsx`) is the single, first-class, always-reachable answer to
spec sections 7, 8, 16, 17, and 18 together:

- **Core Offline Readiness** strip: Bible/Manual Input/Presentation shown as
  ready (never alarming), Microphone/Speech shown as *optional, not
  configured* — never as a failure (spec section 16/28's BAD-vs-BETTER
  distinction).
- **Test Service** controls (start/end), required before any scenario runs
  — matching real speech's own requirement of an active service.
- **Manual Transcript** — a single text box submitting straight into
  `processTestTranscript`, i.e. the exact same production Bible pipeline
  live speech uses. No separate "test" detection logic exists anywhere in
  this path.
- **Test Scenarios** — five deterministic, pre-written transcripts
  (Scripture; Scripture + Context; Sermon; Multi-Domain; Presentation) plus
  a Full Service run (Start → Welcome → Worship → Scripture → Sermon →
  Prayer → Closing → Stop), each labeled with exactly what it is expected
  to (and, where relevant, is honestly *not* guaranteed to) trigger — never
  a fabricated "PASS."
- **Activity Log** — reports what was *submitted*, not a guessed outcome;
  actual findings are reviewed on the Live Service tab's existing Attention
  Queue/Intelligence Feed, deliberately not duplicated here.

This satisfies the mission's "manual transcript enters the same production
intelligence pipeline" rule literally: `TestCenter.tsx` imports nothing
from a fake/parallel engine — every button calls a pre-existing
`lib/commands.ts` wrapper around a pre-existing Tauri command.

## 14. Offline verification

`cargo tree --workspace --all-features` contains no HTTP client crate (the
same check every prior phase has run — unchanged result this phase). The
Xvfb smoke test (section 22) shows the full startup path — Bible dataset
verification, sermon/service/content-intelligence engine initialization,
speech-engine and acoustic-recognizer degradation reporting — with no
network activity of any kind, `environment: Production`. No new dependency
was added this phase, so no new offline-dependency risk was introduced.

## 15. Database migration rule

**No migration was added this phase.** Per spec section 14, an additive
migration is only justified once it's proven the existing schema cannot
support the requirement. Every capability this phase strengthens (Bible
detection resolution, saved Scripture, presentation lifecycle, service
history) already had a fully sufficient schema and API surface from Phases
1.0–3.6 — the root cause (section 2) was a **code-level** literal, not a
missing table or column. Section 13 (History reuse) and section 19
(the new acceptance test) both confirm the existing schema was sufficient
by successfully exercising the entire chain against it without any schema
change.

## 16. Windows installer verification

Rebuilt via `cargo build --release --target x86_64-pc-windows-gnu` (through
`tauri build`), the identical cross-compilation toolchain used since Phase
3.4 (rustup target `x86_64-pc-windows-gnu`, `gcc-mingw-w64`, `makensis`).

- `target/x86_64-pc-windows-gnu/release/cip-desktop.exe`: `file(1)` reports
  `PE32+ executable (GUI) x86-64 (stripped to external PDB), for MS
  Windows` — genuinely x64.
- Installer `Church Intelligence Platform_0.1.0_x64-setup.exe`: `file(1)`
  reports a standard 32-bit NSIS self-extracting *bootstrapper* stub — this
  is normal NSIS behavior, not a sign the installed application is 32-bit
  (same distinction documented in every prior release manifest).
- SHA-256: `8fd186ffb79ec362365c6e30d2b9894204c8bbff4f9c7672287add09adfca800`
  (7,107,984 bytes) — recorded in
  `release/windows/Church Intelligence Platform_0.1.0_x64-setup.exe.sha256`
  and `release/windows/release-manifest.json`.
- A Linux `.deb` was also rebuilt (`target/release/bundle/deb/`, not
  committed — `target/` is gitignored, same as every prior phase); its
  checksum is recorded in `pilot-evidence/3.7/software/
  release-artifact-3.7-linux.sha256` as Environment A/B attestation that
  the Linux release build succeeds, matching the pattern in
  `pilot-evidence/3.4–3.6/software/release-artifact-*-linux.sha256`.

This is cross-compilation and `file(1)` verification — Environment A/B
evidence. It is **not** installation, launch, or first-run testing on a
real Windows machine (that remains Environment C, section 23/25 — not
performed this phase, same as every prior phase).

## 17. Failure recovery

- **Close/reopen**: proven directly by `phase_3_7_full_offline_operator_chain_acceptance`
  (a real file-backed SQLite connection is dropped and reopened) and by the
  pre-existing `service_history_survives_a_simulated_application_restart`
  / `prepared_presentation_items_survive_a_simulated_restart_and_stay_prepared`
  tests (unmodified, still passing — see section 20).
- **Kill mid-service**: `reconcile_stale_active_presentation_items`
  (pre-existing, Phase 1.4/2.9) still runs at startup, unaffected by this
  phase's changes — a stale `Active` item is reconciled to `Stopped` on the
  next launch, never left claiming to still be on screen.
- **Missing microphone / missing Whisper model**: both were already
  optional at startup (`cip::speech` logs "built without the `whisper`
  feature; live transcription is unavailable (manual operation still
  works)" — see the Xvfb log, section 22); this phase does not change
  startup gating, and the Offline Test Center's readiness strip now makes
  this explicit and non-alarming in the UI (section 13/28) rather than
  only in a log line.
- **Corrupted/invalid speech configuration**: unchanged from Phase 3.2's
  hardware-capability-matrix audit (`docs/phase-3-2-hardware-pilot.md`) —
  speech configuration failures are already isolated from application
  startup; no change was needed or made this phase.

No hardware-recovery claim is made here — see section 25.

## 18. Security

Re-audited per spec section 25: `git diff 45f23b4 -- apps/desktop/src-tauri/capabilities/
apps/desktop/src-tauri/tauri.conf.json` is empty — no Tauri capability, CSP,
or IPC authorization surface changed this phase. The only new Tauri-adjacent
surface is `TestCenter.tsx`, which calls exclusively pre-existing,
pre-authorized commands (no new `#[tauri::command]` was added this phase).
No new filesystem path, database path, environment variable, or local model
path was introduced. No secrets, debug logging (`dbg!`/`console.log`), or
temporary/backup file handling was added — see the secrets scan in
`pilot-evidence/3.7/software/automated-regression.json`.

## 19. Licensing

No new dataset or content asset was introduced this phase. BSB remains
`VerifiedPublicDomain` (unchanged). The Offline Test Center's Music
scenario uses only the pre-existing, already-audited development fixture
(`"Test Fixture Hymn One"`, from `database/seeds/dev_seed.sql` /
`integrations/music`'s dev fixture, documented in
`docs/music-datasets.md`) — no new song, lyric, font, image, or icon asset
was added. No copyrighted production song data was introduced, per spec
section 10/26's explicit prohibition.

## 20. Automated acceptance test (spec section 19)

`pipeline::tests::phase_3_7_full_offline_operator_chain_acceptance`
(`apps/desktop/src-tauri/src/pipeline.rs`) proves, in one deterministic
test against a real file-backed SQLite database and the real, complete BSB
dataset:

fresh DB → import + verify real BSB → real-text search (`James 2:2`) →
save Scripture → start a service → submit a manual transcript
(`"Please turn to James chapter 2 verse 2"`) through the exact production
pipeline (`handle_final_transcript`) → verify the resulting detection and
suggestion resolve against real BSB text → operator approves →
`build_scripture_slide`/`persist_prepared_item`/`prepare_to_activate`/
`commit_activation`/`stop_active_item` (prepare → activate → stop, laptop
screen only) → stop the service → **close the connection and reopen the
same on-disk file** (a real restart, not the same connection kept open) →
verify the service shows `Ended` and appears in history, the saved
Scripture is still present with the same reference, and the presentation
item's final state is `Stopped` (never reset to `Active`) → a **second**,
completely fresh `BibleProvider` connection confirms the saved reference
still resolves to the exact real BSB verse text.

Where a domain is only partially testable offline (Music, and the
Music-anchored slice of Content/Cross-Domain — sections 6/9/10), this is
recorded honestly rather than fabricated into the test.

## 21. Environment A evidence (automated)

See `pilot-evidence/3.7/software/automated-regression.json` for full
command output. Summary: `cargo fmt --check` pass; `cargo check --workspace`
pass; `cargo clippy --workspace --all-targets -- -D warnings` pass (0
warnings); `cargo test --workspace` **779 passed, 0 failed** (up from 776
at the Phase 3.6 baseline — the 2 new `resolve_default_translation_id_*`
regression tests plus the 1 new
`phase_3_7_full_offline_operator_chain_acceptance` test); `cargo check -p
cip-desktop --features whisper` pass; `cargo test -p cip-ai-speech
--features whisper` 7 passed, 0 failed; `npm run typecheck` pass; `npx
vitest run` 191 passed, 0 failed (unchanged from Phase 3.6 — `TestCenter.tsx`
introduces no new pure/exported logic beyond static scenario data, which
TypeScript's own compiler already checks structurally); `npm run build`
pass; `npm run lint` 0 errors, 3 pre-existing warnings (unchanged from
every prior phase, not introduced here).

This proves code correctness and deterministic behavior — **not** human
usability, Windows physical behavior, or real hardware.

## 22. Environment B evidence (Xvfb)

Full logs in `pilot-evidence/3.7/software/cip-xvfb-3-7-run1-fresh.log` and
`cip-xvfb-3-7-run2-idempotent.log`. Fresh launch (`HOME` pointed at an
empty directory): `10 migration(s) applied`, `BSB production Bible
dataset: 66 book(s), 1189 chapter(s), 31086 verse(s) total (31086
imported, 0 already present)`, sermon/service/content-intelligence engines
initialized `(deterministic, offline)`, speech engine reports it was built
without the `whisper` feature (manual operation still works), acoustic
recognizer reports `Unavailable` (no configured model directory) — never
treated as a startup failure. Idempotent relaunch against the same `HOME`:
`0 migration(s) applied`, `(0 imported, 31086 already present)` — proving
both a clean first launch and a safe, idempotent second launch, entirely
under Xvfb with `environment: Production`.

This proves the Linux release binary starts, initializes every offline
engine, and behaves idempotently — **not** Windows UX, real audio, a
physical display, or human usability. Xvfb is never substituted for
Environment C anywhere in this document.

## 23. Environment C evidence (real Windows laptop)

**Not performed this phase.** This container has no real Windows machine —
the same constraint reported in every prior phase (3.1 through 3.6). No
human operator sat at a real Windows laptop, disconnected it from the
Internet, installed this phase's `.exe`, or exercised the checklist in
section 24. This is the deciding fact behind this phase's final gate
(section 26/27) — see spec section 41's binding rule, quoted there
verbatim.

## 24. Human operator checklist (for the real Windows laptop)

The exact procedure a human operator should run on the real HP EliteBook
(or any real Windows 10/11 x64 laptop) to turn this phase's gate from HOLD
to PASS. Every step below calls only what this phase's implementation
actually supports (per spec section 22's "the exact steps may be expanded
based on what the implementation actually supports").

1. Disconnect the laptop from the Internet (airplane mode or unplug
   Ethernet/Wi-Fi).
2. Copy `release/windows/Church Intelligence Platform_0.1.0_x64-setup.exe`
   to the laptop (USB drive, direct transfer — no developer tooling
   required) and run it. Expect a Windows SmartScreen "unknown publisher"
   warning (the installer is unsigned, per `release-manifest.json`'s
   `knownLimitations`) — proceed anyway ("More info" → "Run anyway").
3. Launch **Church Intelligence Platform** from the Start Menu.
4. Confirm the app window opens with no microphone and no projector/second
   display connected.
5. Confirm Bible = READY, Database = READY, Manual Input = READY,
   Presentation = READY. Confirm Microphone and Speech (Whisper) are shown
   as **optional, not configured** — never as a red failure state.
6. Open **Bible**. Confirm the Browse tab lists all 66 books.
7. Open **Genesis**, confirm chapter 1 opens with real verse text.
8. Open **James**, confirm chapter 2 opens.
9. Search "James 2:2". Confirm the real BSB verse text appears.
10. Open the result. Save it. Confirm it appears under the Saved tab.
11. Prepare it for presentation.
12. Open the presentation display window on the laptop's own screen (not a
    projector).
13. Display the prepared Scripture. Confirm the real BSB text is legible
    on screen.
14. Stop the display.
15. Go to **Offline Test Center**. Confirm the readiness strip matches
    step 5.
16. Start a Test Service.
17. In Manual Transcript, submit: `"Please turn to James chapter 2 verse
    2."`
18. Switch to **Live Service**. Confirm a Bible suggestion for James 2:2
    appears in the Attention Queue.
19. Approve it.
20. Prepare it, open the presentation display, and display it on the
    laptop's own screen.
21. Stop the display.
22. Return to Offline Test Center and End the Test Service.
23. Open **History**. Confirm the just-ended test service appears, with
    its transcript, the approved suggestion, and the presentation item
    visible.
24. Close CIP entirely (not just the window — exit the application).
25. Relaunch CIP.
26. Open **Bible → Saved**. Confirm the James 2:2 saved Scripture from
    step 10 is still present.
27. Open **History**. Confirm the test service from step 23 is still
    present, with the same data.
28. Record actual timings for each step (section 25) and any deviation
    from the above for the final report.

## 25. Performance

Not captured on real Windows hardware this phase (no Environment C
available — section 23). Marked **NOT VERIFIED**; the procedure to capture
it is section 24 step 28 above — a human operator with a stopwatch (or the
OS's own timing) on the real laptop, recording: cold startup time, Bible
Library open time, search latency for "James 2:2", chapter-browse latency,
Scripture save latency, test-service creation latency, manual-transcript
processing latency (submit → suggestion appears), presentation prepare
latency, presentation display latency, History load latency, and
close/relaunch (restart) recovery time. No timing value is invented here.

## 26. Known limitations

- No Environment C (real Windows hardware) evidence exists for this phase
  or any prior phase — see section 23.
- Music Library is legitimately empty in a production build (section 6);
  Content/Cross-Domain findings anchored on Music are bounded by the same
  limitation (sections 9–10).
- Physical microphone, Whisper transcription, and physical
  projector/second-display qualification are explicitly out of scope for
  this phase (spec section 35) and are not claimed anywhere in this
  document.
- The Windows installer remains unsigned (SmartScreen warning on first
  run) — unchanged from every prior phase's known limitation.
- Saved `ContentCandidate` records remain in-memory only — a pre-existing,
  deliberate, unrelated design decision from Phase 2.7/3.0, re-confirmed
  (not re-litigated) by this phase's audit.

## 27. Deferred physical hardware qualification

Per spec section 35, even a PASS on this phase's offline-operator gate
would **not** constitute microphone-, Whisper-, or projector-qualification.
Those remain separate, later Environment C phases, in sequence: **Phase
3.7 PASS → Physical Hardware Qualification → Microphone → Whisper → Second
Display/Projector → Real Service → Final Pilot Gate.** Given this phase's
gate is HOLD (see below), none of that sequence begins yet — per spec
section 42's stop condition, this session does not auto-begin microphone,
projector, or Phase 3.8 work.

---

## PASS/HOLD matrix

| Item | Result |
|---|---|
| Software build (Windows installer + Linux release, cross-compiled) | PASS |
| Automated tests (Environment A) | PASS (779/779 Rust, 191/191 frontend, 0 failures) |
| Xvfb smoke test (Environment B) | PASS (fresh + idempotent relaunch, offline, `Production` environment) |
| Real Windows offline operator (Environment C) | **HOLD** — not performed, no real Windows laptop available in this container |
| Microphone qualification | NOT TESTED (out of scope, spec section 35) |
| Whisper qualification | NOT TESTED (out of scope, spec section 35) |
| Projector/second-display qualification | NOT TESTED (out of scope, spec section 35) |

**Required CORE items** (spec section 34) — Bible Library, Bible Search,
Bible Browse, Saved Scripture, Manual Transcript, Intelligence Processing,
Service Lifecycle, Presentation on Laptop Screen, History, Restart
Recovery, Offline Operation — are all proven at Environment A/B (code
correctness + real-BSB-dataset acceptance test + Xvfb offline runtime).
**None of them has Environment C proof**, because no real Windows laptop
was used this phase. Per spec section 41's unconditional rule, that alone
is sufficient to hold the gate regardless of how complete the Environment
A/B evidence is — averaging or "mostly pass" language is explicitly
disallowed by spec section 34.

## Final gate

```
FULL OFFLINE OPERATOR TEST: HOLD
```

```
REAL WINDOWS HARDWARE QUALIFICATION: NOT PART OF THIS GATE
```

**Exact blocker**: no real Windows laptop is available in this build
environment. Every Environment A (automated) and Environment B (Xvfb)
check this phase set out to prove has passed cleanly, and the root-cause
Bible-readiness bug (section 2) is fixed and regression-tested. The single
remaining requirement to convert this HOLD into a PASS is **Environment
C**: a human operator running the section 24 checklist, step by step, on
the real target Windows laptop (the HP EliteBook or equivalent), with
Internet disconnected. No amount of additional automated testing or Xvfb
verification can substitute for that step — per spec section 41's explicit
rule, this document does not convert automated or Xvfb results into
Environment C evidence, does not claim human usability without an actual
human operator, and does not claim physical hardware readiness.

Per spec section 42: this HOLD stops the phase here. Microphone/projector
qualification and Phase 3.8 do not begin automatically.
