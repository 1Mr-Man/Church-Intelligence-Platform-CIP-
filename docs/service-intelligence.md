# Service Intelligence (Phase 2.4, per the authoritative Phase 2 roadmap)

## Roadmap note

This repository's Phase 2 roadmap is:

```
2.0 Intelligence Architecture -> 2.1 Unified Intelligence Event/Context Layer -> 2.2 Music Content Foundation ->
2.3 Music Intelligence -> 2.4 Service Intelligence (THIS PHASE) -> 2.5 Sermon Intelligence Foundation ->
2.6 Sermon Intelligence -> 2.7 Content Intelligence -> 2.8 Cross-Domain Intelligence ->
2.9 Unified Operator Intelligence Workspace -> 2.10 Full Phase 2 Validation
```

An earlier body of work in this repository's history - `core/intelligence/src/cross_domain.rs`,
`apps/desktop/src-tauri/src/cross_domain.rs`, and `docs/cross-domain-intelligence.md` - was
built and committed under an internal label that also read "Phase 2.4." That label is a
historical commit/doc artifact and is **not** rewritten by this document or this phase: the
code, tests, and docs for that earlier work are unchanged and remain fully intact. Under the
authoritative roadmap above, that functionality belongs to the future, formal **Phase 2.8
Cross-Domain Intelligence**, where it will be validated and integrated alongside Sermon,
Content, and Service Intelligence. See "Future integration with Phase 2.8" below.

This document describes what "Phase 2.4" means under the authoritative roadmap: **Service
Intelligence** - understanding what part of a live church service is happening, from
transcript evidence alone.

## 1. Purpose

Service Intelligence answers one question a live operator repeatedly needs answered without
looking away from the transcript: **what stage of the service is happening right now?**
Opening, worship, prayer, Scripture reading, sermon, offering, announcements, or closing -
inferred deterministically from spoken trigger phrases, the same way `core/sermon` infers
sermon structure from spoken cues. It never guesses at meaning, never fabricates a transition
with no textual evidence, and never blocks or overrides an operator's own judgment - it only
surfaces an inference the operator can accept as-is, ignore, or correct.

## 2. Architecture

```text
TranscriptSegment (final, live or manual test harness)
        |
        v
IntelligenceContext.service_status  (already built by the Tauri layer - unchanged)
        |
        v
ServiceIntelligenceEngine::analyze(&input, &context)  [core/intelligence/src/service_adapter.rs]
        |  deterministic phrase-cue detection + debounce/hysteresis
        v
IntelligenceFinding (domain: Service, kind: ServiceState)
        |
        v
FindingQueue  (operator review: same accept/reject lifecycle every domain already has)
        |
        v
Service Intelligence panel (read-only current-phase display + operator mark/correct/acknowledge)
```

`ServiceIntelligenceEngine` implements the same [`IntelligenceEngine`] trait every other
engine (Bible/Music/Sermon) implements, and is orchestrated the same way: a Tauri-agnostic
module (`apps/desktop/src-tauri/src/service.rs`, mirroring `sermon.rs`) sits between the pure
engine and the Tauri commands, with no engine-to-engine calls anywhere in the path.

## 3. Lifecycle vs. phase

`cip_core_service::ServiceStatus` (`Started`/`Paused`/`Ended`) already answers **"is a service
running at all"** and is untouched by this phase. `ServicePhase` (this phase's own new type)
answers a completely different question: **"what part of the running service is happening."**
The two are independent axes that are never conflated:

- A service can be `Started` and in phase `Unknown` (nothing has been said yet that matches a
  cue).
- Phase inference is deliberately suspended whenever `context.service_status` is anything
  other than `Started` - `ServiceIntelligenceEngine::analyze` returns no findings at all while
  paused or ended, so a phase can never silently "advance" while nobody is speaking into a live
  service.

## 4. Service state model

`ServicePhase` (`core/intelligence/src/service_adapter.rs`) is a 9-variant enum:

`Unknown`, `Opening`, `Worship`, `Prayer`, `ScriptureReading`, `Sermon`, `Offering`,
`Announcement`, `Closing`.

`Unknown` is the honest starting state and a legitimate ongoing state - it is never silently
defaulted to `Opening` just because a service started. The engine's own internal
`EngineState` (current phase, when it started, the previous phase, a running transition
counter, and a pending weak-cue candidate) is the only mutable state this phase introduces; it
lives entirely inside `ServiceIntelligenceEngine`, `Mutex`-guarded exactly like
`SermonIntelligenceEngine`'s and `MusicIntelligenceEngine`'s own accumulating state.

This is deliberately a smaller set than every phase a real service could contain - no
`Communion`, `AltarCall`, or generic `Transition` phase. See "NOT AVAILABLE" below for why.

## 5. Evidence

