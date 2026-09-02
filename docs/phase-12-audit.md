# Phase 12 audit: Multi-language Whisper

## Trigger

The user's own item 9 from a pasted advice list: *"Multi-language
Whisper - currently pinned to `en`; Yoruba/Igbo/Hausa support is
explicitly named as a strategic differentiator by the advice given CIP's
likely user base (RCCG etc.), not just a nice-to-have."* Followed by the
explicit instruction "Keep going into multi-language Whisper next."

## Verifying the premise before building anything

Before writing code, two claims from the trigger needed real evidence,
not assumption - this project's standing discipline (see every prior
phase's audit doc) is to verify before implementing, not to build on an
unchecked premise.

**Claim 1: "currently pinned to `en`."** Confirmed by reading
`ai/speech/src/whisper.rs`'s `run_inference` before this phase: it never
called `FullParams::set_language` at all. `whisper-rs`'s own doc comment
on `set_language` states its C-level default is `"en"`
(`whisper_full_default_params()` sets `.language = "en"`), so every
transcription this codebase has ever run was implicitly English-only -
not configurable, not auto-detecting, just silently defaulting. The
`language: Option<String>` field that already existed on
`WhisperSpeechEngine` was dead weight: set to `None` at construction,
never written anywhere else, and only ever read to tag the *output*
`TranscriptSegment.language` (which was therefore always `None` too) -
it never touched the actual decode.

**Claim 2: Yoruba/Igbo/Hausa are all really supported.** Checked
directly against the vendored `whisper.cpp` source this build already
compiles (`whisper-rs-sys-0.13.1/whisper.cpp/src/whisper.cpp`'s `g_lang`
table - the exact 100-language vocabulary baked into every multilingual
Whisper model's tokenizer). Result: **Yoruba (`yo`, id 66) and Hausa
(`ha`, id 95) are both real, trained entries. Igbo has no entry at all -
not under `ig`, not under any other code.** This is a hard limitation of
the Whisper model architecture itself, not a missing CIP config option:
there is no token in the vocabulary for Igbo, so no amount of
`set_language`-style configuration could make a Whisper model correctly
condition on it. Forcing an unsupported code would either be silently
rejected by `whisper_lang_id` (which falls back to a partial name-match
search, not a real language token) or produce hallucinated, wrong-language
output - and this project's standing rule against fabricating progress
means that path is not attempted. See "What this phase does NOT do."

## Scope decisions

1. **Real, verified languages only: English (default), Yoruba, Hausa,
   and Auto-detect.** No Igbo option is offered anywhere in the UI - an
   honest limitation, documented with the evidence above, not silently
   dropped.

2. **Live-editable, no restart.** Unlike installing a different model
   file (which `install_whisper_model`'s own docs say "takes effect on
   CIP's next launch"), the *language* a loaded multilingual model
   should condition on is a per-inference parameter, not something that
   requires reloading the model weights. A new `SpeechEngine::set_language`
   trait method (default no-op, mirroring `discard_buffered_audio`'s own
   precedent) lets `WhisperSpeechEngine` apply a change starting with the
   *next* inference window - already-buffered-but-not-yet-inferred audio
   finishes on whatever language was selected when it started buffering,
   the same "never distort in-flight state" discipline
   `discard_buffered_audio` and the VAD gate already follow.

3. **Never fabricates which language was actually used.** After every
   real `whisper.cpp` `full()` call, `WhisperSpeechEngine` reads back
   `state.full_lang_id_from_state()` - the language id whisper.cpp
   *actually* used for that pass, whether it was forced or auto-detected -
   and converts it back to a code via `whisper_rs::get_lang_str`. This
   is what now populates `TranscriptSegment.language`, replacing the
   permanently-`None` field that existed before this phase. Auto-detect
   mode's real value: if the operator selects Auto-detect and whisper.cpp
   guesses the speaker is actually speaking French, `language` now
   honestly says `"fr"` rather than lying by omission.

4. **Detects and surfaces whether the loaded model can honor a language
   switch at all.** `whisper-rs::WhisperContext::is_multilingual()` (a
   real, already-available binding) is called once at model-load time
   and stored in `SpeechDiagnostics.model_is_multilingual`. An
   English-only model file (a `ggml-*.en.bin`, architecturally missing
   the language-token vocabulary entirely) cannot honor Yoruba/Hausa/
   Auto no matter what the operator selects - `get_speech_language_capabilities`
   reports this honestly (`Some(false)`) rather than letting the
   operator believe a language switch worked when the loaded model
   cannot physically do it.

5. **Not Admin-gated.** Which language a service is being preached in is
   a live-workflow choice, not a system-configuration item - it belongs
   with the same "available to any logged-in operator" class as
   selecting a Bible translation or the audio input device, not with
   Phase 10's seven configuration-gated commands (installing a model
   file, importing a Bible dataset, production-integration credentials).

## Testing boundary

Every test in this phase that requires a real loaded multilingual model
(`is_multilingual()`'s true behavior, `full_lang_id_from_state()`'s real
readback, an actual Yoruba/Hausa transcription) is untestable in this
environment for the same documented reason every other Whisper-model
test in this codebase already is: the standard model host is blocked by
this environment's egress policy (see `ai/speech/src/whisper.rs`'s own
module docs). What *is* genuinely testable without a model file, and is
tested: `SUPPORTED_LANGUAGES`'s exact contents (English/Yoruba/Hausa/
Auto, and explicitly *not* Igbo) and `is_supported_language`'s
validation logic, both pure functions in `ai/speech/src/lib.rs`;
`set_speech_language`'s rejection of an unsupported code at the command
layer, tested the same way every other input-validation command in this
codebase already is (`ensure_admin`-style pure-function testing, no
`tauri::test` harness).

## What this phase does NOT do

- **No Igbo support** - see the evidence above. A future phase could
  revisit this only if Whisper itself gains Igbo support upstream, or if
  CIP ever adopts a different STT backend/model with real Igbo training
  data - neither is attempted here.
- **No per-service language history or auto-switching mid-service** -
  one language selection applies until the operator changes it again;
  CIP does not attempt to detect a language change mid-sermon and switch
  automatically (Auto-detect mode re-detects every inference window
  independently, which is a related but different thing - it does not
  remember or lock onto a detected language across windows).
- **No translation** - `whisper.cpp`'s own `translate` flag (translate
  the recognized speech into English text) is not touched; this phase is
  about which language is *recognized*, not translating the output.
- **No language-specific Bible-reference detection tuning** -
  `core/bible`'s reference detector, alias tables, and paraphrase/
  semantic matching remain English-text-oriented regardless of the
  transcription language; a Yoruba sermon transcribed correctly by
  Whisper would still need an English (or otherwise-matching) spoken
  reference for `core/bible` to detect it. Out of scope for this phase,
  which is specifically about the speech-recognition layer.
