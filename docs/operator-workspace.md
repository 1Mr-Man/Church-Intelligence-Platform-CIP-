# Unified Operator Workspace (Phase 2.9, per the authoritative Phase 2 roadmap)

Phases 2.0-2.8 built eight independently real, working capabilities - Bible,
Music (deterministic + acoustic), Service, Sermon Foundation, Sermon
Intelligence, Content Intelligence, Cross-Domain Intelligence, and Local
Presentation Display - each with its own panel in `LiveChurchBrain.tsx`.
Phase 2.9 does not add a ninth capability. It makes the eight that already
exist usable together, from one screen, during a live service.

> Phase 2.9 is explicitly **not**: Phase 2.10 validation, church
> management, content generation, social-media automation, full AI/LLM
> integration, OBS/vMix integration, a mobile app, a cloud platform, or a
> new intelligence engine. See the "NOT AVAILABLE" section below for the
> exhaustive list.

## Purpose

Before Phase 2.9, answering "what does CIP think is happening right now"
required reading six-plus separate panels and mentally merging their state.
Phase 2.9 adds three new regions that answer that question directly - a
glance-able header, a bounded "what needs a decision" queue, and a merged
chronological feed - while leaving every existing panel exactly where it
was, fully functional, for diagnostics.

## Architectural audit (step 1/2 of the required process)

`apps/desktop/src/components/LiveChurchBrain.tsx` (2,285 lines before this
phase) is the *only* frontend component - there is no router, no other
page, and `App.tsx` mounts it unconditionally once `isTauriRuntime()` is
true. It contains 18 `<h2>`-headed `<section className="live-brain__panel">`
blocks, each a flat vertical stack (no responsive grid, no `@media` query
anywhere in `LiveChurchBrain.css` before this phase) with ~35 independent
`useState` slices and one large event-subscription `useEffect` wiring
every existing `AppEvent`.