Every phase transition traces to one of exactly two evidence kinds, both already defined by
Phase 2.0's `EvidenceSource`:

- **`EvidenceSource::Transcript`** - a verbatim matched phrase from a real transcript segment
  (e.g. `"let us pray"`, `"turn with me to"`). The `matched_phrase` recorded on the finding is
  always a literal substring of the segment text - never paraphrased, never invented.
- **`EvidenceSource::OperatorAction`** - an explicit operator "mark" or "correct" action.

Nothing in this phase invents a third evidence kind, and nothing here calls another engine for
evidence - cue detection reads only the current transcript segment's own text.

## 6. Findings

Every Service Intelligence output is an ordinary `IntelligenceFinding` - `domain: Service`,
`kind: ServiceState` (both enum variants have existed, unused, since Phase 2.0). No new finding
type was introduced. Three shapes of `ServiceState` finding exist, distinguished only by their
summary text (never by a new field):

- `"Service phase changed #<n>: <FROM> -> <TO>"` - a detected or operator-driven transition.
- `"Anomaly #<n>: unexpected service phase transition <FROM> -> <TO>"` - see "Anomaly
  detection" below.
- The operator-action variant of the first shape additionally appends `"(operator marked)"` or
  `"(operator corrected)"`.

Every summary embeds the engine's own running `transition_count` as `#<n>`. This is not
decoration: it is what keeps `FindingQueue::add`'s summary-based equivalence check from
silently dropping a later, genuinely recurring transition (e.g. Sermon -> Prayer -> Sermon ->
Prayer) while an earlier, textually-identical transition is still unresolved in the queue.

## 7. Confidence

- A **Strong** cue transition (e.g. `"let us pray"`) is `AssertionLevel::Inferred` with
  `ConfidenceSource::Heuristic`, score `0.85` - specific enough to trust on a single occurrence,
  but still explicitly an inference, never claimed as certain.
- A **Weak** cue transition (confirmed only after `WEAK_DEBOUNCE_STREAK` repeats) is
  `AssertionLevel::Suggested`, score `0.6` - the lowest-confidence category this codebase
  defines, reflecting that the evidence is genuinely thinner.
- An **anomaly** finding is `AssertionLevel::Inferred`, score `0.5`.
- An **operator mark/correct** finding is `AssertionLevel::Observed`, `ConfidenceSource::Human`,
  score `1.0` - an operator's own statement of the service's phase is a direct observation, not
  an inference.

`IntelligenceDomain::Service`'s pre-existing `baseline_priority()` hook (reserved since Phase
2.0) automatically grants `ServiceState` findings `High` priority, except in the Low confidence
bucket - no new priority logic was written this phase.

## 8. Transition detection

`detect_phase_cues` scans a fixed, 19-entry static table of phrase-anchored regular
expressions (`static PHASE_CUES`, mirroring `core/sermon::detection`'s
`LazyLock<Regex>` + macro-generated table idiom exactly) against the current segment's text.
Every cue requires an explicit trigger phrase - "let us pray," "turn with me to," "our
offering," "may the grace" - never a purely statistical or keyword-frequency heuristic. When a
segment matches more than one cue, the first `Strong` match (in table order) wins; if there is
no `Strong` match, the first `Weak` match is used. A segment matching nothing produces no
finding and no error.

## 9. Hysteresis / debounce

Every cue is tagged `CueStrength::Strong` or `CueStrength::Weak`:

- A **Strong** cue (specific and unambiguous) transitions immediately on a single occurrence -
  the same way an explicit operator action is trusted immediately.
- A **Weak** cue (a bare, more easily-coincidental word, e.g. `"worship"` or `"good morning"` on
  their own) only transitions once the *same* candidate phase has been cued
  `WEAK_DEBOUNCE_STREAK` (`2`) consecutive times. A differing weak cue, or the current phase
  being reconfirmed, resets the pending candidate. This means a single stray mention of a word
  like "worship" inside a sermon illustration is never enough on its own to flip the phase -
  the false-positive acceptance scenario (below) is satisfied by phrase-precision alone, and
  this debounce mechanism is the second, independent layer of protection for the weaker cues.

## 10. Anomaly detection

`classify_transition(from, to)` classifies every transition as `Expected` (the immediate next
step in the conventional Opening -> ... -> Closing order, or staying in place), `Possible`
(skipping ahead, or either side `Unknown`), or `Unexpected` (a real regression to an earlier
phase). This classification **never blocks** a transition - every transition this engine
detects, or an operator asserts, is always accepted regardless of plausibility. An `Unexpected`
transition additionally emits a second finding (the `"Anomaly #<n>: ..."` shape) so an operator
can review it; it does not replace or suppress the ordinary transition finding.

**Transcript staleness** is a second, separate anomaly signal, but a deliberately different
one: it is wall-clock-dependent, so it is computed entirely outside the deterministic engine
(see section 13, "Service health"), and it is **never** a reason to automatically end or pause
the service - only an informational status an operator can see.

