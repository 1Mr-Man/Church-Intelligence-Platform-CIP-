# Phase 12: Multi-language Whisper

## Baseline

Trigger: the user's own item 9 from a pasted advice list ("Multi-language
Whisper - currently pinned to `en`..."), followed by the explicit
instruction "Keep going into multi-language Whisper next." Full
reasoning in `docs/phase-12-audit.md`, including the direct-evidence
verification this phase started with (confirming the "pinned to `en`"
claim by reading `ai/speech/src/whisper.rs`, and confirming
Yoruba/Hausa - but *not* Igbo - are real Whisper languages by reading
`whisper.cpp`'s own vendored source's `g_lang` table).

## Design choices

See `docs/phase-12-audit.md` in full. Summary: English (default),
Yoruba, Hausa, and Auto-detect are offered - all four are real, verified
entries in `whisper.cpp`'s 100-language vocabulary; **Igbo is
deliberately not offered**, because that vocabulary has no Igbo entry at
all (checked directly against the vendored source, not assumed) - a
hard limitation of the Whisper model architecture itself, not a missing
CIP config option. Live-editable via a new `SpeechEngine::set_language`
trait method (default no-op), applied starting with the next inference
window. Every real inference pass reads back the language whisper.cpp
*actually* used via `full_lang_id_from_state`, so `TranscriptSegment.language`
is now an honest report of what happened rather than the permanently-`None`
field it was before this phase. Whether the loaded model can honor a
language switch at all (`WhisperContext::is_multilingual()`) is detected
once at load time and surfaced honestly. Not Admin-gated - a live-workflow
choice, not a system-configuration item.

## What was built

- **`core/ai/src/speech_engine.rs`**: new `SpeechEngine::set_language`
  trait method (default no-op, mirrors `discard_buffered_audio`'s own
  precedent).
- **`ai/speech/src/lib.rs`**: `SUPPORTED_LANGUAGES` (English/Yoruba/
  Hausa/Auto-detect, deliberately unconditional on the `whisper` Cargo
  feature) and `is_supported_language` - the single validation point the
  command layer uses.
- **`ai/speech/src/whisper.rs`**: `requested_language: String` field
  (replaces the previously dead `language: Option<String>`, default
  `"en"`); `is_multilingual()` (wraps `WhisperContext::is_multilingual`);
  `run_inference` now calls `params.set_language(Some(&self.requested_language))`
  before decoding and reads back the real used/detected language via
  `state.full_lang_id_from_state()` + `whisper_rs::get_lang_str` after;
  `SpeechEngine::set_language` implementation updates
  `requested_language` for the next window.
- **`apps/desktop/src-tauri/src/state.rs`**: `AppState.speech_language`
  (`Mutex<String>`, defaults `"en"`); `SpeechDiagnostics.model_is_multilingual`
  (`Option<bool>`, `None` until a model loads).
- **`apps/desktop/src-tauri/src/lib.rs`**: `create_speech_engine` now
  calls `engine.is_multilingual()` before boxing and populates
  `model_is_multilingual`.
- **`apps/desktop/src-tauri/src/commands.rs`**: `SpeechLanguageOptionDto`/
  `SpeechLanguageCapabilitiesDto`; 2 new commands
  (`get_speech_language_capabilities` - open to any operator;
  `set_speech_language` - validates against `is_supported_language`,
  updates `speech_language`, and applies immediately via
  `speech_engine.set_language`).
- **Frontend**: `domain/speech.ts` (`SpeechLanguageOption`/
  `SpeechLanguageCapabilities`); 2 new `commands.ts` wrappers; a
  language `<select>` in `LiveChurchBrain.tsx` next to the audio-device
  selector, plus an honest notice when the loaded model is English-only
  and can't honor the current selection.

## Testing boundary