| Panel | Purpose | Data source | Commands | Events | Classification |
| --- | --- | --- | --- | --- | --- |
| Service | Start/pause/resume/end the service | `LiveStatus.service` | `startService`/`pauseService`/`resumeService`/`endService` | (polled) | KEEP |
| Audio & Speech | Device selection, listening control, manual transcript entry | `LiveStatus` | `startListening`/`stopListening`/`processTestTranscript` | `TRANSCRIPT_UPDATED` | KEEP |
| Current Scripture | Active/last reference, context correction | `activeContext`/`lastReference` | `correctScriptureContext` | `SCRIPTURE_DETECTED`/`SCRIPTURE_UPDATED` | KEEP (also surfaced in the new header) |
| Ambiguous Scripture | Resolve an ambiguous detection | `ambiguous` | `resolveAmbiguousReference` | (derived from `SCRIPTURE_DETECTED`) | KEEP |
| Live Transcript | Recent final transcript segments | `transcript` | `listTranscript` | `TRANSCRIPT_UPDATED` | KEEP |
| Pending Suggestions | Bible-domain review queue | `suggestions` | `approveSuggestion`/`rejectSuggestion`/`editSuggestion`/`previewPresentation` | `SUGGESTION_CREATED`/`APPROVED`/`REJECTED`/`EDITED` | KEEP + MERGED (surfaces as the `bible` domain in the new unified feed/attention queue) |
| Approved — Ready to Prepare | Approved-but-not-prepared suggestions | `approvedSuggestions` | `preparePresentation` | `PRESENTATION_PREPARED` | KEEP |
| Service Timeline | Chronological audit log | `timeline` | `listTimeline` | (polled) | KEEP (diagnostics-flavored, but already compact) |
| Manual Bible Search | Ad hoc lookup against the real BSB dataset | (none, direct query) | `searchBible`/`previewScripture`/`createManualPresentation` | - | KEEP (a primary operator tool, not diagnostics) |
| Current Output | Presentation display control | `preparedItems`/`activeDisplayItem`/`displayWindowOpen` | `openPresentationDisplay`/`closePresentationDisplay`/`displayPresentation`/`clearPresentationDisplay`/`cancelPresentation` | `PRESENTATION_STARTED`/`STOPPED`/`PREPARED`/`CANCELLED` | KEEP (also summarized in the new header) |
| Content Registry | Installed Bible/content datasets, import, licensing | `contentItems` | `listContentRegistry`/`setContentEnabled`/`checkBibleDatasetIntegrity`/`importBibleDataset` | - | **MOVED to Diagnostics** |
| Intelligence Status | Which domains have a real engine registered | `intelligenceCapabilities` | `getIntelligenceCapabilities` | - | **MOVED to Diagnostics** |
| Music Intelligence | Recognition findings, acoustic status, current song, search | `musicFindings`/`status.currentSong` | `analyzeMusicTranscript`/`acceptMusicFinding`/`rejectMusicFinding`/`clearCurrentSong`/`searchMusic` | `MUSIC_FINDING_*`/`CURRENT_SONG_CHANGED` | KEEP + MERGED |
| Sermon Foundation | Sermon lifecycle, speaker, section, segment linkage | `sermonFoundation`/`sermonSegments` | `startSermon`/`pauseSermon`/`resumeSermon`/`endSermon`/`setSermonTitle`/`assignSermonSpeaker`/`changeSermonSection`/`linkTranscriptSegmentToSermon` | `SERMON_STARTED`/`PAUSED`/`RESUMED`/`ENDED`/`SECTION_CHANGED`/`SPEAKER_CHANGED`/`METADATA_CHANGED`/`SEGMENT_LINKED` | KEEP (also summarized in the new header) |
| Sermon Intelligence | Theme/structure/point findings | `sermonFindings`/`sermonState` | `analyzeSermonTranscript`/`acceptSermonFinding`/`rejectSermonFinding` | `SERMON_FINDING_*`/`SERMON_STATE_CHANGED`/`THEME_CHANGED`/`STRUCTURE_UPDATED` | KEEP + MERGED |
| Cross-Domain Intelligence | Correlations across domains | `crossDomainCorrelations` | `analyzeCrossDomain`/`reviewCrossDomainCorrelation`/`dismissCrossDomainCorrelation` | `CROSS_DOMAIN_CORRELATION_*` | KEEP + MERGED |
| Content Intelligence | Future-content candidates | `contentCandidates` | `analyzeContentIntelligence`/`acceptContentCandidate`/`rejectContentCandidate` | `CONTENT_CANDIDATE_*` | KEEP + MERGED |
| Service Intelligence | Phase, transitions, anomalies | `serviceIntel`/`serviceTransitions`/`serviceAnomalies` | `analyzeServiceTranscript`/`markServicePhase`/`correctServicePhase`/`acknowledgeServiceAnomaly` | `SERVICE_PHASE_*`/`SERVICE_ANOMALY_*` | KEEP + MERGED (anomalies only - see below) |
| Service History | Past completed services (read-only archive) | `history`/`historyDetail` | `listServiceHistory`/`listTimeline` | - | KEEP (already its own collapsible Show/Hide) |

No panel was found genuinely redundant enough to remove outright - every
one still does something the new unified regions don't replace (manual
transcript entry, search, lifecycle controls, full evidence detail). See
"Consolidation strategy" below for exactly what "MERGED" means.

## Consolidation strategy

Two panels moved into a collapsible `<details className="live-brain__panel">`
"Diagnostics" disclosure, using the exact same `<details>`/`<summary>`
pattern this file already used for "Manual / test transcript entry" etc.:
**Content Registry** and **Intelligence Status**. Both answer "what is
installed" rather than "what is happening right now" - genuinely
diagnostic, rarely touched mid-service, and safe to collapse by default
without losing any capability (a click still reveals them in full).

Every other panel stays exactly where it was, fully expanded, exactly as
tested before this phase - "MERGED" above means the panel's *findings* now
additionally appear in the new unified feed/attention queue (a read
projection), never that the panel itself was removed or its own
accept/reject/preview/prepare controls were taken away.

## New architecture: three regions, zero new backend surface

```
apps/desktop/src/lib/unifiedFeed.ts       - UnifiedIntelligenceItem + buildUnifiedFeed()
apps/desktop/src/lib/attentionQueue.ts    - buildAttentionQueue()
apps/desktop/src/components/workspace/
    WorkspaceHeader.tsx   - region A: glance-able service/sermon/song/scripture/output state
    AttentionQueue.tsx    - region B: bounded, actionable, priority-ordered
    IntelligenceFeed.tsx  - region C: bounded, filterable, chronological, read-only
    IntelligenceCard.tsx  - shared per-item renderer (domain/confidence/assertion/status/evidence)
    actions.ts            - the small action-label vocabulary (no command names)
```