## 11. Operator correction

Two Tauri commands let an operator act directly, bypassing debounce entirely and always
producing an `Observed`, confidence-`1.0` finding:

- **`mark_service_phase(phase, note?)`** - "the service is now in this phase" (e.g. after a
  visual cue with no matching transcript phrase). Transitions immediately.
- **`correct_service_phase(phase, note?)`** - the same mechanism, but additionally *rejects*
  (never deletes) every other still-pending transition finding for the current service before
  queuing the correction. The superseded finding's own status becomes `Rejected`; it remains
  fully present and auditable via `list_service_transitions`, which includes rejected
  transitions. This is how the system's own last inference is superseded without ever being
  erased from the record.

## 12. Timeline

Every service-phase transition, correction, and anomaly acknowledgment is recorded through the
same `timeline::record_event` / `audit_events` table every other domain already uses - no new
timeline mechanism, no new table.

## 13. Service health

"Is the transcript still updating" is answered by `transcript_freshness`
(`apps/desktop/src-tauri/src/service.rs`), a pure function of two explicit timestamps -
`Option<last_transcript_at>` and `now` - that never calls `Utc::now()` itself. The real caller
(`get_service_intelligence_state`) supplies `now` from `Utc::now()` at the moment of the
command call; every test supplies an explicit value instead, keeping the function itself fully
deterministic and testable. Three states: `Unknown` (no final segment received yet this
service), `Fresh`, and `Stale { secondsSince }` (no final segment for
`TRANSCRIPT_STALE_AFTER_SECONDS` = 30 seconds or more). `last_transcript_at` is updated in
exactly one place: the real, live-audio final-segment success path in `commands.rs` - never by
the manual/test-transcript harness (`analyze_service_transcript`), so staleness reflects actual
microphone activity, not test calls. Staleness is purely informational and **never**
automatically ends, pauses, or otherwise mutates the service.

## 14. Persistence

