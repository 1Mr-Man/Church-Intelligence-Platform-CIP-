# Phase 2.7.1 — Content Intelligence Operationalization & Church Resource
# Library UX

## 1. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `4c6978f` (Phase 3.7, "Full Offline Operator Test Mode &
  System Verification")
- Working tree at start: clean

Full audit findings live in `docs/phase-2-7-1-audit.md`, written and
completed **before** any implementation began, per this phase's own
audit-first requirement. This document covers what was actually built
afterward and the resulting evidence/gate.

## 2. Audit findings

Summarized (full detail in `docs/phase-2-7-1-audit.md`):

- Bible Library, Bible search/browse/save/reuse, and the presentation
  chain were all already real and working against the real, complete BSB
  dataset (66 books, 1,189 chapters, 31,086 verses, verified directly this
  phase, not merely cited from a prior report).
- No Bible cross-reference data exists anywhere in this codebase - a fact,
  not a gap, now stated honestly in the UI rather than left silent.
- Music has no licensed production dataset and the `MusicProvider` trait
  has no song-enumeration method at all - building browse/save against the
  fictional dev fixture was judged not worth doing, again, matching Phase
  3.6's own conclusion.
- **The one clearly-provable gap**: `ContentCandidate` acceptance only
  ever flipped an in-memory `Mutex`'s status. Nothing wrote it to SQLite.
  An accepted content candidate did not survive the service ending, let
  alone an application restart - directly conflicting with this phase's
  own acceptance scenario and "SAVED CONTENT" gate requirement.

## 3. Existing architecture reused

Every change this phase makes composes with, rather than duplicates,
existing architecture: the real `ContentCandidate` type (persisted
verbatim via its own pre-existing `Serialize`/`Deserialize`, never
re-derived), the existing `saved_scriptures` migration/persistence pattern
(reused as the template for the new table), the existing
`list_presentation_history(serviceId)` command shape (reused as the
template for `list_saved_content(serviceId)`), and the existing
`HistoryView.tsx` card-list rendering pattern. No second intelligence
engine, no second `ContentCandidate` type, no second presentation path,
no second persistence architecture.

## 4. New functionality

1. `database/migrations/0011_saved_content_candidates.sql` (additive) - a
   `saved_content_candidates` table storing the accepted `ContentCandidate`
   as JSON, verbatim.
2. `persistence.rs`: `persist_saved_content_candidate`,
   `list_saved_content_candidates_for_service`.
3. `commands.rs`: `accept_content_candidate` now also persists a durable
   copy; new `list_saved_content(serviceId)` command.
4. `HistoryView.tsx`: a new "Saved Content" section per opened service.
5. `BibleLibrary.tsx`: an honest cross-reference disclaimer under every
   verse's text.

## 5. Bible Library

Re-confirmed fully working (Browse/Search/Saved/Preview/Prepare), all
against the real BSB dataset, with no audio dependency anywhere. The only
change this phase makes is additive honesty: "Cross-references are not
available in this installed Bible dataset." — no cross-reference data was
fabricated to fill that gap.

## 6. Music Library status

Unchanged. **LEGALLY BLOCKED** for browse/save: no licensed production
song dataset exists in this repository, and this phase additionally
confirmed the `MusicProvider` trait itself has no song-enumeration method
- there is currently no way to list "every song in a dataset" even for the
fictional dev fixture. Search against whatever dataset *is* installed
(dev/test only) continues to work unchanged. Building a browse/detail/save
UI against fictional data was judged, again, not worth doing.

## 7. Content Intelligence

The full detection → correlation → review → accept chain was already real
end to end up through acceptance. This phase closes the one broken link:
acceptance now durably persists. Nothing about detection, correlation,
review, or rejection changed - `analyze_content_intelligence`,
`list_content_candidates`, `reject_content_candidate`, and the events
`ContentCandidateDetected`/`Accepted`/`Rejected` are all untouched.

## 8. Saved resources

Saved Scripture (Phase 3.6): unchanged, still fully durable. Saved
Content (this phase): now durable via `saved_content_candidates`,
reopenable via `list_saved_content(serviceId)` regardless of whether that
service is still active or the application has since restarted - proven
by `pipeline::tests::phase_2_7_1_saved_content_candidate_survives_a_real_restart`,
which closes and reopens a real on-disk SQLite file mid-test.