Everything requiring a real loaded multilingual model
(`is_multilingual()`'s true behavior, `full_lang_id_from_state()`'s real
readback, an actual Yoruba/Hausa transcription) is untestable in this
environment for the same documented reason every other Whisper-model
test in this codebase already is (the standard model host is blocked by
this environment's egress policy). What *is* genuinely testable without
a model file: `SUPPORTED_LANGUAGES`'s exact contents and
`is_supported_language`'s validation logic, both pure functions - 4 new
tests in `ai/speech/src/lib.rs`, including an explicit
`igbo_is_never_offered_as_a_supported_language` test that guards against
the exclusion being silently reverted without the same verification.
`set_speech_language`'s rejection of an unsupported code is exercised
indirectly through those same `is_supported_language` tests, per this
project's standing "no `tauri::test` harness" convention (thin command
wrappers stay untested directly; the pure logic beneath them is what's
tested).

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs (default and `--features whisper`).
- `cargo check --workspace` / `cargo check --features whisper`: clean.
- `cargo test --workspace`: 1014 passed, 0 failed (default config, up
  from Phase 11's 1010 - 4 new, all in `ai/speech`).
- `cargo test -p cip-ai-speech --features whisper`: 15 passed, 0 failed
  (up from 11 pre-Phase-12 - the same 4 new tests, plus confirming the
  whisper-feature build itself is unaffected).
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings,
  unchanged) / `npm run test -- --run` (285 passed, up from Phase 11's
  280 - 5 new) / `npm run build`: all clean.

## Architectural safety

- 2 new Tauri commands, zero new events, zero new migrations - no
  persistence at all, the selection is in-memory/session-scoped,
  identical precedent to `production_integration_config`/
  `current_operator`/`companion_snapshot`.
- `SpeechEngine::set_language`'s default no-op means `NullSpeechEngine`
  and `ScriptedSpeechEngine` are completely unaffected - the trait
  addition changes zero existing behavior for either.
- `WhisperSpeechEngine`'s pre-Phase-12 default behavior (implicit
  English, whisper.cpp's own C-level default) is preserved exactly for
  any caller that never touches the new setting - `requested_language`
  defaults to `"en"`, the same value whisper.cpp would have used anyway.
- `core/bible`, `core/service`, `core/presentation` (every domain
  contract crate) are entirely untouched - language selection lives only
  in the speech-recognition layer, never in detection/presentation.

## Windows rebuild

Required: this phase changes Rust code compiled into the desktop binary
(new trait method, changed `WhisperSpeechEngine` field/behavior, new
`AppState`/`SpeechDiagnostics` fields, two new commands). See
`pilot-evidence/12/windows/installer-contents-verification.json` and the
updated `release/windows/release-manifest.json`.

## Known limitations (honest, not deferred silently)

- **No Igbo support** - `whisper.cpp`'s own 100-language vocabulary has
  no Igbo entry at all, verified directly against the vendored source.
  This is a hard limitation of the Whisper model architecture, not a
  missing CIP configuration option - see `docs/phase-12-audit.md`'s
  "Verifying the premise before building anything" for the full
  evidence. A future phase could revisit this only if Whisper itself
  gains Igbo support upstream, or if CIP ever adopts a different STT
  backend with real Igbo training data.
- **No per-service language history or mid-service auto-switching** -
  one selection applies until changed again; Auto-detect re-detects
  every inference window independently rather than locking onto a
  detected language across windows.
- **No translation** - `whisper.cpp`'s own `translate` flag is untouched;
  this phase is about which language is recognized, not translating the
  output to English.
- **No language-specific Bible-reference detection tuning** -
  `core/bible`'s reference detector remains English-text-oriented
  regardless of the transcription language.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware**, and no real Yoruba or Hausa audio has ever
  been transcribed by this code (no real model, no real recorded audio
  of either language exists in this repository or this environment) -
  see `physicalHardwareStatement` item 21 in the updated release
  manifest.

## Final gate

Environment A (build-time verification, full regression, direct binary
symbol inspection): PASS. Environment C (a real operator selecting
Yoruba or Hausa with a real multilingual model installed, speaking in
that language, and confirming the transcript and its reported
`language` field are both correct; and separately confirming an
English-only model honestly reports `modelIsMultilingual: false` and
still transcribes in English regardless of the selection): not yet
performed - carried forward into `physicalHardwareStatement` per this
project's standing discipline.
