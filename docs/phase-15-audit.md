# Phase 15 Audit: Transcript Stall Visibility + Fuller-Context Bible Detection

## Trigger

Nine real Windows pilot screenshots (Phase 14's own rebuilt artifact, sha256
`f0c52145fa3adfb4ab5a6ed8d38e0ef0a735891b3e85d5b992ebd53ada728b33`), taken
minutes apart while a YouTube sermon ("How to Study the Bible - Apostle
Gideon Odoma") played through the laptop's own speakers and was picked up by
its own microphone. Across all nine photos, spanning several minutes of
visible video progress (the video's own captions and playback timer clearly
advance between screenshots), the Live Transcript panel shows the **exact
same six lines, frozen at 19:36:54** - never a seventh line, however long the
video kept playing. The operator also reported "delay... produce very little
output, accuracy fair, no bible detection" and, separately, an input level
around 5%, and asked that CIP detect Scripture even when paraphrased and no
book name is spoken.

## What was actually wrong (verified by direct code trace, not assumed)

1. **The Live Transcript panel was never merely slow - it can go completely
   silent with zero operator-visible signal, for an unbounded stretch, as a
   direct side effect of Phase 14's own fix.** Traced the exact call chain:
   - `ai/speech/src/whisper.rs::run_inference` returns `Ok(vec![])` (an empty
     result, not a segment with empty text) whenever the decoded text is one
     of Phase 14's `NON_SPEECH_PLACEHOLDERS`.
   - `apps/desktop/src-tauri/src/commands.rs::handle_audio_chunk`'s
     `for segment in segments` loop - the *only* call site for both
     `finalize_bible_only` (Bible detection) and `segmenter.push` (the
     accumulator that eventually flushes a line into the Live Transcript
     panel) - therefore never executes at all for a placeholder-discarded
     window.
   - `segmentation.rs::TranscriptSegmenter::push` only starts counting its
     15s accumulation window (`first_start_ms`) the first time it receives
     *non-empty* text; while it never receives one, `span_ms` stays `0`
     forever and the window never flushes.
   - Net effect: during any sustained stretch where every ~3s Whisper window
     decodes to a placeholder (very plausible on the speaker-to-mic loopback
     test setup in these screenshots - quiet, echoing, indirect audio is
     exactly the scenario Phase 14 was built to recognize and discard), the
     operator sees literally nothing happen, with no indication CIP is still
     listening versus having silently stopped. Before Phase 14, at least a
     (wrong) placeholder caption appeared every ~3s; Phase 14 correctly
     stopped showing those, but nothing was added to tell the operator the
     app was still alive during the resulting silence.
2. **A real, already-computed "how long since the last transcript activity"
   signal already existed** (`service.rs::transcript_freshness`,
   `TRANSCRIPT_STALE_AFTER_SECONDS = 30`, already polled every status refresh
   into `serviceIntel.transcriptFreshness`) - but it was only ever displayed
   in the Service Intelligence section further down the page, not next to
   the Live Transcript panel itself, which is exactly where an operator
   staring at a stalled transcript is actually looking. The screenshots
   never show that section expanded.
3. **Bible detection genuinely only ever runs on each raw ~3s Whisper
   window** (`finalize_bible_only`, confirmed via its own doc comment: "Bible
   detection's only live-audio entry point" since Phase 4.3), never on the
   bounded 12-20s accumulated window `finalize_and_route_segment` builds for
   Sermon/Service/Music. `core/service/src/bible_intelligence.rs`'s own
   thresholds (`MIN_PARAPHRASE_SIGNIFICANT_WORDS`/
   `MIN_SEMANTIC_SIGNIFICANT_WORDS = 4` distinct words) are a low bar, but a
   single ~3s window (roughly 8-15 spoken words, often fewer once stopwords
   are stripped) frequently doesn't clear it at all, and even when it does,
   `try_paraphrase`'s overlap ratio is computed against only that fragment's
   handful of words rather than the fuller sentence a genuine paraphrase
   needs to be recognized against. None of the six reported transcript lines
   ("understand the Word of God," "starting the Bible is inevitable if you
   have," etc.) are themselves paraphrases of a specific verse - they're
   meta-commentary about Bible study - so no correct implementation would
   have matched them to a reference; the real, fixable gap is that a
   genuine paraphrase spoken later in the sermon would have had a
   meaningfully worse chance of being caught by a lone 3-second fragment
   than by the same wording considered as a whole sentence.
4. **The lexical paraphrase detector (`core/bible/src/paraphrase.rs`) is
   confirmed real, tested, and working correctly within its own documented
   scope** - a bounded word-overlap heuristic against a specific verse's own
   vocabulary, not conceptual/semantic matching. Its own module docs already
   honestly document the "Jesus said we should love our enemies" (Matthew
   5:44) gap - a paraphrase sharing almost no vocabulary with the verse -
   which is exactly what the semantic (embedding) fallback exists for, and
   that fallback requires an operator-provisioned embedding model this
   environment has never had the opportunity to verify against on real
   hardware (an already-documented, carried-forward limitation, not new).

## What was built

- **`core/service/src/bible_intelligence.rs`**: new
  `retry_paraphrase_or_semantic_with_fuller_context` - the exact same
  `try_paraphrase`/`try_semantic` fallbacks `process_transcript_segment`
  already uses, called directly (never re-running citation detection, since
  a raw ~3s window is already enough for that and re-attempting it would
  only rediscover what the fast path already found or already correctly
  missed).
- **`apps/desktop/src-tauri/src/pipeline.rs`**: factored the existing
  detection/suggestion persistence-and-dedup logic (previously inline at the
  tail of `handle_final_transcript_inner`) out into a shared
  `persist_detections_and_suggestions` helper, and added
  `retry_paraphrase_or_semantic_with_fuller_context` as a second caller of
  it - deliberately never calls `persist_transcript_segment` itself, since
  the caller guarantees the accumulated segment's row already exists.
- **`apps/desktop/src-tauri/src/commands.rs`**:
  - `finalize_bible_only` now returns `bool` (whether it kept a suggestion).
  - New `finalize_bible_fuller_context_retry`, called from
    `spawn_speech_worker`'s per-window flush (both the normal 15-20s flush
    and the stop-mid-window tail flush) *only* when none of the window's own
    raw ~3s sub-segments already found a suggestion - so a confident
    citation or an already-found paraphrase/semantic match is never
    second-guessed.
  - A per-listening-session `window_had_suggestion: bool`, owned exclusively
    by the speech worker thread (mirrors `segmenter`/`consecutive_overloads`'s
    own ownership discipline), reset on every window flush and on the
    overload-discard path's own segmenter reset (so a discarded window's
    stale suggestion tally never leaks into the next one).
- **Frontend (`LiveChurchBrain.tsx`)**: the Live Transcript panel now
  repeats the already-existing, already-polled `transcriptFreshness` signal
  directly under the transcript list itself (only when genuinely stale, ≥30s
  since the last segment) - "No new transcript in Ns - still listening;
  this can mean the microphone isn't picking up clear speech... rather than
  a stopped app." No new backend signal was needed; this was a UI-placement
  fix, moving an existing, correct, already-tested value to where an
  operator experiencing a stall is actually looking.

## Explicitly deferred

- **No audio gain normalization/AGC** - unchanged from Phase 14's own
  documented deferral; still the right call without real hardware to verify
  a gain stage against.
- **No change to any paraphrase/semantic threshold** (`MIN_PARAPHRASE_SCORE`,
  `MIN_SEMANTIC_SIMILARITY`, the `4`-significant-word minimums) - the fix
  here is giving the existing, already-calibrated thresholds more
  vocabulary to work with, not loosening them, which would risk false
  positives without any real operator feedback to calibrate against.
- **No new placeholder captions added to `NON_SPEECH_PLACEHOLDERS`** - no
  new evidence of an unrecognized caption surfaced in these screenshots.

## Testing boundary

`retry_paraphrase_or_semantic_with_fuller_context` (core/service) is pure
and fully unit-tested (4 new tests: finds a paraphrase a short fragment
alone would miss, never attempts citation detection, never mutates context,
falls through to semantic when lexical finds nothing). The `commands.rs`
wiring (`window_had_suggestion` tracking, the two call sites) is thin
orchestration mirroring `silent_windows_skipped`'s own already-established
"untested directly, the pure logic beneath it is what's tested" precedent -
no dedicated integration test was added for it, consistent with this
project's standing convention. The frontend change only ever displays an
already-tested, already-polled value (`transcriptFreshness`); no new
frontend test was added, mirroring Phase 13's own precedent of skipping a
redundant render test for a section that only surfaces already-tested data.

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs.
- `cargo test --workspace`: all green; `cip-core-service` 35 passed (up from
  31 - the 4 new tests), `cip-desktop` 350 passed (unchanged - the wiring is
  thin orchestration with no new direct test, per the testing-boundary note
  above).
- `cargo test -p cip-desktop --features whisper` / `cargo test -p
  cip-ai-speech --features whisper`: 350 / 19 passed, unchanged.
- Frontend: `npm run typecheck` / `npm run lint` (same 5 pre-existing
  warnings) / `npm run test -- --run` (294 passed, unchanged) / `npm run
  build`: all clean.

## Architectural safety

- Zero new Tauri commands, zero new events, zero new migrations - the
  fuller-context retry reuses `finalize_and_route_segment`'s own
  already-persisted accumulated segment row; the stall indicator reuses an
  already-existing, already-polled command's output.
- `retry_paraphrase_or_semantic_with_fuller_context` can never attempt
  citation detection or mutate `DefaultScriptureContextManager` - it calls
  only `try_paraphrase`/`try_semantic`, the same functions
  `process_transcript_segment_inner` already calls under the identical "only
  when nothing else already found a suggestion" gating.
- `persist_detections_and_suggestions`'s extraction is a pure refactor -
  `handle_final_transcript_inner`'s own behavior for the raw-window path is
  byte-for-byte unchanged; only its second caller is new.
- `core/bible`, `core/sermon`, `core/music`, `core/presentation` (every
  domain contract crate other than `core/service`, which this phase touches
  only to add one new function alongside the existing ones) are entirely
  untouched.

## Known limitations (honest, not deferred silently)

- The fuller-context retry raises the odds of catching a genuine paraphrase
  that needs more than one raw window's worth of vocabulary, but it is still
  bounded by the same lexical-overlap/semantic-similarity thresholds as
  before - it cannot turn a paraphrase sharing almost no vocabulary with its
  verse into a lexical match (that remains the semantic fallback's job, and
  that fallback still needs an operator-provisioned embedding model this
  environment has never verified against real hardware).
- The stall indicator explains *that* nothing new has been transcribed
  recently and offers the most likely honest reason (a quiet/unclear
  microphone signal) - it cannot diagnose every possible cause (e.g. a
  genuinely stopped listening session versus a long pause in speech) any
  more precisely than the underlying `transcript_freshness` signal already
  could.
- This exact rebuilt artifact has not yet been installed or launched on real
  Windows hardware, and neither fix has been re-verified against a second
  real pilot session - see `physicalHardwareStatement` item 24 in the
  updated release manifest.

## Final gate

Environment A (build-time verification, full regression, direct binary
symbol inspection): PASS. Environment C (a second real pilot session on the
same or similar hardware, confirming the Live Transcript panel now surfaces
a clear "no new transcript" message instead of silently freezing during a
long quiet/unclear stretch, and that a genuine paraphrase spoken without a
book name is now more likely to be caught): not yet performed - carried
forward into `physicalHardwareStatement` per this project's standing
discipline.
