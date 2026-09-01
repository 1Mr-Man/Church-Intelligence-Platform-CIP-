# Phase 4.3 — Instant Bible Detection (Fast Detection Lane)

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `6e825c9` (Phase 4.2, live Bible detection performance + search filter)

## Why this phase exists

The operator shared two screen recordings of a competitor product
("Pewbeam") reading Scripture aloud live: a reference like "Genesis
chapter two verse six" is recognized and the verse is on screen within
a couple of seconds of the sentence finishing, with a live transcript
panel updating in near real time. The operator's reaction: "CIP should
be much better than this, we have put more effort than the results
Whisper is giving me" - and, separately, pasted a large third-party
analysis recommending paid cloud streaming ASR (Deepgram, AssemblyAI,
Google/Azure, Gemini Live) and hosted LLM key-point extraction (GPT-
4o-mini, Claude Haiku, Groq-hosted Llama). The operator's own explicit
constraint closed that door: **CIP must run on a free, local model, not
a monthly-subscription API** - consistent with this project's offline-
first architecture (see `docs/README.md`/every prior phase's "offline
dependency proof").

## What the videos actually showed (verified, not assumed)

Neither video is CIP - both are the competitor "Pewbeam" (one a live
Scripture-reading demo, one a slide-editor feature promo). Frame
extraction (`ffmpeg -vf fps=1`) of the demo showed:

- The reader states the reference explicitly and briefly ("Genesis
  chapter two verse six.", "Matthew chapter one verse eight.") - short,
  clean utterances, not continuous freeform sermon speech.
- A "Recent detections" panel surfaces candidates with a confidence
  score and explicit "+ Add to queue"/"Present" controls - an operator
  review step exists, it is just fast.
- The live transcript panel updates in small, frequent increments, not
  in large multi-sentence blocks.

This rules out "a smarter model reads minds before the sentence
finishes" - the reference is fully spoken before it's recognized. It
points at low per-utterance latency (fast recognition of a short
utterance, acted on immediately), not a fundamentally different
recognition strategy.

## Root cause: CIP had two independent latency sources

1. **Whisper's own buffering/inference** (`ai/speech/src/whisper.rs`):
   audio is buffered to a fixed ~3.0s window before one `full()` call
   runs; Phase 4.2 fixed that inference itself being far slower than
   real time (missing AVX2/FMA/F16C).
2. **A second, independent, and larger delay found this phase**: even
   after Whisper produces a genuine final ~3s segment, Phase 3.8.7.5's
   `TranscriptSegmenter` (`apps/desktop/src-tauri/src/segmentation.rs`)
   concatenates it into a bounded **12-20 second logical window** before
   Bible reference detection ever runs on it. A reference spoken at the
   very start of that window could wait up to ~15-20s before Bible
   detection even sees the text - regardless of how fast inference
   itself is.

The second point was undiscovered until this phase's audit (reading
`segmentation.rs` and `commands.rs::handle_audio_chunk`/
`finalize_and_route_segment` together) and is the larger of the two -
Phase 4.2 alone could not have closed this gap.

## Why "silent preload" (the operator's own answer to the partial-
## result-policy question) turned out not to be the lever

The operator was asked, and explicitly chose: CIP may silently preload
data from an unconfirmed partial transcript fragment, but must never
change what's on screen from a guess - every other domain's "always
Pending, never auto-display" rule stays intact. On investigation,
though, preloading buys nothing here: verse lookup is a local SQLite
query, already sub-millisecond. The real bottleneck was never "how
fast can CIP fetch a verse" - it was "how long until CIP has even
looked at the words that were spoken." Preloading a verse a few
seconds early cannot help if detection itself hasn't run yet.

## Fix: a fast Bible-only detection lane, decoupled from the 12-20s batch

`apps/desktop/src-tauri/src/commands.rs`:

- New `finalize_bible_only` runs the existing, unmodified
  `pipeline::handle_final_transcript` (persist the raw segment, Bible
  Intelligence Core detection, Phase 1.3/4.1 deduplication, suggestion
  persistence) on **each raw, already-final ~3s Whisper segment**,
  immediately as it arrives - not the 12-20s batch. It also emits
  `TranscriptUpdated`, so the Live Transcript panel now updates roughly
  every ~3s instead of every ~15-20s.
- `finalize_and_route_segment` (the 12-20s batch handler) no longer
  runs Bible detection at all - it now only persists its own
  `transcript_segments` row (still required: Sermon Intelligence's own
  schema has a hard, `NOT NULL` foreign key on exactly this row - see
  `database/migrations/0008_sermon_foundation.sql`) and still routes to
  Sermon/Service/Music-text (Phase 3.8.7.5 Part B), completely
  unchanged - those three genuinely need full sentences, and Cross-
  Domain/Content Intelligence remain deliberately excluded from any
  automatic trigger, as before.
- It no longer emits `TranscriptUpdated` either: the raw segments
  already cover that ground a few seconds earlier, and re-emitting the
  same speech concatenated into a bigger block would show the operator
  duplicated text.

### Why this doesn't detect anything twice, corrupt state, or violate a
### foreign key

- **No duplicate suggestions**: `handle_final_transcript`'s existing
  Phase 1.3/4.1 deduplication window (60s, same-category) already
  suppresses a repeat detection of the same reference - it was written
  for exactly this "the same reference gets mentioned again shortly
  after" case, so it does the right thing here for free. Since the
  12-20s batch no longer runs Bible detection at all, there is nothing
  left for it to duplicate either.
- **No corrupted continuation state**: `DefaultScriptureContextManager`
  is now only ever updated once per raw segment (the fast lane), not
  twice (once fast, once again from the batch reprocessing the same
  words) - simpler than before, not more fragile.
- **No broken foreign key**: `scripture_detections.transcript_segment_id`
  and `ai_suggestions.transcript_segment_id` are real, `PRAGMA
  foreign_keys = ON`-enforced references to `transcript_segments(id)`.
  Because the fast lane persists its own `transcript_segments` row for
  the raw segment before detecting (exactly what
  `handle_final_transcript` already did, unmodified), every detection/
  suggestion it produces has a real row to point to.

### Known, accepted trade-off

Both the raw ~3s rows and the 12-20s batch's own row now exist in
`transcript_segments` (the batch's row is required for Sermon's
foreign key). `list_transcript`/History (`HistoryView.tsx`) queries all
of them, so a service's transcript history will show both granularities
side by side - genuinely honest (both really were transcribed), but
visually more repetitive than a single clean transcript. The operator-
facing **live** transcript feed (`LiveChurchBrain.tsx`'s "Live
transcript" panel, what the operator watches during a service) does
**not** have this problem - only the fast lane's `TranscriptUpdated`
events reach it now. Reducing History's redundancy would need a schema
change (a column distinguishing "raw" from "batch" rows) and is
deliberately deferred rather than rushed into this phase.

## Local, free-model path confirmed - no cloud API considered or added

Per the operator's explicit constraint, no cloud/subscription service
(Deepgram, AssemblyAI, Google/Azure Speech, Gemini Live, Groq-hosted
anything, GPT-4o-mini, Claude Haiku) was evaluated for adoption - all
of them require either an API key, a subscription, or network
dependency, incompatible with this project's offline-first
architecture (see every prior phase's "offline dependency proof"
section) and the operator's own words. Whisper.cpp (already CIP's
engine, MIT-licensed, fully local) remains the only speech engine in
this codebase; this phase changes *when* CIP acts on its output, not
what produces it.

## Full regression result

- `cargo fmt --check`: clean.
- `cargo check -p cip-desktop`: clean.
- `cargo clippy -p cip-desktop --all-targets -- -D warnings`: clean.
- `cargo test -p cip-desktop`: see `pilot-evidence/4.3/` for the exact
  pass count - `pipeline.rs`/`segmentation.rs` (the modules whose
  actual logic runs Bible detection and batching) are byte-for-byte
  unchanged this phase, so their existing test coverage still applies
  unmodified; `commands.rs`'s own test module is deliberately scoped to
  "input validation + extracted guard logic" only (documented in its
  own module comment) - this project has no `tauri::test` harness, so
  the new orchestration glue (`finalize_bible_only`, the trimmed
  `finalize_and_route_segment`) is exercised the same way
  `handle_audio_chunk`/the pre-existing `finalize_and_route_segment`
  always have been: end-to-end on real hardware, not a mock Tauri app.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/commands.rs
FILES CREATED: docs/phase-4-3-instant-bible-detection.md,
  pilot-evidence/4.3/*
FILES DELETED: NONE
RUST SOURCE CHANGED: apps/desktop/src-tauri/src/commands.rs only - new
  finalize_bible_only function, finalize_and_route_segment trimmed to
  persist + route only (no longer calls Bible detection or emits
  TranscriptUpdated)
PIPELINE.RS / SEGMENTATION.RS: UNCHANGED (byte-for-byte) - Bible
  Intelligence Core, the 60s dedup window, and the 12-20s batching
  logic are reused exactly as they already existed, just called from a
  different place at a different cadence
FRONTEND CHANGED: NONE - LiveChurchBrain.tsx's existing
  onTranscriptUpdated/onScriptureDetected/onSuggestionCreated handlers
  needed no changes; they just now fire sooner and more often
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE (same event types, different cadence)
DATABASE / MIGRATIONS: UNCHANGED (no schema change - the "known,
  accepted trade-off" above deliberately does not add one this phase)
NETWORK CAPABILITIES: NONE ADDED - no cloud ASR/LLM service considered
  or added, per the operator's own explicit constraint
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass - fmt/check/clippy/test all
  clean, changes traced end to end against real FK/dedup/routing
  behavior via direct source reading (no `tauri::test` harness exists
  in this project to exercise this level automatically - see the
  regression section above).
- **Environment B (Xvfb)**: not re-run this phase - no UI-rendering
  code changed (the frontend receives the same event types, just
  sooner).
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED.** This
  is the decisive gate: does live Bible detection now surface a
  reference within a few seconds of it being spoken (matching the
  competitor demo's feel), does the Live Transcript panel now update
  every few seconds instead of every 15-20s, and does History's
  now-more-repetitive transcript list bother the operator enough to
  warrant the schema change described above.

## Known limitations

- History (`HistoryView.tsx`'s transcript list) now shows both raw
  ~3s fragments and 12-20s batch summaries for the same span of
  speech - documented above, not fixed this phase.
- This phase does not further shrink Whisper's own ~3s buffering
  window (Phase 4.2's SIMD fix only sped up inference *within* that
  window) - if real-hardware testing shows detection is still not fast
  enough even at the new ~3-6s cadence, that is the next, larger lever
  (a genuine sliding-window/short-buffer Whisper reconfiguration),
  deliberately not attempted this phase pending real evidence it's
  needed.
- No cloud/streaming ASR service was evaluated or added, per the
  operator's own explicit constraint - this phase's entire gain comes
  from using CIP's existing local whisper.cpp output sooner and more
  often, not from a different or larger model.

## Deferred work

Real-Windows re-test (Environment C). If it shows the new ~3-6s
cadence is still not fast enough, the next step is shrinking Whisper's
own buffering window (shorter, more frequent `full()` calls,
whisper.cpp's own "stream" pattern) - a larger, riskier change
deliberately not attempted without real evidence it's the remaining
bottleneck. If History's mixed granularity proves genuinely confusing
in practice, a small migration adding a `segment_kind` column (raw vs.
batch) would let the operator-facing History view filter to one
granularity - also deliberately deferred pending real feedback.

## Final gate

| Item | Status |
|---|---|
| Diagnosed the operator's competitor-comparison videos with real evidence (frame extraction), not assumption | DONE |
| Rejected every cloud/subscription API option per the operator's explicit constraint | DONE |
| Found the second, larger latency source (12-20s batching before Bible detection) via direct source audit | DONE |
| Determined "silent preload" would not have helped (verse lookup was never the bottleneck) before building unnecessary machinery | DONE |
| Implemented the fast Bible-only lane, decoupled from the 12-20s batch, with FK/dedup/context-state correctness verified by direct code reading | DONE |
| Full regression green | DONE |
| Windows artifact rebuilt end to end, new function verified present in the compiled binary | DONE - see `pilot-evidence/4.3/` |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 4.3: Environment A verification PASS. Real Windows re-test
(Environment C) is the pending, decisive gate on whether live Bible
detection now feels close to what the operator's competitor comparison
showed.**