## 9. History

Gained a new "Saved Content" section, reusing the exact same list-card
pattern Presentation History already used. Service History, Presentation
History, Scripture & Findings, Transcript, and Timeline are all unchanged.

## 10. Presentation

Unchanged. `PresentationItem`'s `Prepared → Active → Stopped` state
machine, `build_scripture_slide`/`persist_prepared_item`/
`prepare_to_activate`/`commit_activation`/`stop_active_item`, and startup
crash reconciliation are all untouched this phase - re-confirmed, not
re-implemented.

## 11. Offline operation

No new dependency was added. `cargo tree --workspace --all-features`
still contains no HTTP client crate. The Xvfb smoke test (section 18)
shows a fresh launch applying migration 11, importing the real BSB
dataset, and initializing every intelligence engine `(deterministic,
offline)`, entirely under `environment: Production` with no network
activity - and an idempotent second launch applying zero migrations and
importing zero already-present verses.

## 12. Licensing

BSB: `VerifiedPublicDomain`, unchanged. No new song, lyric, font, image,
or icon asset was introduced. The persisted `ContentCandidate` payload
contains only content this application's own intelligence engines already
derived from a real transcript the operator entered - never anything
copied from an external source.

## 13. Security

`git diff` against the starting HEAD shows an empty diff for
`apps/desktop/src-tauri/capabilities/`, `tauri.conf.json`, and
`events.rs` - no capability, CSP, IPC-authorization, or event-surface
change this phase. Exactly one new Tauri command was added
(`list_saved_content`, read-only, service-scoped, same authorization
posture as the pre-existing `list_presentation_history`). No new
filesystem path, database path, environment variable, or local model path.
Secrets/debug-artifact scan: no matches.

## 14. Performance

The new `list_saved_content_candidates_for_service` query is bounded to
one service's rows (same discipline as every other `list_*` query in this
codebase) and never loads the full table. No O(n²) operation was
introduced. Real-hardware timing was not captured this phase (no
Environment C available) and is not claimed - the same honest limitation
every prior phase has recorded for real Windows timing.

## 15. Failure recovery

Covered by `phase_2_7_1_saved_content_candidate_survives_a_real_restart`:
close/reopen the real database file, and the accepted candidate is
unchanged and intact. No new failure mode was introduced - persisting on
accept happens inside the same `db` lock already held for that command's
existing timeline write, so a persistence failure surfaces as a normal
`AppError`, exactly like every other write in `commands.rs`.

## 16. Tests

Four new tests (three persistence round-trip tests, one real-file-restart
acceptance test), all passing. Zero existing tests were removed or
weakened. See section 17 for exact counts.

## 17. Environment A results (automated)

See `pilot-evidence/2.7.1/software/automated-regression.json`. Summary:
`cargo fmt --check` pass; `cargo check --workspace` pass; `cargo clippy
--workspace --all-targets -- -D warnings` pass (0 warnings); `cargo test
--workspace` **783 passed, 0 failed** (up from 779 at the Phase 3.7
baseline); `cargo check -p cip-desktop --features whisper` pass; `cargo
test -p cip-ai-speech --features whisper` 7 passed, 0 failed; `npm run
typecheck` pass; `npx vitest run` 191 passed, 0 failed (unchanged); `npm
run build` pass; `npm run lint` 0 errors, 3 pre-existing warnings
(unchanged from every prior phase).

## 18. Environment B results (Xvfb)

Full logs in `pilot-evidence/2.7.1/software/cip-xvfb-2-7-1-run1-fresh.log`
and `cip-xvfb-2-7-1-run2-idempotent.log`. Fresh launch: `11 migration(s)
applied`, real BSB dataset imported (31,086 verses), every intelligence
engine initialized `(deterministic, offline)`. Idempotent relaunch: `0
migration(s) applied`, `(0 imported, 31086 already present)`. Both under
`environment: Production`, no network activity.

## 19. Environment C results

**Not performed.** No real Windows machine is available in this
container - the same constraint recorded in every prior phase (3.1
through 3.7). No human operator ran the checklist in section 21.

## 20. Deferred work

- Music song browse/detail/save (no licensed dataset; no enumeration
  capability in `MusicProvider` either).
- A generalized Collections/Favorites framework (not proven necessary -
  the flat Saved Scripture and Saved Content lists already serve the
  reuse need).
