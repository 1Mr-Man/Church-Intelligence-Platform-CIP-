# Phase 3.8.1 — Audit

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `9eb1ea2` (Phase 3.8, "Offline Service Replay + Professional
  Church Operator Workspace")
- Working tree at start: clean

This audit was written before any Phase 3.8.1 code changed, per this
phase's own "audit first" requirement (section 1).

## B. What the user actually observed

Tested on a real HP EliteBook (Windows), with a real ~52-minute sermon
transcript (Pastor Poju Oyemade, WOFBEC 2026, 1st Session):

1. Service Replay loads, labels itself correctly, transcript import works.
2. **The transcript is reduced to only 2 segments.**
3. Activity Log shows replay occurring, but **no Bible detections, sermon
   insights, topic/theme, key points, attention items are visibly shown**
   anywhere on the Service Replay screen.
4. Environment/database diagnostics are visible but the intelligence
   experience is not representative of a real service.

## C. Runtime trace: transcript → UI

Traced end-to-end by reading the actual code (not guessed):

1. **Import**: `ServiceReplay.tsx`'s `onFileSelected` reads the file with
   `FileReader.readAsText()` into `transcriptText` state. Unchanged,
   correct, no defect found here.
2. **Segmentation**: `startReplay()` calls `segmentTranscript(transcriptText)`
   (`replay.ts`). This is where defect #1 lives — see section D.
3. **Scheduling**: `playLoop()` iterates `segments` sequentially, calling
   `processReplaySegment(text)` for each one, then sleeping
   `delayForSpeed(speed)` before advancing. This part is sequential and
   correct — confirmed by the existing Phase 3.8 acceptance test
   (`phase_3_8_service_replay_full_offline_acceptance`), which is
   unaffected by either defect below and remains valid.
4. **processReplaySegment(text)** calls, in order:
   `commands.processTestTranscript(text)` → `commands.analyzeBibleTranscript(text)`
   → `commands.analyzeSermonTranscript(text)`.
   - **`process_test_transcript`** (`commands.rs:1166`) runs
     `handle_final_transcript` (the same Bible Suggestion path live audio
     uses), persists the transcript segment, and **returns**
     `ProcessedSegment { serviceId, detections, suggestions }`. It also
     **emits** `TranscriptUpdated` for the segment and, via
     `emit_processed_segment_events`, `ScriptureDetected`/`ScriptureUpdated`
     per detection and **`SuggestionCreated`** per suggestion (each also
     recorded to the timeline).
   - **`analyze_bible_transcript`** (`commands.rs:2460`) runs the Bible
     Finding path (`BibleIntelligenceEngine`) and **returns**
     `Vec<IntelligenceFinding>`.
   - **`analyze_sermon_transcript`** (`commands.rs:3088`) runs
     `sermon::analyze_and_queue` against the real, persistent
     `state.sermon_engine` (accumulated across every call for the active
     service — confirmed by reading `before`/`after` snapshot diffing at
     lines 3130-3155), **returns** `Vec<IntelligenceFinding>`, and
     **emits** `SermonFindingDetected` per finding plus
     `SermonStateChanged` / `SermonThemeChanged` / `SermonStructureUpdated`
     whenever the accumulated sermon state actually changes.
5. **`processReplaySegment`** (`ServiceReplay.tsx:274-287`) awaits all
   three calls and then does exactly one thing with every result:
   `appendLog(...)` — a single string appended to a local "Activity Log."
   **The return values (`ProcessedSegment`, `IntelligenceFinding[]` ×2) are
   discarded.** No component subscribes to `SuggestionCreated`,
   `ScriptureDetected`, `SermonFindingDetected`,
   `SermonStateChanged`/`SermonThemeChanged`/`SermonStructureUpdated`, or
   any other live event anywhere in `ServiceReplay.tsx`.
6. **Where results ARE actually displayed**: `LiveChurchBrain.tsx` (the
   "Live Service" tab, `App.tsx`'s `section === "live"`) already does all
   of this correctly — it fetches `suggestions`/`sermonFindings`/
   `sermonState`/etc. when a service becomes active, subscribes to every
   one of the events named above via `liveEvents.on*`, derives a
   `unifiedFeed`/`attentionQueue` (`lib/unifiedFeed.ts`,
   `lib/attentionQueue.ts`), and renders them via the existing
   `WorkspaceHeader`/`SystemStatusStrip`/`AttentionQueue`/`IntelligenceFeed`/
   `PresentationCard` components (`components/workspace/`).
7. **Root cause of finding #3 (missing intelligence display) — confirmed,
   not guessed**: `App.tsx` renders "Live Service" and "Service Replay" as
   two mutually exclusive tabs (`AppSection = "live" | ... | "replay"`,
   `{section === "live" && <LiveChurchBrain />}` /
   `{section === "replay" && <ServiceReplay />}`). While an operator
   watches Service Replay run, they are necessarily NOT looking at the one
   screen that already renders Bible/Sermon/Attention/Presentation
   correctly. This is **category "UI not subscribing to the relevant
   state/event"** from the spec's own diagnostic list (section 1.4) —
   nothing is broken on the backend; the intelligence is genuinely
   generated, persisted, returned, and emitted (steps 4-5 above all
   verified true), it is simply never rendered on the screen the operator
   is looking at during replay.

## D. Root cause of the 2-segment bug — confirmed, not guessed

`segmentTranscript` (`replay.ts`, Phase 3.8):

```ts
export function segmentTranscript(text: string): string[] {
  const paragraphs = text.split(/\n\s*\n/)...filter(...);
  if (paragraphs.length > 1) return paragraphs;   // <-- returned AS-IS, unbounded size
  ... sentence-fallback only when paragraphs.length <= 1 ...
}
```

If a real transcript file has more than one blank-line-delimited block —
even just two, each possibly containing the entire first/second half of a
52-minute sermon — the function returns those blocks verbatim as the
segments, with **no upper bound on segment size** and no further
splitting. This is exactly the reported "2 segments" behavior: the
transcript happened to contain exactly 2 blank-line breaks (a common
artifact of how transcripts get exported/pasted — most line breaks are
single newlines within a block, with only one or two genuine paragraph
gaps). The single-huge-paragraph sentence-fallback path was never reached
because `paragraphs.length` was `2`, not `≤ 1`.

This is a **transcript segmentation defect** (category confirmed, per
section 1.4's diagnostic list), not a timing, sequencing, or
result-surfacing defect — segmentation happens once, synchronously, before
`playLoop` starts.

## E. Confirmed: not a second pipeline, not stale state, not a timing bug

Checked and ruled out:

- **Command sequencing**: correct — `processTestTranscript` →
  `analyzeBibleTranscript` → `analyzeSermonTranscript`, awaited strictly
  in order per segment, matching the existing Phase 3.8 acceptance test.
- **Replay timing**: correct — `playLoop` awaits one segment fully before
  sleeping and advancing; unaffected by either defect.
- **Analysis only triggered after replay finishes**: false — each of the
  three calls already runs once per segment, during replay, not after.
- **Stale frontend state**: not applicable — `ServiceReplay.tsx` holds no
  intelligence state at all today (that is the defect, not staleness of
  existing state).
- **A second/duplicate intelligence pipeline was never created in Phase
  3.8** and none is needed now — every command already used is real,
  pre-existing, and correct.

## F. Gap register

| # | Gap | Category | Fix location |
|---|-----|----------|---------------|
| 1 | 2-segment collapse on real transcripts | Transcript segmentation | `replay.ts` — bound every paragraph's chunk size, add optional timestamp-cue parsing |
| 2 | No Bible/Sermon/Attention/Presentation display during replay | UI not subscribing to existing state/events | `ServiceReplay.tsx` — mount the same `commands.list*`/`liveEvents.on*` subscriptions `LiveChurchBrain.tsx` already uses, render via the existing `components/workspace/*` components |
| 3 | Generic error text on "service already active" | UX (not a defect, an explicit ask, section 10) | `ServiceReplay.tsx` — detect this specific error and offer "End Service" inline |
| 4 | Diagnostics (DB path, migration count, engine internals) visible on the primary screen | UX (section 8) | Confirm these are already Diagnostics-only in `LiveChurchBrain.tsx`'s `mode === "diagnostics"` gate — verify `ServiceReplay.tsx` never surfaces them (it does not; it only shows Bible/Speech/Mic readiness words, no raw paths) |

No new Tauri command, no new database migration, no new intelligence
engine is justified by any of the above — every gap is closed either in
`replay.ts` (pure segmentation logic) or `ServiceReplay.tsx` (reusing
existing commands, existing events, and existing presentational
components from `components/workspace/`). This is confirmed further in
section G below.

## G. Architectural safety — diff against backend surface (pre-implementation)

```
git diff 9eb1ea2 -- apps/desktop/src-tauri/capabilities/ apps/desktop/src-tauri/tauri.conf.json   -> (to be re-confirmed empty after implementation)
git diff 9eb1ea2 -- apps/desktop/src-tauri/src/lib.rs                                             -> (to be re-confirmed empty after implementation)
git diff 9eb1ea2 -- apps/desktop/src-tauri/src/events.rs apps/desktop/src/events/                 -> (to be re-confirmed empty after implementation)
git diff 9eb1ea2 -- core/intelligence/ presentation/renderer/ core/presentation/                  -> (to be re-confirmed empty after implementation)
git diff 9eb1ea2 -- database/migrations/ database/src/migrations.rs                                -> (to be re-confirmed empty after implementation)
```

Expected result: all empty. The plan below requires zero backend changes;
only the Rust acceptance test file (`pipeline.rs`, test code only, not
engine/command code) is expected to change, plus the frontend.

## H. Implementation plan

1. **`replay.ts`**: change `segmentTranscript` to return a small
   `ReplaySegment { sequence, timestampLabel, text }[]` structure (spec
   section 3's required fields; `source`/session identity are attached by
   the component at the point of use, not by this pure function). Add
   optional timestamp-cue-line parsing (recognizes a line that consists
   *only* of one or two timecodes, `HH:MM:SS[.,mmm]`, optionally bracketed
   and/or separated by `-->`/`-`/`–`/`—` — a common transcript-export
   convention) as segment boundaries when at least two such cue lines are
   present. Otherwise, fall back to paragraph splitting, but — the actual
   fix — every paragraph (whether there is 1 or many) is now split into
   sentences and re-grouped into bounded-size chunks (a few sentences,
   capped at a sensible character budget), never returned as one
   unbounded block and never split down to a single sentence per segment
   for a long paragraph.
2. **`ServiceReplay.tsx`**: mount the same read model
   `LiveChurchBrain.tsx` already uses for the active service — pending +
   approved suggestions, sermon findings + sermon state, cross-domain
   correlations, content candidates, prepared items, active display
   item/window state, service transitions/anomalies, service status — via
   the same `commands.list*`/`commands.get*` calls and the same
   `liveEvents.on*` subscriptions, and derive the same
   `unifiedFeed`/`attentionQueue` via the existing, unmodified
   `lib/unifiedFeed.ts`/`lib/attentionQueue.ts`. Render via the existing
   `WorkspaceHeader`, `SystemStatusStrip`, `AttentionQueue`,
   `IntelligenceFeed`, and `PresentationCard` components — the same
   components, same props, same action dispatch pattern already proven in
   `LiveChurchBrain.tsx`. Add "elapsed" and "currently hearing" (the last
   replayed segment's text) as replay-local state. Add an explicit
   "Source: SERVICE REPLAY" indicator. `LiveChurchBrain.tsx` itself is not
   modified — this is additive-only, avoiding any regression risk to the
   already-tested Live Service screen.
3. Detect the specific `"a service is already active - end it before
   starting a new one"` error text in `startTestService`/`startReplay` and
   render an inline recovery affordance (an "End Service" button) instead
   of only the raw error string.
4. New Rust acceptance test in `pipeline.rs`:
   `phase_3_8_1_service_replay_progressive_intelligence_acceptance` — a
   longer, synthetic, multi-topic transcript (not the user's real
   copyrighted transcript, which was not supplied verbatim — only a list
   of the Scripture references it contains) fed as many (15+) sequential
   segments, proving: real Bible/Sermon findings accumulate progressively
   across many calls (not just the previous test's 4), sermon state
   persists and evolves correctly across a longer sequence, findings
   retain full provenance, and nothing is hardcoded — the test never
   asserts a specific reference was detected merely because the fixture
   text says so; it asserts detection against the real BSB dataset only
   for references the deterministic detector actually recognizes.
5. New `replay.test.ts` cases: timestamp-cue segmentation, the specific
   "few large paragraphs must not collapse to a handful of giant segments"
   regression (the literal bug reported), bounded chunk size, and the
   existing edge cases (empty input, no terminal punctuation) retained.
6. Full regression, Windows/Linux rebuild, docs, evidence, commit — same
   discipline as every prior phase.

Proceeding to implementation as scoped above.
