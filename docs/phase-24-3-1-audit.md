# Phase 24.3.1: Quality-Tier Language-Conditioning Fix

## Trigger

Offered as one of several viable next options after Phase 24.3 (true
dual-tier Whisper): "the language-conditioning gap in the quality tier -
`transcribe_once` always uses the quality engine's own default language
setting; it's never told what language the fast tier detected for that
window." The operator asked for this fix specifically.

## The gap

`WhisperSpeechEngine::transcribe_once` (Phase 24.3) decoded using
`self.requested_language` - the quality engine's *own* separately
configured language (set via its own `set_language`, defaulting to
`"en"`). The fast tier's real, per-window detected language
(`TranscriptSegment.language`, read back from whisper.cpp's own
`full_lang_id_from_state` since Phase 12) was never passed to the quality
tier at all. On a service using `"auto"` language detection - or simply
one where the two tiers' language settings happened to differ - the two
engines could silently condition their decodes of the *exact same audio*
on different languages, with no error and no diagnostic signal that this
had happened.

## Fix

- `core/ai/src/speech_engine.rs`: `SpeechEngine::transcribe_once` gained
  a `language_hint: Option<&str>` parameter. Doc comment explains its
  contract: honest evidence about *this specific audio* (what the first
  engine's own real decode reported), never a guess; a no-op for the
  default implementation and any engine that doesn't do language-
  conditioned decoding.
- `ai/speech/src/whisper.rs`: `WhisperSpeechEngine::transcribe_once` now
  takes `language_hint`, using `language_hint.unwrap_or(&self.requested_language)`
  as `decode_pass`'s `requested_language` argument for *that one call
  only* - `self.requested_language` itself (this instance's own
  `set_language`-configured default, governing every other decode this
  instance performs) is never mutated.
- `apps/desktop/src-tauri/src/commands.rs`: `QualityJob` gained
  `language_hint: Option<String>`, populated in `handle_audio_chunk` from
  `segment.language.clone()` - the fast tier's own already-detected value
  for the exact window this job's audio came from. `spawn_quality_worker`
  passes `job.language_hint.as_deref()` through to `transcribe_once`.

No new commands, no new events, no new migration - this is a pure
signal-plumbing fix within the existing Phase 24.3 dual-tier pipeline.

## Why this is safe

`language_hint` only ever affects the one `transcribe_once` call it's
passed to - it never touches `self.requested_language`, so an operator's
explicit language selection for the quality tier (if they ever set one
independently) is never silently overridden for any *other* call. When
the fast tier reports no language for a window (rare - only an engine
that never detects one), `language_hint` is `None` and the quality tier
falls back to exactly its own prior behavior (`self.requested_language`),
so this fix can only make the two tiers agree *more* often, never less.

## Testing boundary

Same limitation as `transcribe_once` itself (`docs/phase-24-3-audit.md`):
verifying the actual decode conditions on the passed language needs a
real `WhisperContext`, which needs a real model file this container
cannot obtain. The fallback logic itself (`unwrap_or`) is trivial enough
that no new pure-logic unit test was warranted - compilation and the
existing 31/31 `cip-ai-speech --features whisper` tests (unchanged)
confirm the signature change didn't break anything real.

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets
-- -D warnings` clean, and again with `--features whisper` on the
desktop crate - clean. `cargo test --workspace` unchanged pass counts
everywhere (368/368 `cip-desktop` unit tests in both feature configs;
31/31 `cip-ai-speech --features whisper`) - no new tests, per the
testing-boundary note above. Frontend: `npm run typecheck` 0 errors,
`npm run build` clean - this change touches zero frontend files, so
`npm run lint`/`npm run test -- --run` were not expected to differ and
were not re-run in full for this small a change; `typecheck`/`build`
confirm nothing frontend-visible broke.

## Final gate

Environment A (fmt/clippy/test in both feature configs, frontend
typecheck/build): PASS. This fix does not close Environment C for the
quality tier - it makes the existing, still-real-hardware-unconfirmed
dual-tier pipeline more correct once it *is* confirmed, not a
substitute for that confirmation. See `docs/phase-24-3-audit.md`'s own
Known Limitations for the standing gap.
