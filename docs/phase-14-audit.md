# Phase 14 Audit: Real Windows Pilot — Whisper Hallucination on Quiet Audio

## Trigger

The first genuine Environment C evidence this project has received: two screenshots from a real
Windows laptop (HP, Intel Smart Sound Technology array microphone) running the Phase 13 installer
live. The operator reported: "It fabricating live transcript, it not detecting the audio correctly
and it detect blank audio frequently, no bible detection." The Live Transcript panel showed:

```
"(speaking in foreign language)" (75%)
"(speaking in foreign language)" (75%)
"[BLANK_AUDIO]" (75%)
"I've got myself going on." (75%)
"(speaking in foreign language)" (75%)
"Yeah, we did it. We did it." (75%)
"[inaudible]" (75%)
"[BLANK_AUDIO]" (75%)
"(speaking in foreign language)" (75%)
"Peace forever for beating the against fire." (75%)
"and then they're done for me, and come to me." (75%)
"[LAUGHTER]" (75%)
```

Input level was reported as 6%. No Scripture reference was ever detected. Every single line showed
the identical confidence, `(75%)`.

## Root-cause investigation (source-verified, not assumed)

**Claim 1 — every confidence badge reads 75% because it is a hardcoded literal, not a real score.**
Confirmed directly: `ai/speech/src/whisper.rs`'s `run_inference` builds every `TranscriptSegment`
with `ConfidenceResult::new(0.75, ConfidenceSource::Model, Some("whisper.cpp full() decode; no
per-token confidence exposed by this API".to_string()))`. That comment's premise is itself false -
verified against the vendored `whisper-rs-0.14.4` source
(`~/.cargo/registry/.../whisper-rs-0.14.4/src/whisper_state.rs`): `WhisperState::full_n_tokens` and
`WhisperState::full_get_token_prob` are both real, implemented accessors exposing whisper.cpp's own
per-token decode probability. This code was never using them - `0.75` was displayed for a garbled
hallucination and for "Yeah, we did it. We did it." alike, giving the operator zero way to tell them
apart from the confidence badge.

**Claim 2 — the bracketed/parenthesized lines are not CIP inventing text; they are whisper.cpp's own
well-documented non-speech placeholder captions.** `[BLANK_AUDIO]`, `(speaking in foreign language)`,
`[inaudible]`, and `[LAUGHTER]` are not garbage in the sense of random tokens - they are literal
strings that appear verbatim in the caption datasets Whisper was trained on (YouTube-auto-caption-
style annotations for non-speech audio), which the model reproduces as ordinary decoded text when it
is uncertain about quiet, unclear, or non-speech-shaped audio. This is a widely-documented
whisper.cpp/openai-whisper failure mode, not a defect unique to this integration - but this codebase
had no code anywhere that recognized these specific strings and treated them as "not real spoken
content," so they were displayed, and would have been persisted and fed into Bible/Sermon/Music/
Content detection, exactly as if the congregation had said them. That directly violates this
project's own "never fabricate" discipline (the same discipline that motivated Phase 5.3's silence
gate) just as much as inventing text from nothing would.