Every one of these is **pure, synchronous, and framework-free where it
matters** (`unifiedFeed.ts`/`attentionQueue.ts` have zero React import) -
built and tested exactly like every existing `lib/*.ts` module in this
codebase (`timelineFormat.ts`, `keyboardShortcuts.ts`, `runtime.ts`), not
introduced as a new kind of thing. `LiveChurchBrain.tsx` builds both via
`useMemo` from state it *already* fetches/subscribes to, and passes a
single `handleUnifiedAction` dispatcher down - **no new Tauri command, no
new event, no new `AppState` field, no new database migration.**

### Why not a bigger aggregate command

Spec section 27 explicitly warns against a `get_unified_workspace_state()`
mega-command. It was never needed here: every value the three new regions
display is already fetched by an existing command or pushed by an existing
event into `LiveChurchBrain`'s own state - the workspace layer only
re-projects state that already exists in the browser, in memory, for free.

## `UnifiedIntelligenceItem`: a frontend-only view-model, never a second backend type

```ts
export interface UnifiedIntelligenceItem {
  id: string;
  domain: "bible" | "music" | "sermon" | "service" | "content" | "correlation";
  summary: string;
  confidence: ConfidenceResult;      // reused, unmodified
  assertionLevel: AssertionLevel;    // reused, unmodified
  rawStatus: string;                 // the literal SuggestionStatus/FindingStatus - never remapped
  needsAttention: boolean;
  createdAt: string;
  detailLine: string | null;         // source text / rule id / candidate type - never fabricated
  evidenceCount: number;
  source: Suggestion | IntelligenceFinding | ContentCandidate | IntelligenceCorrelation;
}
```