No new database table, migration, or persisted state was added. `ServicePhase` state lives only
in the in-process `ServiceIntelligenceEngine` (reset on app restart, same as
`SermonIntelligenceEngine`/`MusicIntelligenceEngine`'s own accumulating state); findings live
only in the existing in-memory `FindingQueue` (Phase 2.0 spec preference: persistence only when
clearly justified, and nothing here yet needs to survive a restart). Every transition is,
however, durably recorded in `audit_events` via the existing timeline mechanism, so the history
of what happened is not lost even though the live "current phase" pointer itself is not
persisted.

## 15. IPC (Tauri commands)

All seven are new this phase, following the exact naming/shape conventions `sermon.rs`/
`music.rs` established:

| Command | Purpose |
|---|---|
| `analyze_service_transcript(text)` | Manual/test-mode harness - persists `text` as a transcript segment and runs it through the real, accumulating `AppState.service_engine`. |
| `get_service_intelligence_state()` | Returns `ServiceIntelligenceSummary` (current phase, when it started, previous phase, transition count, transcript freshness). Read-only. |
| `list_service_transitions()` | All transition findings (including rejected/superseded ones) for the current service. |
| `list_service_anomalies()` | Pending anomaly findings for the current service. |
| `mark_service_phase(phase, note?)` | Operator marks the current phase. |
| `correct_service_phase(phase, note?)` | Operator corrects the current phase, superseding pending transitions. |
| `acknowledge_service_anomaly(findingId)` | Operator acknowledges (accepts) an anomaly finding. |

`analyze_service_transcript` is manual/test-mode only, exactly like the equivalent Bible/Music/
Sermon/Cross-Domain commands before it - `pipeline.rs::handle_final_transcript` (the real live-
audio path) is untouched except for the one line that records `last_transcript_at` (section 13).

## 16. Events

Four new `AppEvent` variants, each with a distinct wire name (locked in by a distinctness test
in `events.rs`): `ServicePhaseChanged`, `ServicePhaseCorrected`, `ServiceAnomalyDetected`,
`ServiceAnomalyAcknowledged`. All four carry an `IntelligenceFinding` payload, serialized
camelCase, the same shape every other domain's finding events already carry.

## 17. Frontend

`LiveChurchBrain.tsx`'s "Service Intelligence" section is read-only for system-detected state
(current phase, previous phase, transcript freshness) plus operator-actionable controls: a
phase selector with "Mark" and "Correct" buttons, a list of pending anomalies each with an
"Acknowledge" button, and a list of recent transitions. Nothing in this panel auto-navigates,
auto-presents, or auto-approves anything - every state change requires either new transcript
evidence flowing through the existing pipeline or an explicit operator click.

## 18. Offline guarantee

The only new dependency this phase introduced is `regex` (already used elsewhere in this
workspace by `core/sermon`, added to `core/intelligence/Cargo.toml` as
`regex.workspace = true`) - a pure, local pattern-matching library with no network I/O.
`cargo tree -p cip-core-intelligence` and `cargo tree -p cip-desktop` both confirm zero
network-related crates (`reqwest`, `hyper`, `native-tls`, `rustls`, `curl`, and similar are all
absent from both trees). Service Intelligence performs no HTTP calls, no cloud transmission of
transcript text, and no external analytics of any kind - it is local-first and fully
functional offline, like every other domain in this codebase.

## 19. Performance

A throwaway release-mode benchmark (written, measured, then deleted before commit, per this
phase's own process convention) called `ServiceIntelligenceEngine::analyze` at 20, 100, and
1000 synthetic transcript segments against a single accumulating engine instance. Observed
per-call cost stayed flat at roughly 2-3 microseconds regardless of how many segments had
already been processed (the first small batch's average was dominated by one-time regex
compilation warm-up, not by growth). This is expected by construction: `analyze` scans a fixed
19-entry cue table and mutates only a handful of scalar fields (`current_phase`,
`phase_started_at`, `previous_phase`, `transition_count`, an `Option<(ServicePhase, u32)>`
weak-candidate) - nothing in `EngineState` grows with the number of segments processed, so
there is no O(n) or worse per-call cost to find. `core/intelligence/src/service_adapter.rs`
also keeps a permanent (non-throwaway) `ten_thousand_segments_never_exhaust_memory_or_break_analysis`
test proving the same absence of unbounded growth as part of the regular suite.

## 20. PROVEN

- Deterministic, phrase-anchored phase-transition detection over live or manual transcript
  segments, with every transition traceable to a verbatim matched phrase or an explicit
  operator action.
- Strong/weak cue debounce that prevents a single stray or ambiguous mention from flipping the
  phase.
- Phase inference correctly suspended while the service is not `Started`.
- Backward ("unexpected") transitions are flagged for review, never blocked.
- Transcript staleness is tracked and surfaced without ever auto-ending or auto-pausing the
  service.
- Operator mark/correct workflow, including correction superseding (rejecting, never deleting)
  prior pending transitions.
- Anomaly acknowledgment reusing the existing `FindingQueue` accept lifecycle.
- Full IPC/event/timeline/frontend integration with no engine-to-engine calls.
- O(1)-per-segment analysis cost, measured directly; fully offline (no new network-capable
  dependency).

## 21. NOT AVAILABLE / NOT VERIFIED

- Only 8 real phases (Opening through Closing) plus `Unknown` are modeled. `Communion`,
  `AltarCall`, and a generic `Transition` phase were considered and deliberately left out: no
  reliable, low-false-positive deterministic phrase cue was designed for them, and inventing
  one without real transcript evidence to validate against would violate this phase's
  evidence-based discipline. A future phase can add them once real cue phrases are validated.
- Cue phrases are English-only and tuned to a fairly conventional evangelical/Protestant
  service order; a service with a substantially different structure or vocabulary may see more
  `Unknown` time or more `Unexpected` anomalies than a canonical service would.
- No semantic/statistical/ML phase classification exists or is planned as part of this phase -
  see "Deterministic-first" framing throughout this document.
- No cross-domain correlation (e.g. "the sermon phase's content matches this Bible finding") is
  implemented here - that is explicitly out of scope for Phase 2.4 and reserved for Phase 2.8
  (next section).

## 22. Known limitations

- A single transcript segment containing cues for two different phases resolves to only one
  cue (Strong-first, then first-in-table-order) - the other cue in that segment is silently not
  acted on. This mirrors `core/sermon`'s own single-classification-per-segment behavior and is
  an accepted, documented simplification rather than an oversight.
- The weak-cue debounce candidate is a single `Option<(ServicePhase, u32)>` - only one
  candidate phase is tracked at a time, so two different weak cues alternating segment-to-
  segment will never accumulate a streak for either (each resets the other). This is the
  intended conservative behavior, not a bug.
- `ServiceIntelligenceEngine`'s state is in-process only (see "Persistence") - restarting the
  application resets the current phase to `Unknown`, even though the transition history remains
  in the timeline/audit log.

## 23. Future integration with Phase 2.8 Cross-Domain Intelligence

The existing `crate::cross_domain` correlation engine (see `docs/cross-domain-intelligence.md`)
already reads `IntelligenceContext.recent_findings` from every domain to derive correlations.
Service Intelligence's `ServiceState` findings are ordinary findings in that same
`recent_findings` context, so no new integration code is required for Cross-Domain
Intelligence to eventually correlate a service phase with, e.g., a sermon point or a Bible
reference detected in the same window - that correlation logic itself belongs to the future,
formal Phase 2.8 validation and is intentionally not built here.