- Per-verse "used in service/presentation" usage-reference UI (the
  underlying data exists, but a cross-service query to surface it well is
  a genuinely separate addition, deferred rather than rushed).
- Any visual redesign beyond Phase 3.5.1's existing semantic color system
  - the reference screenshot was UX inspiration only, never a redesign
  mandate.

## 21. Final gate

Per-capability evaluation (spec section 35):

| Item | Result |
|---|---|
| BIBLE LIBRARY | PASS (Environment A/B) |
| BIBLE SEARCH | PASS |
| BIBLE BROWSE | PASS |
| SCRIPTURE SAVE/REUSE | PASS |
| SCRIPTURE PRESENTATION | PASS |
| MUSIC LIBRARY | LEGALLY BLOCKED |
| CONTENT INTELLIGENCE | PASS |
| SAVED CONTENT | **PASS** (was the one real gap this phase closes) |
| SERVICE HISTORY | PASS |
| PRESENTATION HISTORY | PASS |
| OFFLINE TEST WORKFLOW | PASS (Environment A/B - Xvfb offline verified) |
| DATABASE PERSISTENCE | PASS |
| FAILURE RECOVERY | PASS |
| SECURITY | PASS |
| LICENSING | PASS |
| PERFORMANCE | PASS (Environment A/B - bounded queries; real-hardware timing not captured, not claimed) |
| ARCHITECTURE | PASS (single intelligence/presentation/persistence architecture preserved, confirmed by diff) |

Every capability this phase set out to prove is genuinely provable at
Environment A/B, and Saved Content - the one real gap the audit found -
is now fixed and durably tested.

That said, this document does not convert Environment A/B evidence into
Environment C evidence, and does not claim physical hardware readiness or
human-operator usability. No real Windows laptop was used this phase -
the same standing limitation `docs/phase-3-7-offline-operator-test.md`
already recorded, unchanged by this phase's work. The overarching gate
therefore remains:

```
FULL OFFLINE OPERATOR TEST: HOLD
```

**Exact blocker** (unchanged from Phase 3.7): a human operator has not run
the section 21 checklist below on a real Windows laptop with Internet
disconnected. Every Environment A/B check this phase and Phase 3.7 set out
to prove has passed cleanly.

## Human operator checklist addendum (extends `docs/phase-3-7-offline-operator-test.md` section 24)

After completing that document's 28-step checklist, additionally:

29. In Offline Test Center, run the Multi-Domain or Sermon scenario, then
    switch to Live Service and accept the resulting Content Candidate (if
    one was produced).
30. Close CIP entirely and relaunch it.
31. Open History, select the same test service, and confirm the accepted
    candidate appears under "Saved Content" with its original title and
    working concept intact.

---

**Answers to the phase's own final questions (spec section 30):**

1. Can the operator use the Bible without audio? **Yes.**
2. Can the operator browse all 66 books? **Yes.**
3. Can the operator search the real BSB dataset? **Yes.**
4. Can the operator save Scripture? **Yes.**
5. Can the operator reuse Scripture? **Yes.**
6. Can the operator present Scripture? **Yes**, on the laptop's own
   screen.
7. Can the operator see presentation history? **Yes.**
8. Can the operator see service history? **Yes.**
9. Can the operator test intelligence without a microphone? **Yes**, via
   Manual Transcript / the Offline Test Center (Phase 3.7).
10. Can the operator review Content Candidates? **Yes.**
11. Can the operator save/reopen accepted content? **Yes - this is what
    this phase fixed.**
12. Does everything survive restart? **Yes**, proven at Environment A for
    every capability listed above; not yet proven on real Windows
    hardware (Environment C).
13. Does it work offline? **Yes**, proven at Environment A/B.
14. Is any production music data missing? **Yes** - honestly disclosed,
    never fabricated.
15. Are licensing limitations honestly represented? **Yes.**
16. Did you create any duplicate architecture? **No.**
17. Did you alter existing backend contracts? **No** - one command
    (`accept_content_candidate`) gained an additional side effect with an
    unchanged response shape; one new command was added; nothing existing
    was removed or changed shape.
18. Was physical hardware actually tested? **No.**
19. Was a human church operator actually tested? **No.**
20. Is the next physical-hardware gate PASS or HOLD? **HOLD** - unchanged,
    not part of this phase's scope (spec section 36).