**Claim 3 — the audio genuinely reached whisper.cpp; this was not a silence-gate miss.**
`SILENCE_RMS_THRESHOLD = 0.01` (1%) in `ai/speech/src/whisper.rs`, deliberately conservative per its
own doc comment ("skipping a window that actually contained soft-but-real speech is a far worse
failure than occasionally spending one inference pass on true silence... revisit once real operator
feedback from a live sanctuary environment exists" - Phase 5.3). The reported 6% input level is above
that floor, so every window was correctly sent to `whisper.cpp`'s real `full()` call - the VAD gate
worked exactly as designed. The problem is that whisper.cpp itself hallucinates more on quiet,
echo-y, or otherwise low-clarity audio, and this codebase did nothing downstream of that inference
call to recognize when it had happened.

**Claim 4 — no gain control exists anywhere in the audio path.**
Confirmed by reading `integrations/audio/src/lib.rs` in full: `downmix_to_i16` only converts sample
format and averages channels; there is no normalization, no AGC, no gain stage anywhere between the
microphone and either the level meter or `feed_audio`. This was considered as a fix (see "Explicitly
deferred" below) and rejected for this phase with a documented reason, not silently skipped.

**Claim 5 — "no Bible detection" is very likely a consequence of Claims 1-3, not a fifth, separate
defect.** Of the twelve lines shown, seven are pure non-speech placeholder captions and would (once
Claim 2 is fixed) never reach the detector at all; three are garbled nonsense phrases with no
Scripture content ("Peace forever for beating the against fire.", "and then they're done for me, and
come to me.", "I've got myself going on."); the two coherent lines ("Yeah, we did it. We did it.")
also contain no Scripture reference. There is no evidence in this transcript of a real, cleanly
transcribed Scripture reference that Bible Intelligence failed to catch - the more precise, honestly
stated finding is that this specific low-signal capture never produced a clean enough transcript for
detection to have anything to work with, not that detection itself is broken. This is reported to
the user as the honest read of the evidence, not silently assumed to be "fixed" by the changes below.

## Design decisions

1. **Recognize and drop whisper.cpp's own known non-speech placeholder outputs**, the same way a
   near-silent window is already dropped before inference - post-decode this time, since these are
   knowable only after `full()` returns. A new pure function normalizes decoded text (lowercase,
   strip everything but letters/digits, collapse whitespace) and compares it against a documented set
   of known placeholder phrases; a segment whose *entire* normalized text is one of these phrases is
   discarded exactly like an empty-text result already is. Deliberately conservative: a segment that
   mixes real words with a bracketed tag is kept in full, since only the pure-placeholder case is
   unambiguous.
2. **Compute a real per-segment confidence** from whisper.cpp's own per-token decode probabilities
   (`full_n_tokens`/`full_get_token_prob`, averaged across every token in the pass), replacing the
   hardcoded `0.75`. This alone would not have hidden the placeholder captions above (whisper.cpp is
   often quite "confident" in exactly this failure mode), which is why fix 1 above is still necessary
   - but it makes the confidence badge honest for every other segment, including the garbled-but-real-
   looking sentences that fix 1 cannot catch.
3. **Track and surface both outcomes honestly in diagnostics**, mirroring Phase 5.3's own
   `silent_windows_skipped` counter exactly: a new `SpeechEngine::last_feed_was_non_speech_placeholder`
   trait method (default `false`) and a new `SpeechDiagnostics.non_speech_placeholders_skipped`
   counter, visible in the same System Diagnostics panel row style as the existing silence counter -
   so an operator (or a future debugging session) can see directly how often this is happening, not
   just that it no longer displays garbage.
4. **Give the operator an honest, actionable reading of the input-level number itself.** "input level
   6%" told this real operator nothing about whether that was a problem. A quiet-but-real-signal band
   (above the 1% silence floor, below a documented "healthy" floor) now reads "LOW SIGNAL" with a
   concrete suggestion (move the microphone closer / raise its gain), instead of the same plain
   "SIGNAL CAPTURED" text used at any level above silence. The threshold is a judgment call, not a lab
   measurement - documented as such, matching this codebase's own established precedent for
   heuristic-but-undocumented-as-precise thresholds, and directly informed by this real operator's
   own evidence that 6% produced an unusable transcript.

## Explicitly deferred (and why)

**Software gain normalization / AGC before feeding Whisper** was seriously considered - boosting a
quiet signal before decode is a real, commonly cited mitigation for exactly this failure mode. It is
deliberately **not** implemented this phase: `WhisperSpeechEngine` buffers audio across many small
`AudioChunk` deliveries into one ~3-second window before running `full()`, so a gain stage applied
per-chunk (the natural place to intercept it, in `handle_audio_chunk`) would apply a different gain
factor to different sub-slices of the same inference window as the input level naturally fluctuates -
a real risk of introducing new pumping/discontinuity artifacts this environment has no real
microphone or ears to verify against. Getting this right needs either a whole-window-consistent gain
stage (a larger structural change) or real-hardware listening tests neither of which this phase can
respsonsibly ship unverified. Confirmed safe to defer without harming other consumers: `AudioChunk` is
already cloned to the acoustic-music worker *before* the speech-specific copy is sent onward
(`commands.rs`'s `sink` closure), so a future gain stage scoped to the speech path alone would not be
architecturally difficult to add later - it is being deferred on verification grounds, not
architectural ones.

## Scope boundaries

- No changes to Bible/Sermon/Music/Content detection logic - the evidence in "Claim 5" above does not
  support a defect there.
- No changes to `SILENCE_RMS_THRESHOLD` itself - it did its job correctly per Claim 3; this phase adds
  a second, independent, post-decode safeguard rather than trying to make one threshold do both jobs.
- No audio gain/normalization changes - see "Explicitly deferred" above.
- No change to how `AudioEngineStatus.input_level` itself is computed - only to how the frontend
  interprets and labels the number it already receives.
