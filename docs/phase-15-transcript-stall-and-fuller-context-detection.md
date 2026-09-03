# Phase 15: Transcript Stall Visibility + Fuller-Context Bible Detection

## Baseline

Trigger: nine real Windows pilot screenshots of Phase 14's own rebuilt artifact, showing the
Live Transcript panel frozen at the exact same six lines across several minutes of visible video
progress, plus the operator's report of "delay... very little output... no bible detection" and
a request that CIP detect Scripture even when paraphrased without a book name. Full root-cause
investigation in `docs/phase-15-audit.md`.

## What was actually wrong (verified, not assumed)

1. A placeholder-discarded Whisper window (Phase 14's own fix) returns an empty result, so
   neither Bible detection nor the transcript-window accumulator ever sees it - during a
   sustained quiet/unclear stretch the operator sees nothing happen with no signal CIP is still
   listening, a real side effect of Phase 14's own correct fix.
2. An already-computed "seconds since the last transcript activity" signal already existed
   (`transcript_freshness`, 30s threshold) but was only ever shown in the Service Intelligence
   section, not next to the Live Transcript panel itself where a stall is actually noticed.
3. Bible detection (citation, lexical paraphrase, semantic) only ever runs on each raw ~3s
   Whisper window, never on the fuller 12-20s accumulated window - a single ~3s fragment often
   lacks the vocabulary a genuine paraphrase needs to be recognized against, even though the
   accumulated window usually has plenty.
4. The lexical paraphrase detector itself is confirmed correct and already tested within its own
   documented scope (word-overlap against a specific verse's vocabulary) - none of the six
   reported transcript lines were themselves paraphrases of any verse, so no implementation would
   have matched them; the real, fixable gap is reduced odds of catching a genuine paraphrase from
   a lone 3-second fragment.

## What was built

- **`core/service/src/bible_intelligence.rs`**: new
  `retry_paraphrase_or_semantic_with_fuller_context` - the same `try_paraphrase`/`try_semantic`
  fallbacks already used, never re-running citation detection.
- **`apps/desktop/src-tauri/src/pipeline.rs`**: extracted the existing detection/suggestion
  persistence-and-dedup logic into a shared `persist_detections_and_suggestions` helper, reused
  by both the raw-window path and the new fuller-context retry (which never re-persists the
  transcript segment row itself).
- **`apps/desktop/src-tauri/src/commands.rs`**: `finalize_bible_only` now reports whether it kept
  a suggestion; a new `finalize_bible_fuller_context_retry`, gated on that report, runs once each
  accumulated 12-20s window closes (and on the stop-mid-window tail flush) only when nothing
  already found a suggestion.
- **Frontend**: the Live Transcript panel now repeats the already-existing `transcriptFreshness`
  signal directly under the transcript list when genuinely stale (≥30s), with an honest
  explanation (a quiet/unclear microphone signal, not a stopped app) instead of silence.

## Explicitly deferred

No audio gain normalization (unchanged from Phase 14's own deferral), no threshold changes to the
paraphrase/semantic fallbacks, no new placeholder captions - see `docs/phase-15-audit.md`'s
"Explicitly deferred" section.

## Testing boundary

`retry_paraphrase_or_semantic_with_fuller_context` is pure and fully unit-tested (4 new tests).
The `commands.rs` wiring is thin orchestration mirroring `silent_windows_skipped`'s own
already-established "untested directly" precedent. The frontend change only displays an
already-tested, already-polled value - no new frontend test was added, mirroring Phase 13's own
precedent.

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both feature configs.
- `cargo test --workspace`: all green; `cip-core-service` 35 passed (up from 31), `cip-desktop`
  350 passed (unchanged).
- `cargo test -p cip-desktop --features whisper` / `cargo test -p cip-ai-speech --features
  whisper`: 350 / 19 passed, unchanged.
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings, unchanged) / `npm run test --
  run` (294 passed, unchanged) / `npm run build`: all clean.

## Architectural safety

- Zero new Tauri commands, zero new events, zero new migrations.
- The fuller-context retry can never attempt citation detection or mutate the active Scripture
  context - it calls only the two already-existing fallback functions under the same gating
  `process_transcript_segment_inner` already uses.
- `persist_detections_and_suggestions`'s extraction is a pure refactor - the raw-window path's
  own behavior is byte-for-byte unchanged.
- `core/bible`, `core/sermon`, `core/music`, `core/presentation` are entirely untouched.

## Known limitations (honest, not deferred silently)

- The fuller-context retry raises the odds of a genuine paraphrase match but is still bounded by
  the same thresholds as before - a paraphrase sharing almost no vocabulary with its verse still
  needs the semantic (embedding) fallback, which still needs an operator-provisioned model never
  verified on real hardware.
- The stall indicator explains that nothing new has been transcribed and offers the most likely
  honest reason - it cannot distinguish every possible cause with more precision than the
  underlying freshness signal already can.
- This exact rebuilt artifact has NOT yet been installed or launched on real Windows hardware -
  see `physicalHardwareStatement` item 24 in the updated release manifest.

## Final gate

Environment A (build-time verification, full regression, direct binary symbol inspection): PASS.
Environment C (a second real pilot session confirming the stall indicator appears during a real
quiet stretch and that a genuine paraphrase is now more reliably caught): not yet performed -
carried forward into `physicalHardwareStatement`.