`source` always points at the real, original object - nothing is
flattened away. This is the direct implementation of spec rule 8 ("the
operator must be able to answer *why* did CIP produce this"): every card
still shows domain, confidence%, assertion level, raw status, evidence
count, and a detail line, and the underlying object (with its full
`evidence`/`provenance`) is one property away.

### The Bible domain uses `Suggestion`, not `IntelligenceFinding`

No command to list Bible-domain `IntelligenceFinding`s exists -
`analyze_bible_transcript` (Phase 2.8) is a write-only bridge for
cross-domain correlation, never a read path the frontend calls. The real,
already-displayed Bible mechanism is the Phase 1.3 `Suggestion` queue
(`Pending Suggestions`), so that is what the `bible` domain in the unified
feed reuses - not a new fetch, not a fabricated finding.

### Service transitions are never actionable, and the feed says so honestly

`ServiceIntelligenceSummary`'s transitions are a historical log with no
real accept/reject/acknowledge action in the existing UI (only anomalies
have `acknowledgeServiceAnomaly`). `buildUnifiedFeed` takes
`serviceTransitions`/`serviceAnomalies` as two separate inputs precisely so
transitions can be mapped with `needsAttention` always `false` regardless
of their raw `FindingStatus` - claiming otherwise would dangle an action
the operator cannot actually take.

## Region A: `WorkspaceHeader`

Reads directly from `LiveStatus`, `SermonFoundationSummary`,
`ServiceIntelligenceSummary`, the active Scripture context, and the
current presentation output - performs no inference of its own. Shows
"Unknown"/"No active sermon"/"None confirmed" rather than guessing when a
fact genuinely is not yet known (spec rule 5: "Unknown speaker remains
Unknown"; "Unavailable acoustic recognition remains Unavailable").

## Region B: `AttentionQueue`

Every item with `needsAttention === true`, ordered by confidence
descending (the same `ConfidenceResult.score` every panel already shows -
never a new invented priority number), tied-broken by newest-first, then
domain, then id - fully deterministic (spec rule 13), documented in
`attentionQueue.ts`'s own doc comments. Bounded to
`MAX_VISIBLE_ATTENTION_ITEMS = 8` - attention is meant to stay sparse by
design, never become a second copy of the full feed. **High-confidence
information is never hidden for being uninteresting** - a canonical test
(`operatorWorkflow.test.ts`) proves an `Observed`, 1.0-confidence service
finding legitimately outranks a 0.95-confidence inferred correlation.

Each card's action buttons (`actions.ts::actionsFor(domain)`) call exactly
the command the matching per-domain panel already calls, via
`LiveChurchBrain::handleUnifiedAction` - `AttentionQueue`/`IntelligenceCard`
know nothing about command names, only action labels, so the real dispatch
logic lives in exactly one place (spec rule 10).

## Region C: `IntelligenceFeed`

Every unified item (needing attention or not), newest-first, bounded to
`MAX_VISIBLE_INTELLIGENCE_ITEMS = 50`, filterable by domain via a chip row
(`All`/`Bible`/`Music`/`Sermon`/`Service`/`Content`/`Correlations`) -
read-only by design: operator actions live in the Attention Queue and the
existing per-domain panels, not duplicated here.

## Event-driven updates, not polling

Zero new polling was introduced. `unifiedFeed`/`attentionQueue` are
`useMemo`-derived from state that is already updated exclusively by the
existing event subscriptions (`liveEvents.on*`) - a new correlation,
finding, or candidate flows into the unified regions automatically the
same render cycle its own per-domain panel updates, with no additional
IPC call.

## Presentation integration (unchanged, reused)

The workspace does not introduce a second presentation system. `Current
Output` (existing) plus the new header's "Output" field both read
`PresentationItem`/`activeDisplayItem`/`displayWindowOpen` - the same
state, updated by the same `PRESENTATION_PREPARED`/`STARTED`/`STOPPED`/
`CANCELLED` events. The header shows exactly three states -
`CLOSED`/`OPEN, NOTHING DISPLAYED`/`ACTIVE — ON SCREEN` - making
Prepared-≠-Active-≠-Displayed visible at a glance without touching
`presentation.rs`, `presentation_display.rs`, or the renderer.

## BSB dataset and licensing visibility

Unchanged: `Manual Bible Search` and `Current Scripture` still query the
real, already-imported BSB dataset via the existing `searchBible`/
`previewScripture` commands; the (now-collapsed, not removed) Content
Registry panel still displays `licensingStatus` per item exactly as
before. Nothing in this phase touches `core/bible`, the BSB dataset, or
the licensing gate.

## Bounded state

| Bound | Value | Rationale |
| --- | --- | --- |
| `MAX_VISIBLE_INTELLIGENCE_ITEMS` | 50 | Same order of magnitude as `core/intelligence::context`'s own 20-per-domain bounds, scaled up since this merges six domains into one list. |
| `MAX_VISIBLE_ATTENTION_ITEMS` | 8 | Deliberately smaller - attention should stay sparse, not become a second full feed. |

Both bounds are enforced inside the pure functions themselves
(`buildUnifiedFeed`/`buildAttentionQueue`), not left to the caller to
remember to slice.

## Deterministic ordering

Both `unifiedFeed.ts::compareItems` and `attentionQueue.ts::compareByAttentionPriority`
are documented, multi-key comparators (never dependent on object/array
iteration order) - mirroring `core/intelligence::cross_domain::sort_deterministically`'s
own discipline on the Rust side. Proven by `unifiedFeed.test.ts`'s
determinism test (10 repeated calls against the same input produce
identical id order).

## Accessibility

- Every action is a real `<button type="button">`, never a div with a
  click handler - keyboard-reachable via native tab order, no custom
  trap.
- Domain filter chips use `aria-pressed` and a `role="group"` with
  `aria-label` on the container.
- Status/confidence/assertion-level are always plain text
  (`IntelligenceCard`'s `workspace-card__meta` line) - never color-only,
  matching `.live-brain__badge`'s existing convention.

## Responsive behavior

`workspace.css` adds exactly two `@media` rules: a 4-column header grid
above 900px, and a 2-column header grid below 520px (the default
`auto-fit, minmax(180px, 1fr)` grid already reflows reasonably in
between). The card lists and filter chips wrap naturally via flexbox at
any width with no separate breakpoint needed. No operator action is ever
hidden below a breakpoint - `LiveChurchBrain.css` had zero `@media` rules
before this phase; this is the first responsive behavior in the app,
scoped to exactly what changed.

## Failure isolation

Each unified-feed source (`suggestions`, `musicFindings`, ...) is an
independent array already isolated by its own existing fetch/event
handling - one domain's command failing (e.g. Music unavailable) leaves
its array empty, and `buildUnifiedFeed` degrades to simply producing fewer
items for that domain, never throwing. No new failure mode was introduced.

## Web runtime safety

The entire workspace lives inside `LiveChurchBrain`, which `App.tsx` never
mounts when `!isTauriRuntime()` - `WebRuntimeNotice` renders instead, and
none of `unifiedFeed.ts`/`attentionQueue.ts`/the new components ever
executes outside Tauri. Verified via `vite build` + `vite preview` +
headless Chromium: the DOM contains only `WebRuntimeNotice` markup, zero
console errors.

## Performance

Measured directly (`performance.now()`, this machine, a throwaway
benchmark test deleted before commit, matching the established
methodology) - `buildUnifiedFeed` + `buildAttentionQueue` combined, over a
synthetic mix of music/sermon/service findings:

| Items | Feed size (bounded) | Attention size (bounded) | Combined time |
| --- | --- | --- | --- |
| 20 | 15 | 8 | ~0.5ms (first call, includes JIT warmup) |
| 50 | 39 | 8 | ~0.06ms |
| 100 | 50 (bounded) | 8 (bounded) | ~0.17ms |

No O(n²) behavior found - both functions are single-pass map + one sort
each, well under a millisecond at realistic live-service volumes.

## Offline guarantee

Zero new dependencies: `apps/desktop/package.json` and `Cargo.lock` are
both unchanged by this phase (`git diff --stat` on each is empty). Every
new module is plain TypeScript/React using only what the project already
depends on.

## Test counts

- `lib/unifiedFeed.test.ts`: 14 tests (domain mapping, ordering,
  determinism, bounding, no-duplication, assertion-level honesty,
  evidence traceability, needsAttention semantics per domain/status,
  service-transition-vs-anomaly distinction, correlation detail line).
- `lib/attentionQueue.test.ts`: 6 tests (filtering, confidence ordering,
  deterministic tie-breaking, bounding, empty case, purity/no-mutation).
- `lib/operatorWorkflow.test.ts`: 2 tests (the canonical full-service
  scenario across all six domains; resolving an item removes it from
  attention without deleting evidence).
- `components/workspace/actions.test.ts`: 6 tests (the exact action set
  per domain, including the service-domain acknowledge-only case).
- Total new: 28 tests, all passing. Full frontend suite: 179/179 passing
  (151 pre-existing, unmodified, + 28 new).

## PROVEN

- The unified workspace loads and renders alongside every existing panel,
  with zero changes to any existing panel's own logic (only two panels
  gained a `<details>` wrapper, purely presentational).
- Findings from all six domains (Bible/Music/Sermon/Service/Content/
  Correlation) appear in the unified feed with domain identity, evidence
  count, and the real source object preserved.
- Event-driven updates work: the feed/attention queue re-derive
  automatically from existing state changes, with zero new polling or IPC
  calls.
- Operator actions (approve/reject/accept/acknowledge/review/dismiss) from
  the Attention Queue call exactly the same existing commands the
  per-domain panels already use.
- Presentation control (`Current Output`) is unchanged and reused; the new
  header summarizes the same state without a second presentation system.
- The BSB dataset and licensing metadata display exactly as before.
- Responsive layout: verified via CSS review and the header grid's two
  `@media` breakpoints; no action is hidden at any width.
- Web runtime safety: verified via `vite build` + `vite preview` +
  headless Chromium - zero console errors, `WebRuntimeNotice` renders
  correctly, no Tauri IPC attempted.
- Offline operation: zero new dependencies (`Cargo.lock`/`package.json`
  both unchanged).
- Desktop runtime: verified twice under Xvfb - clean startup, 0 migrations
  applied both times, BSB dataset idempotent (0 imported, already
  present), no panics, identical logs across both launches.
- Full Rust regression (`cargo fmt --check`/`check --workspace`/
  `clippy --workspace --all-targets -- -D warnings`/`test --workspace`,
  plus `cargo check -p cip-desktop --features whisper` and
  `cargo test -p cip-ai-speech --features whisper`) all pass unchanged -
  no Rust file was modified this phase.

## NOT AVAILABLE

- No LLM/semantic reasoning of any kind, no generated sermon summaries, no
  automatic social-media publishing, no OBS/vMix/NDI integration, no cloud
  AI, no multilingual expansion - none of this phase's scope, and none
  implemented.
- No second intelligence engine, no second `IntelligenceContext`, no
  engine-to-engine calls - the workspace is a read/dispatch layer only.
- No automatic presentation, preparation, or display triggered by any
  unified feed item or attention-queue entry appearing, at any confidence
  level.
- No component-level (React-rendering) test suite - this codebase has none
  (`vitest` runs in the default `node` environment, no
  `@testing-library/react`/jsdom in `package.json`/`vite.config.ts`); every
  new test here is a pure-logic test over `unifiedFeed.ts`/
  `attentionQueue.ts`/`actions.ts`, matching this project's own established
  testing boundary (mirrors the Rust side's documented decision not to
  stand up `tauri::test::mock_builder()` for `#[tauri::command]`
  functions).
- No new database migration, no new Tauri command, no new `AppEvent`
  variant - none were needed; see "New architecture" above for why.
- No panel was deleted - "Diagnostics" panels are collapsed by default,
  never removed.

## NOT VERIFIED

- Real physical projector/second-monitor behavior - no such hardware is
  attached to this environment; `docs/presentation.md`'s own "NOT
  VERIFIED" section already covers this and is unchanged by this phase.
- Real microphone/live-audio conditions during an actual church service.
- Real multi-monitor operator setups, touch/tablet input specifically
  (the responsive CSS is reviewed and breakpoint-tested via browser
  viewport resize logic in code, not verified against a physical tablet).
- Screen-reader software behavior specifically (semantic HTML/ARIA
  attributes were used correctly per the accessibility section above, but
  no screen reader was run against the live app in this environment).

## Phase 2.10 handoff

Phase 2.9 deliberately did not perform Phase 2.10's job. What it leaves
ready for that phase: a single coherent entry point
(`LiveChurchBrain`/`WorkspaceHeader`/`AttentionQueue`/`IntelligenceFeed`)
that already exercises all eight prior capabilities together in one
render tree, real desktop-runtime idempotency already proven twice, and a
full regression suite (Rust + frontend) already green. Phase 2.10's own
scope - installation, real audio/speech hardware, real church workflow,
security review, full offline behavior audit, failure recovery under real
conditions - remains entirely its own, unclaimed here.

---

# Addendum (Phase 3.5.1) — Visual Design Reference

The architecture above (three regions, zero new backend surface) is
unchanged. This addendum documents the visual design system Phase 3.5.1
applied on top of it, in response to real Windows screenshot evidence
being requested for that phase (see `docs/phase-3-5-1-ux-audit.md` for why
that evidence was unavailable in this environment, and what was audited
instead). Phase 3.5 (between 2.9 and 3.5.1) had already restructured this
same render tree into Operator/Diagnostics modes with `ServiceControlBar`/
`SystemStatusStrip`/`PresentationCard`; that structure is also unchanged
by this addendum - only its visual treatment is.

## A1. Theme

CIP has one fixed, dedicated application theme - not a light/dark toggle
tied to the OS preference. Real production church-AV software
(ProPresenter, OBS, vMix) is dark by convention: operators run it in dim
auditoriums and sound booths, on a laptop next to a much brighter
projector output. A desktop application built for that environment has no
reason to inherit a document viewer's light/dark preference.

Base surfaces (`apps/desktop/src/index.css`):

| Token | Value | Use |
|---|---|---|
| `--bg` | `#0c1120` | Page background |
| `--bg-elevated` | `#131a2e` | Panel/card surface (one step up from page) |
| `--bg-elevated-2` | `#1b2440` | Nested surface (a card inside a card) |
| `--text-h` | `#f4f6fb` | Primary text (headings, values) |
| `--text` | `#a9b2c8` | Secondary text (labels, hints) |
| `--border` / `--border-strong` | `#2a3454` / `#3c4a75` | Card borders |
| `--accent` | `#3b82f6` | Primary button / active toggle - same blue family as the "live" semantic color, so "click this" and "this is in progress" read as one idea |

## A2. Semantic color system

Eight colors, each with exactly one meaning, reused identically everywhere
it appears. Color always accompanies a text label or icon - never the
only signal.

| Color | Token prefix | Meaning | Where it appears |
|---|---|---|---|
| Green | `--status-good` | Ready / connected / active / success | Readiness pills, "On Screen" badge, healthy status dots |
| Blue | `--status-live` | Live / listening / in progress | "● Live" service badge, sermon-domain intelligence cards |
| Amber | `--status-warn` | Needs attention / pending / optional | "Needs Attention" section top border, "Paused" badge, optional-feature notices |
| Red | `--status-bad` | Error / disconnected / failure | Error banners, danger buttons |
| Purple | `--status-intel` | Cross-domain / content / AI-adjacent | Content and Correlation domain badges |
| Teal | `--status-audio` | Audio / live media | Music domain badges |
| Gold | `--status-scripture` | Scripture / presentation emphasis, used selectively | Presentation card border/background, Bible domain badge, prepared-item scripture heading |
| Neutral gray | `--status-neutral` | Idle / informational / structural | Service-domain badges, closed/idle states |

Domain → color mapping (Attention Queue, Intelligence Feed):

- 📖 Bible → gold (`--status-scripture`)
- 🎵 Music → teal (`--status-audio`)
- 🎙 Sermon → blue (`--status-live`)
- ⚙ Service → neutral gray
- 🟣 Content → purple (`--status-intel`)
- 🔗 Correlation → purple (`--status-intel`)

## A3. What changed vs. Phase 3.5

Phase 3.5 built the correct *structure* described in the rest of this
file. Phase 3.5.1 found and fixed why that structure still looked like an
engineering console on a real screen - see `docs/phase-3-5-1-ux-audit.md`
for the full findings. In summary:

1. Removed a leftover Vite/Tauri template shell on `#root` (`text-align:
   center`, a fixed `1126px` width, and a permanent side border) that was
   centering all body text and clamping the entire app to a narrow column
   regardless of screen size - Phase 3.5's own layout widening had been
   silently clamped by this the whole time.
2. Replaced the light/dark doc-site theme with CIP's own professional dark
   theme (above).
3. Gave every intelligence domain a real, consistent color + icon instead
   of a `currentColor`-bordered pill that inherited whatever gray was
   nearby.
4. Trimmed `WorkspaceHeader` from a raw ALL-CAPS `<dl>` duplicating five
   other panels (and leaking terms like "ACOUSTIC" and a raw backend
   error string into Operator Mode) down to the three facts nothing else
   on screen shows: service phase, active sermon/speaker, and confirmed
   current song - rendered as the same status-pill language as the rest
   of the screen.
5. Gave panels an elevated surface color distinct from the page
   background, added a three-tier button hierarchy (primary/secondary/
   tertiary), and made the Presentation card visually dominant
   (gold-tinted border/background, larger heading).

## A4. Backend contract preservation

Every change in this addendum is CSS, or a presentational trim of one
React component (`WorkspaceHeader`) using only props it already received.
No Tauri command, no Tauri event, no database schema, no intelligence
engine, and no presentation-safety rule changed. See
`pilot-evidence/3.5.1/software/` for the automated proof (`git diff`
scoped entirely to `apps/desktop/src/**` and `docs/**`).

## A5. Evidence status

Every visual claim in this addendum is `PROVEN_AUTOMATED` (build succeeds,
tests pass) or based on direct source-code reading of the CSS/JSX now in
the tree - not `VERIFIED_WINDOWS` or `VERIFIED_PHYSICAL_HARDWARE`. No
screenshot or video of this environment's output has been captured or
reviewed, because no screenshot tool is available in this container and
Xvfb is explicitly not a UX evidence source per this project's own rules.
See `docs/phase-3-5-1-ux-audit.md` section 0 and the Phase 3.5.1 gate in
`docs/phase-3-5-operator-ux.md` for the complete, honest evidence
classification.
