# Acoustic Music Recognition (Phase 2.2)

This document explains the acoustic (audio-fingerprint/embedding) song
recognition foundation added in Phase 2.2, built directly on top of
Phase 2.1's Music Intelligence Foundation
([`docs/music-intelligence.md`](music-intelligence.md)). Read that
document first - this one only explains what's new: recognizing a song
from the audio itself, fused with the existing lyric/title matching, not
a replacement for it.

**Not in this phase:** a chosen acoustic model architecture, a trained
model, real-world acoustic recognition accuracy, a commercial acoustic
recognition provider, or automatic presentation/approval of anything
acoustic recognition produces. This phase builds the complete
*architecture* - segmentation, the signal-quality gate, the recognizer
contract, three real implementations of it, evidence fusion, ambiguity/
continuity/transition handling, and the operator workflow - and proves
every piece of it works, without pretending a real acoustic model exists
where none does. See "PROVEN vs NOT AVAILABLE / NOT VERIFIED" below.

## What "acoustic recognition" means here, honestly

Phase 2.1 recognizes songs by matching **text** - a spoken title, a hymn
number, or transcribed lyric words - against a locally installed
dataset. Phase 2.2 adds a second, independent recognition path:
recognizing a song from the **sound of the music itself**, the way a
commercial "name that tune" service does, but running entirely offline
against a locally configured model. These are genuinely different
capabilities, kept structurally distinct throughout:

- `cip_core_music::AcousticMusicRecognizer` is a trait, not an
  implementation - `core/music` defines what an acoustic recognizer
  looks like; it never performs recognition itself, never knows about a
  specific model architecture or vendor, and never touches Tauri,
  SQLite, or `cpal`.
- Every acoustic result carries an honest `AcousticRecognitionStatus`
  (`Available` / `Unavailable` / `Disabled` / `Error`) - `Unavailable` is
  a first-class, expected state, not an error to work around. Nothing in
  this codebase downloads model weights, hard-codes a commercial
  provider, or fabricates a result when no model is configured.
- `MatchType::Acoustic` (reserved since Phase 2.1) is now genuinely
  constructed - by evidence fusion, from a real
  `AcousticRecognitionCandidate` a real recognizer returned - never
  fabricated when no recognizer is configured.

## Architecture

```
Microphone
    v
AudioEngine                              (core/service, unchanged - integrations/audio)
    |  the same AudioChunk stream already feeds SpeechEngine
    v
AcousticWorkerState                      (apps/desktop/src-tauri/src/acoustic.rs)
    |  AudioSegmenter: bounded, overlapping windows
    v
AudioSegment
    |  assess_signal_quality()            (core/music/src/acoustic.rs)
    v
Silence / TooShort / LowQuality  ---->  skipped, no recognition attempt
    |
    Ready
    v
AcousticMusicRecognizer::recognize()      (integrations/music-acoustic - Null/Scripted/Local)
    v
AcousticRecognitionCandidate[]
    v
fuse_acoustic_with_context()              (core/music/src/fusion.rs - noisy-or, never averaging)
    |  combined with recent lyric/title-derived context evidence
    v
MusicIntelligenceEngine::analyze_acoustic()  (core/intelligence/src/music_adapter.rs)
    v
IntelligenceFinding[]                     (domain: Music, kind: Music - same finding model as Phase 2.1)
    v
FindingQueue                              (AppState.intelligence_findings, in-memory, shared with lyric findings)
    v
OPERATOR ACCEPT / REJECT                  (never automatic)
    v
CurrentSong                               (set only by an explicit accept; cleared only by an explicit clear)
    v
Optional presentation workflow            (existing, separate, manual - unchanged)
```

The trait sits in `core/music`, mirroring `cip_core_ai::SpeechEngine`
exactly: `core/ai` defines the speech contract, `ai/speech` supplies
`NullSpeechEngine`/`ScriptedSpeechEngine`/`WhisperSpeechEngine`;
`core/music` defines the acoustic contract,
`integrations/music-acoustic` supplies
`NullAcousticMusicRecognizer`/`ScriptedAcousticMusicRecognizer`/
`LocalAcousticMusicRecognizer`. `core/intelligence` depends on
`core/music` (never the other way around), so
`AcousticRecognitionStatus` is a distinct type from
`cip_core_intelligence::EngineCapability` despite the conceptual
overlap - `core/music` cannot depend on `core/intelligence`.

`analyze_acoustic` is a `MusicIntelligenceEngine` **inherent method**,
not part of the shared `IntelligenceEngine` trait. Acoustic recognition
operates on an `AudioSegment`, not a `TranscriptSegment`, so it does not
fit `IntelligenceInput`'s shape - adding it to the trait would force
every other domain engine (Bible included) to grow an acoustic-shaped
parameter it has no use for. `pipeline.rs` (the live Bible/transcript
pipeline) is completely unchanged; the acoustic worker is a second,
independent consumer of the same audio stream, wired inside
`start_listening`'s existing sink closure.

## Audio segmentation (`core/music::acoustic::AudioSegmenter`)

Turns a stream of raw mono PCM16 `AudioChunk`s (reusing
`cip_core_service::AudioChunk`'s exact format - no second audio type)
into bounded, overlapping `AudioSegment` windows:

- `window_ms` (default 8,000): length of one analysis window.
- `overlap_ms` (default 2,000): consecutive windows overlap by this
  much, so a song transition landing exactly on a window boundary is not
  missed entirely.
- The internal buffer never grows past one window's worth of samples
  while filling - "tolerate dropped chunks" is a design requirement
  (real-time audio that cannot be analyzed in time is stale by
  definition), proven by `segmenter_never_grows_the_buffer_unboundedly_while_filling`.
- `AudioSegment.started_at_ms` uses the same audio-relative clock
  convention `TranscriptSegment.start_ms`/`end_ms` already use
  (milliseconds since capture started, never wall-clock) - kept
  consistent rather than introducing a second time convention, and it
  avoids `core/music` needing a `chrono` dependency it otherwise has no
  use for.

## Signal-quality gate (`assess_signal_quality`)

A deterministic, RMS-based gate that runs before every (comparatively
expensive) recognizer call, never after:

| Result | Meaning |
| --- | --- |
| `Silence` | Empty buffer, or RMS indistinguishable from zero. |
| `TooShort` | Real signal, but shorter than `min_duration_ms` (default 3,000ms). |
| `LowQuality` | Long enough, but RMS below `min_rms` (default 80.0 on the `i16` scale) - present but too quiet/noisy to be worth an expensive recognition call. |
| `Ready` | Long enough and loud enough to attempt recognition. |

`min_rms`'s default is deliberately conservative: this gate must never
reject genuinely quiet-but-real singing, only silence/near-silence - see
`quiet_but_long_enough_is_low_quality` vs `loud_and_long_enough_is_ready`
in `core/music/src/acoustic.rs`'s tests for the exact boundary proven.

## Rate limiting (`AcousticWorkerState`, `apps/desktop/src-tauri`)

Segmentation alone does not bound how often the recognizer is actually
called - `minimum_recognition_interval_ms` (default 5,000ms) does that,
enforced by the worker, not the segmenter (`AudioSegmenter` only knows
about windowing). `AcousticWorkerState::should_attempt_recognition`
combines both independent reasons a window might be skipped (not
`Ready`, or too soon after the last attempt) into one deterministic
decision; `record_recognition_attempt` is called only once the worker
has actually gone on to call the recognizer - a window skipped because
no dataset is enabled does not consume the rate-limit budget, so
recognition resumes immediately once a dataset becomes enabled.

## The recognizer contract and its three implementations

```rust
pub trait AcousticMusicRecognizer: Send + Sync {
    fn status(&self) -> AcousticRecognitionStatus;
    fn method(&self) -> AcousticRecognitionMethod;
    fn status_reason(&self) -> Option<String> { None }
    fn recognize(
        &mut self,
        segment: &AudioSegment,
        content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError>;
}
```

`content_ids` is always pre-scoped to enabled Music datasets by the
caller (the recognizer itself has no concept of "enabled," mirroring
`MusicProvider`'s own dataset-scoping discipline) - every acoustic
result is dataset-scoped exactly like lyric/title matches are.

### `NullAcousticMusicRecognizer` - the safe default

Always reports `Unavailable`/`None`, rejects every `recognize()` call.
Used whenever no real recognizer is configured, so "no acoustic model"
is never fatal - lyric-based Music Intelligence keeps working
regardless, the same guarantee `NullSpeechEngine` gives transcription.

### `ScriptedAcousticMusicRecognizer` - the deterministic test/demo adapter

Ignores the audio it's given and steps through a pre-programmed script
of outcomes: `Candidates(Vec<AcousticRecognitionCandidate>)`,
`NoResult`, `Error(reason)`, `Unavailable(reason)`. Always reports
`Available`/`Test`. This is Phase 2.2's primary deterministic test
adapter, exercising every manual-test-mode scenario the spec requires
(a single candidate; an A+B ambiguity pair; no result; a recognizer
error; a recognizer-unavailable outcome; a transition, expressed as two
consecutive `Candidates` steps naming different songs) without a
microphone or a real model. Also honors `content_ids` defensively -
a scripted candidate naming a dataset outside `content_ids` is silently
dropped, so a dataset-isolation test cannot be defeated by a
misconfigured fixture.

### `LocalAcousticMusicRecognizer` - the real integration boundary

Mirrors `WhisperSpeechEngine::load`'s pattern: genuine configuration,
genuine status resolution from real file-system facts, resolved once at
construction (never re-checked per call). Configuration is
`{ model_dir: Option<PathBuf>, enabled: bool }`; status resolves as:

| Condition | Status | Reason |
| --- | --- | --- |
| `enabled: false` | `Disabled` | "acoustic recognition explicitly disabled" |
| `model_dir: None` | `Unavailable` | "no acoustic model directory configured" |
| configured directory does not exist | `Unavailable` | names the missing directory |
| directory exists, no `acoustic-model.json` manifest | `Unavailable` | "no model manifest found at ..." |
| manifest exists but is empty | `Error` | "model manifest is empty (malformed)" |
| manifest exists and is non-empty | `Unavailable` | "a model manifest is present, but no acoustic inference backend is implemented in this build" |

That last row is the key honesty point: **a present, well-formed
manifest is still never enough to report `Available`** - no real
inference backend (a chosen fingerprint/embedding model architecture,
loaded and run against real audio) is implemented in this build. Section
7 of the Phase 2.2 spec explicitly permits this: "acoustic recognition
architecture implemented and tested, real-world acoustic recognition not
verified in this environment" is a successful outcome, not a shortfall
to paper over. `docs/live-speech.md`'s Whisper section is the precedent
- a real, working inference backend (whisper.cpp) exists there and is
only blocked by the absence of a downloadable model file; here, no
inference backend has been chosen or implemented at all, so `recognize()`
never has anything to honor even if a manifest is present. The seam is
explicit and documented (`LocalAcousticMusicRecognizer`'s module docs)
for a future phase to fill in, the same way the `whisper` Cargo feature
was added to `ai/speech` once whisper-rs was chosen.

## Model-agnostic design

Nothing in `core/music`, `core/intelligence`, or the fusion/evidence
model knows or cares which acoustic method produced a candidate -
`AcousticRecognitionMethod` (`LocalModel` / `ExternalProvider` / `Test` /
`None`) records *what kind* of thing produced a result, never a specific
vendor/model name. `ExternalProvider` is reserved for a future,
explicitly-configured external service; nothing in this codebase
constructs it today - Phase 2.2 does not hard-code a commercial acoustic
recognition provider anywhere. A future real backend (local or external)
plugs into the existing `AcousticMusicRecognizer` trait without any
change to `MusicIntelligenceEngine`'s public contract.

## Evidence fusion (`core/music::fusion::fuse_acoustic_with_context`)

The central new piece of decision logic this phase adds - combining
acoustic and lyric/title evidence into one ranked candidate list without
creating a second, competing finding/confidence system:

- Fresh acoustic candidates for the same song (e.g. two overlapping
  analysis windows) are first collapsed to the single strongest one -
  duplicate observation of the same evidence must never inflate
  confidence just because it was seen twice.
- For each surviving acoustic candidate, a matching context-evidence
  entry (built by the intelligence layer from the single most recent
  Music-domain finding in `context.recent_findings` - reusing Phase
  2.1's exact continuity extraction convention, not a second history
  mechanism) is looked up by `(content_id, song_id)`.
- If a match exists, the two confidences are combined with **noisy-or**
  (`1 - (1 - a) * (1 - b)`, clamped to `0.99`), never a simple average:
  independent, agreeing evidence should raise confidence *beyond* either
  source alone, but fused heuristic evidence must never claim absolute
  (`1.0`) certainty. `fusion_is_not_a_simple_average` proves
  `noisy_or(0.9, 0.9) = 0.99`, far above the average (`0.9`).
- If no match exists, the acoustic candidate passes through unchanged -
  **context evidence never invents a candidate that acoustic recognition
  did not itself produce** (`context_evidence_never_invents_a_candidate_not_already_acoustic`).
  Continuity/context can only ever *strengthen* an existing acoustic
  candidate, never create one from nothing.
- Conflicting songs (two acoustic candidates naming different songs)
  remain fully distinguishable in the output, never merged.
- Dataset isolation is preserved through fusion: context evidence for
  dataset A never strengthens an acoustic candidate for the
  same-numbered song in dataset B.
- Output is sorted deterministically (`confidence desc, song_id asc`)
  with `ranking` reassigned, exactly like `matcher::search_songs` - the
  same list can be fed straight into `matcher::is_ambiguous`.

## Confidence policy - reused, not reinvented

There is no `MusicConfidenceEngine` or second scoring system. Acoustic
candidates carry whatever `ConfidenceResult` the recognizer itself
reports; fusion combines two `ConfidenceResult`s via noisy-or (still the
existing `cip_core_confidence::ConfidenceResult`/`ConfidenceLevel`
types, nothing new). Confidence never itself determines
approval/projection/presentation - only an explicit operator action
does that, unchanged from every earlier phase.

## Ambiguity - the same policy, applied to fused results

`analyze_acoustic` reuses Phase 2.1's exact `is_ambiguous`/
`ambiguity_margin` policy (`top_score - second_score < margin`) on the
*fused* candidate list. When ambiguous, only candidates within the
margin of the top score are emitted as findings (bounded by the same
`MAX_FINDINGS_PER_CALL = 5`), each summarized as "Possible song
(acoustic): ... (operator confirmation required)" - never silently
resolved. `ambiguous_acoustic_candidates_emit_both_for_operator_choice`
proves two close candidates both reach the operator.

## Song continuity and transitions

Continuity is classified with the same `cip_core_music::classify_continuity`
Phase 2.1 uses, fed the same single-most-recent-Music-finding lookup
(`previous_song`) - now shared by both the lyric and acoustic finding
builders in `music_adapter.rs`, so there is exactly one continuity rule
in the codebase, not two that could disagree. A song transition (song A
recognized, then song B) is simply two consecutive acoustic recognitions
naming different songs; it is detected and surfaced as an ordinary new
`Detected` finding for song B - **never automatically declared
current**. `a_later_candidate_for_a_different_song_never_silently_becomes_current`
(`apps/desktop/src-tauri/src/acoustic.rs`) proves this end to end: song A
is accepted and becomes current, song B is then detected, and
`current_song` still names song A until an operator explicitly accepts
B.

## "Current Song" - deliberately minimal state

Only one genuinely new piece of mutable state exists:
`AppState.current_song: Option<CurrentSong>`
(`{ content_id, song_id, confidence }`), set **only** by
`accept_music_finding` deriving one from the just-accepted finding's own
evidence, and cleared **only** by the explicit `clear_current_song`
command. Nothing else ever writes to it - not a high-confidence acoustic
candidate, not continuity, not a transition detection.

The three further concepts the spec names (`NoSong` / `CandidateSong` /
`PossibleTransition`) are **derived, not stored** - avoiding "state
explosion." A frontend or future diagnostic can compute them at read
time from `(current_song, pending Music findings)`:
`current_song == None` and a pending finding exists is a candidate;
`current_song == Some(x)` and a pending finding names a different song
is a possible transition; `current_song == Some(x)` and no pending
finding disagrees is simply confirmed. No Rust type or table exists for
these three states - they are a display-time computation, not
persisted, matching Phase 2.2's "avoid state explosion" instruction.

`current_song_from_finding` (`apps/desktop/src-tauri/src/music.rs`)
decodes the same `EvidenceSource::Context { description: "song_id:<id>" }`
convention `previous_song` reads - works identically for a lyric-sourced
or acoustic-sourced finding, so "what is the current song" has exactly
one derivation rule regardless of how the song was recognized.

## Finding lifecycle - unchanged

Acoustic-sourced findings are ordinary `IntelligenceFinding`s, moving
through the exact same Phase 2.0 lifecycle
(`Detected -> Reviewed -> Accepted`/`Rejected`/`Expired`) lyric findings
already use. No `Projected`/`Displayed`/`Current` state was added to
`IntelligenceFinding` itself - `acoustic_sourced_findings_never_start_anything_but_detected`
proves every finding `analyze_acoustic` produces starts `Detected`, the
same structural guarantee (`FindingQueue`/`music.rs` have no dependency
on `cip_core_presentation` at all) Phase 2.1 already established.

## Evidence: the new `Acoustic` variant

```rust
Acoustic {
    segment_id: Uuid,
    method: String,
    duration_ms: u64,
}
```

Added as a new `EvidenceSource` variant (`core/intelligence::evidence`) -
plain facts about *how* the underlying `AcousticRecognitionCandidate`
was produced, mirrored as plain data the same way `Content`/`Context`
already carry plain strings rather than importing another domain's enum
type. Never a claim that acoustic recognition is more certain than the
finding's own `confidence` score already says. An acoustic-sourced
finding's evidence list starts with this entry (when the original
acoustic candidate's segment/method/duration are still recoverable
through fusion's dedup step), followed by the same `Context`-entry
evidence-string convention lyric findings already use, followed by the
same `song_id:<id>` continuity-carrying entry.

## Operator workflow / Tauri commands

Reusing existing commands wherever they already provide the behavior,
per the explicit "never duplicate IPC commands" instruction:

- `list_music_findings()`, `accept_music_finding(findingId)`,
  `reject_music_finding(findingId)` - **unchanged**, Phase 2.1 commands,
  now also handling acoustic-sourced findings (a finding is a finding
  regardless of source). Accepting a Music finding now additionally
  derives and sets `current_song`, emitting `CURRENT_SONG_CHANGED`.
- **Ambiguity resolution has no dedicated command.** When
  `analyze_acoustic` reports ambiguity, it emits multiple competing
  `Detected` findings (identical to Phase 2.1's lyric-path ambiguity
  handling) - the operator "resolves" ambiguity by calling
  `accept_music_finding` on the one they want and, optionally,
  `reject_music_finding` on the others. A separate
  `resolve_music_ambiguity` command would only duplicate that existing
  behavior.
- `clear_current_song()` - **new**. The only other way `current_song`
  ever changes. Emits `CURRENT_SONG_CHANGED` with a `null` payload only
  when there was actually a song to clear.
- `analyze_music_audio(samples, sampleRateHz)` - **new**. The
  deterministic acoustic-analysis harness, the Phase 2.2 counterpart to
  `analyze_music_transcript`: wraps raw mono PCM16 samples into one
  `AudioSegment` (bypassing the worker's windowing/rate-limiting, since
  a manual single-shot call is not subject to the same "how often can
  the recognizer run" concern live audio is), still gated by the same
  signal-quality check, and runs the same `recognize_fuse_and_queue`
  path the live worker uses - so a configured
  `ScriptedAcousticMusicRecognizer` exercises the exact real pipeline
  end to end without a microphone.
- **No dedicated `get_acoustic_music_status` command.** Its data
  (`{ status, method, reason }`) is reused as a new
  `LiveStatus.acousticStatus` field on the existing `get_live_status`
  diagnostic command, alongside the new `LiveStatus.currentSong` field -
  the frontend already polls `get_live_status` for every other engine's
  status, so this avoids a second, redundant query surface.

Every command validates its own input, returns a typed `AppError`, and
never panics - the same discipline every earlier phase's commands
follow.

## Live audio integration - a second consumer, not a pipeline rewrite

`pipeline.rs` (the Bible/transcript pipeline) is untouched. The acoustic
worker is wired as a *second* consumer inside `start_listening`'s
existing sink closure, alongside (never replacing) the speech-engine
feed:

```rust
let sink: AudioChunkSink = Arc::new(move |chunk: AudioChunk| {
    let _ = acoustic_tx.try_send(chunk.clone());  // Phase 2.2 - bounded, never blocks
    handle_audio_chunk(&sink_app, service_id, chunk);  // Phase 1.2 - unchanged
});
```

The acoustic worker runs on its own background `std::thread`, reading
from a bounded `std::sync::mpsc::sync_channel`
(`ACOUSTIC_CHANNEL_CAPACITY = 8`). If the worker falls behind, the
sender uses `try_send` and drops the newest chunk rather than blocking
the audio capture thread or the speech-engine feed right after it - a
dropped chunk here only means one less acoustic analysis window, never
lost transcript audio (only the acoustic channel's own sender is
touched). The channel closes automatically and cleanly when
`stop_listening`/a failed `start_listening` causes the audio engine to
drop the sink holding the sender's clone - the worker thread's blocking
`recv()` then returns an error and the thread exits on its own; nothing
needs to be explicitly joined or cancelled.

## Failure isolation - proven, not assumed

Every scenario the spec names is either structurally impossible (by
construction) or directly tested:

| Scenario | How it's guaranteed |
| --- | --- |
| Acoustic model unavailable | `NullAcousticMusicRecognizer`/an honestly-`Unavailable` `LocalAcousticMusicRecognizer` reject `recognize()`; lyric-based Music Intelligence (`MusicIntelligenceEngine::analyze`) is a completely separate code path, unaffected - proven by `acoustic_analysis_failure_does_not_affect_lyric_recognition_on_the_same_engine`. |
| Recognizer throws an error mid-service | The worker's `match outcome { Err(e) => ... }` branch logs and records a timeline entry, then the loop simply continues to the next window - proven the error is surfaced, not silently swallowed, by `recognize_fuse_and_queue_surfaces_a_recognizer_failure_never_silently_swallowed`. |
| Audio/capture device disappears | The acoustic channel is tied to the same sink the speech engine reads from - if capture stops, both stop together, cleanly (see "Live audio integration" above); no separate failure mode to handle. |
| Speech recognition fails | The sink closure sends to the acoustic channel *before* calling `handle_audio_chunk` (the speech-feeding function) - two independent statements, neither able to affect the other's outcome. A speech-engine error inside `handle_audio_chunk` never touches the acoustic channel at all. |
| Dataset disabled | `analyze_acoustic` independently re-filters `acoustic_candidates` to only currently-enabled Music datasets, as defense in depth on top of the caller already scoping `content_ids` - proven by `a_disabled_dataset_never_resolves_acoustic_candidates`/`acoustic_recognition_with_no_enabled_music_content_yields_no_findings_and_no_error`. |
| Database (context-building) failure | The worker's `build_music_context` call is wrapped in a `match`; a failure is logged and the loop continues to the next window rather than propagating or panicking. |
| One recognition window is malformed/errors | Never affects a later window - the worker loop has no shared mutable state across iterations except the deliberately-isolated `AcousticWorkerState` (segmenter + rate-limit timestamp), which a failed recognition attempt does not corrupt. |

## Database - no new tables

Acoustic findings live in the same in-memory `FindingQueue` lyric
findings already use - no `raw_audio` table, no persisted microphone
recordings, no new `acoustic_findings`/`acoustic_candidates` table. Raw
audio is captured, analyzed, and discarded: `AudioSegment`s exist only
in memory for the duration of one `recognize()` call and are dropped
immediately after. This matches Phase 2.1's own persistence decision
(`docs/music-intelligence.md`'s "Persistence: findings stay in-memory"
section) and Phase 2.2's explicit privacy requirement - see below.

## Privacy

Audio is sensitive. By default: audio is captured, analyzed locally, and
discarded - never uploaded, never sent to a cloud service, never
persisted to disk in raw form. `LocalAcousticMusicRecognizer` runs
entirely on-device; `ExternalProvider` (a hypothetical future networked
recognizer) is a reserved enum variant nothing in this codebase
constructs. No telemetry of any kind is added by this phase. If a future
phase needs to persist audio for some explicit, operator-consented
reason, that is a deliberate, separately-justified, separately-reviewed
addition - not something this phase does implicitly.

## Configuration (`apps/desktop/src-tauri::config::AcousticConfig`)

Explicit, documented settings, each independently overridable by an
environment variable (mirroring `AppEnvironment::resolve`'s convention -
a sensible default, overridable without a rebuild, never a hard-coded
environment-specific path):

| Field | Env var | Default |
| --- | --- | --- |
| `enabled` | `CIP_ACOUSTIC_ENABLED` | `true` (safe: with no model manifest present, recognition still honestly reports `Unavailable`) |
| `model_dir` | `CIP_ACOUSTIC_MODEL_DIR` | `<app data dir>/models/acoustic` |
| `minimum_audio_ms` | `CIP_ACOUSTIC_MIN_AUDIO_MS` | 3,000 |
| `analysis_window_ms` | `CIP_ACOUSTIC_WINDOW_MS` | 8,000 |
| `overlap_ms` | `CIP_ACOUSTIC_OVERLAP_MS` | 2,000 |

`min_rms` and `minimum_recognition_interval_ms` remain internal tuning
constants on `AcousticAnalysisConfig`'s own `Default` impl (not exposed
as separate environment variables in this phase, to avoid widening the
configuration surface beyond what a real deployment has needed so far);
`MAX_FINDINGS_PER_CALL`/`ambiguity_margin` continue to reuse Phase 2.1's
existing, already-tested `MusicIntelligenceEngine` constants rather than
being reconfigured per acoustic call, so acoustic and lyric findings
share one bounding policy, not two.

## Observability

The `LogCategory::Music` target (already used by Phase 2.1's timeline
recording) is reused for acoustic logging - recognizer construction
status is logged once at startup
(`create_acoustic_recognizer` in `lib.rs`), and every recognition
failure is logged with its reason. No raw audio, no unnecessary
transcript text, and no other sensitive data is ever logged.

## Events

Only one genuinely new event: `CurrentSongChanged`
(`CURRENT_SONG_CHANGED`, payload `CurrentSong | null`). Acoustic-sourced
findings reuse the existing `MusicFindingDetected` event
(`MUSIC_FINDING_DETECTED`) - Phase 2.1's event already carries a plain
`IntelligenceFinding` regardless of whether lyric or acoustic evidence
produced it, so there is no separate
`AcousticSongCandidateDetected`/`MusicSongTransitionDetected` event: a
transition is just another `MusicFindingDetected` finding, distinguished
(if at all) by its own summary text and evidence, not by a different
event type. Both events have frontend mirrors
(`src/events/eventNames.ts`) and dedicated tests proving every event
name is unique and `SCREAMING_SNAKE_CASE`.

## Timeline / audit

Operator actions - accept, reject, and clear-current-song - are recorded
to the existing `audit_events`-backed timeline exactly like every other
operator action (`record_timeline`, `LogCategory::Music`). Internal
per-window recognition attempts are **not** written to the timeline -
only genuinely new findings and operator decisions are, avoiding
flooding `audit_events` with one row per analysis window (a window
occurs at most roughly once every `minimum_recognition_interval_ms`, but
even that would be excessive audit noise for something that isn't an
operator-visible decision).

## Frontend

Extends the existing Music Intelligence panel in `LiveChurchBrain.tsx`
rather than creating a competing dashboard:

- An acoustic-status line reads `LiveStatus.acousticStatus`, showing the
  real status/method/reason - never a fake "AI ready" indicator when the
  status is anything but `available`.
- A "Current Song" block shows the confirmed song (content id, song id,
  confidence) with an explicit "Clear current song" button when set, or
  an honest "No current song" hint (explaining that a pending finding is
  only ever a candidate) when not.
- Each pending finding in the existing list gets a small "acoustic"
  badge when its evidence includes an `Acoustic` entry, so an operator
  can tell at a glance which findings came from audio versus text.
- `onCurrentSongChanged` is subscribed alongside the existing Music
  event listeners, patching `status.currentSong` immediately on push
  rather than waiting for the next 3-second status poll.

All new domain types (`domain/music.ts`'s `AcousticRecognitionStatus`/
`AcousticRecognitionMethod`/`AcousticEngineStatus`/`CurrentSong`,
`domain/intelligence.ts`'s `Acoustic` `EvidenceSource` variant,
`domain/live.ts`'s `LiveStatus` additions) and every new command/event
go through the same runtime-safe `invokeCommand`/`listenSafe` wrappers
every earlier phase's IPC does - a plain web build outside Tauri never
throws calling any of them, it simply rejects with
`TauriUnavailableError` or resolves a no-op unlisten, exactly like every
existing wrapper.

## Test fixtures - fictional, never copyrighted

Every test candidate/fixture song id (`"h1"`, `"h2"`, `"s1"`, `"s2"`) and
every synthetic audio sample (`vec![10_000; n]` for "loud," `vec![0; n]`
for "silent") used throughout this phase's tests is fabricated
specifically for the test - never a real commercial recording, never
real copyrighted audio, matching Phase 2.1's "fictional titles/lyrics
only" discipline (`docs/music-intelligence.md`'s copyright section).

## Testing

- `core/music`'s acoustic module (24 tests): signal-quality gate
  boundaries (silence/too-short/low-quality/ready), segmenter windowing
  and overlap, bounded-buffer behavior, zero-sample-rate handling,
  status/method/candidate serde.
- `core/music`'s fusion module (12 tests): acoustic-only passthrough,
  noisy-or strengthening, "not a simple average," "context never
  invents," conflicting-songs distinguishability, duplicate-evidence
  non-inflation, dataset isolation through fusion, deterministic
  sort/ranking.
- `integrations/music-acoustic` (19 tests): `NullAcousticMusicRecognizer`
  (2), `ScriptedAcousticMusicRecognizer` (10, covering every manual-test-
  mode scenario), `LocalAcousticMusicRecognizer` (7, covering
  disabled/no-dir/nonexistent-dir/no-manifest/malformed-manifest/
  present-manifest-still-honest/never-panics).
- `core/intelligence::music_adapter` (20 new tests, on top of Phase
  2.1's existing 10): acoustic-only findings, acoustic evidence in the
  finding, dataset filtering, ambiguity, fusion-driven confidence
  strengthening from a real accepted lyric finding, unrelated recent
  findings never leaking confidence, duplicate-window non-inflation,
  determinism.
- `core/intelligence::acceptance_tests` (4 new tests, extending Phase
  2.0/2.1's multi-engine acceptance suite): acoustic/lyric failure
  isolation on the same engine, acoustic + Bible sharing one context
  without calling each other, acoustic findings never starting anywhere
  but `Detected`, no-enabled-content degradation.
- `core/intelligence::queue` (2 new tests): `FindingQueue::all()`
  (needed so continuity/fusion can see an *accepted* finding's history,
  not just still-pending ones) includes accepted/rejected findings,
  preserves insertion order.
- `apps/desktop/src-tauri::acoustic` (15 tests): worker windowing/rate-
  limiting, `recognize_fuse_and_queue`'s full pipeline (success, no-
  result, recognizer-unavailable, recognizer-failure), the operator
  workflow (`operator_accept_is_the_only_way_a_current_song_is_derived`),
  and the transition-never-auto-promotes proof
  (`a_later_candidate_for_a_different_song_never_silently_becomes_current`).
- `apps/desktop/src-tauri::music` (2 new tests): `current_song_from_finding`
  against a real accepted finding, and against a non-Music finding.
- `apps/desktop/src-tauri::config` (3 new tests): acoustic config
  defaults, env-var parsing helpers.
- Frontend: domain-contract tests for every new TS type
  (`AcousticEngineStatus`, `CurrentSong`, the `Acoustic` evidence
  variant, the extended `LiveStatus`), command-wrapper tests
  (`analyzeMusicAudio`, `clearCurrentSong`, including the outside-Tauri
  guard), event-subscription tests (`onCurrentSongChanged`), and the
  updated event-count test (29 events, up from 28).

```sh
cargo test -p cip-core-music
cargo test -p cip-integrations-music-acoustic
cargo test -p cip-core-intelligence
cargo test -p cip-desktop acoustic::
cargo test -p cip-desktop music::
npx vitest run   # from apps/desktop
```

## Performance

Measured directly (`std::time::Instant`, release build, this machine,
one run - not a formal benchmark harness, using throwaway test code
deleted before commit, matching the Phase 2.1 measurement methodology):

| Operation | Observed |
| --- | --- |
| `assess_signal_quality` (including building a fresh 128,000-sample `AudioSegment`, i.e. one full 8-second window at 16kHz) | ~78.6µs |
| `AudioSegmenter::push` (one 16,000-sample/1-second chunk) | ~2.0µs |
| `fuse_acoustic_with_context` (2 acoustic candidates + 1 corroborating context entry) | ~1.4µs |
| `MusicIntelligenceEngine::analyze_acoustic` (full path: filter, dedup, fuse, ambiguity, finding construction) | ~1.5µs |

Every stage of the pipeline runs in low single-digit microseconds except
the segment-quality check, which is dominated by copying one full
window's worth of audio into a fresh `AudioSegment` (a one-time,
unavoidable cost of handing the recognizer an owned buffer) rather than
the RMS computation itself. At the default 8-second window with a
5-second minimum recognition interval, total analysis-pipeline CPU cost
per attempted recognition is on the order of tens of microseconds -
several orders of magnitude below the budget a real-time audio pipeline
needs, even before accounting for a real inference backend's own cost
(which this build does not have, so it is not part of this measurement -
see "NOT AVAILABLE" below).

## Offline dependency proof

```sh
cargo tree -p cip-core-music
cargo tree -p cip-core-intelligence
cargo tree -p cip-integrations-music-acoustic
```

None of the three shows `reqwest`, `hyper`, `ureq`, `surf`,
`tungstenite`, `tokio-tungstenite`, or any other network-capable crate -
verified directly, the same proof every earlier phase established for
its own domain. `integrations/music-acoustic` depends only on
`cip-core-music`, `cip-core-confidence`, and `uuid` (plus `tempfile` as
a dev-only dependency for its own filesystem tests) - no SQLite, no
Tauri, no audio capture library, no network client.

## PROVEN vs NOT AVAILABLE / NOT VERIFIED

Read this section before drawing any conclusion about "does acoustic
recognition work."

### PROVEN (implemented, tested, and directly verified in this environment)

- The `AcousticMusicRecognizer` trait boundary and all three
  implementations (`Null`/`Scripted`/`Local`) compile, behave correctly,
  and never panic under any tested condition.
- Audio segmentation: bounded, overlapping windows, correct handling of
  zero sample rate, correct handling of an accumulating vs. draining
  buffer.
- The signal-quality gate's exact silence/too-short/low-quality/ready
  boundaries.
- Evidence fusion's full documented policy: noisy-or combination, "never
  a simple average," "context never invents a candidate," dataset
  isolation preserved, duplicate-evidence non-inflation, deterministic
  ranking.
- Ambiguity handling: close candidates both reach the operator, never
  silently resolved.
- Song continuity and transition detection, end to end through real
  `MusicIntelligenceEngine`/`FindingQueue` state, including "a
  transition is detected but never auto-promoted to current."
- The full operator workflow: detect -> (optionally ambiguous, operator
  chooses) -> accept -> `current_song` set -> clear -> `current_song`
  cleared - verified with real `FindingQueue`/`SqliteMusicProvider`
  state, not synthetic doubles alone.
- Failure isolation: every scenario in the table above, each backed by a
  passing test or a structural (type-level) guarantee.
- Offline behavior: the entire acoustic architecture (segmentation,
  quality gate, `Null`/`Scripted` recognizers, fusion, ambiguity,
  continuity, the operator workflow, and the honestly-`Unavailable`
  `LocalAcousticMusicRecognizer`) works with zero network access,
  because none of `core/music`, `core/intelligence`, or
  `integrations/music-acoustic` depends on a network-capable crate at
  all.
- Regression: Phase 1.1 (Bible detection/context), Phase 1.2 (speech
  pipeline, offline fallback), Phase 1.3 (service lifecycle, suggestion
  workflow), Phase 1.4 (presentation preview/prepare safety), Phase 1.5
  (Content Registry, Bible/Music datasets), Phase 2.0 (unified
  `IntelligenceContext`, registry, failure isolation), and Phase 2.1
  (lyric/title recognition, dataset isolation, `MusicIntelligenceEngine`)
  all still pass unchanged - the full workspace test suite
  (`cargo test --workspace`, `cargo test -p cip-ai-speech --features
  whisper`, and the frontend's `npx vitest run`) is green.

### NOT AVAILABLE / NOT VERIFIED in this environment

- **Real-world acoustic recognition accuracy.** No acoustic model
  architecture was chosen, trained, or implemented in this phase -
  `LocalAcousticMusicRecognizer` never reaches `Available` in this
  build, by design (see "the real integration boundary" above). There is
  therefore nothing to measure recognition accuracy of.
- **Real microphone/hardware acoustic performance.** This environment
  has no audio input hardware to capture real congregational singing
  from; the acoustic worker's live-audio path is proven correct in
  design and in its unit-testable pieces, but was not exercised against
  a real microphone stream in this environment (mirroring
  `docs/live-speech.md`'s existing Whisper caveat for real speech
  input).
- **Recognition of real commercial recordings.** No copyrighted audio of
  any kind was used anywhere in this phase's development or testing -
  see "Test fixtures" above - so nothing here demonstrates recognizing
  an actual hymn/worship recording.
- **A trained model's memory/CPU footprint or inference latency.** The
  performance table above measures the architecture *around* a
  recognizer (segmentation, the quality gate, fusion, finding
  construction) - it does not and cannot measure a real inference
  backend's own cost, because no such backend exists in this build.

The correct, honest summary: **acoustic recognition architecture
implemented and tested; real-world acoustic recognition not verified in
this environment.** That is the successful, complete outcome Phase 2.2's
own spec asks for - not a shortfall.

## Known limitations

- `LocalAcousticMusicRecognizer` will never report `Available` until a
  future phase chooses and implements a real inference backend (the same
  way `WhisperSpeechEngine` required the `whisper` Cargo feature and a
  real model file before speech transcription became real).
- `min_rms`/`minimum_recognition_interval_ms` are not yet independently
  configurable via environment variables (only via
  `AcousticAnalysisConfig`'s Rust-level `Default`) - a deliberate
  scope-limiting decision to avoid widening the configuration surface
  before a real deployment has needed it.
- Ambiguity resolution has no dedicated Rust-side "resolve" concept
  beyond accept/reject on the competing findings - this matches Phase
  2.1's own lyric-path ambiguity handling exactly, so it is a consistent
  design choice, not an acoustic-specific gap.
- `CandidateSong`/`PossibleTransition` are not exposed as a Rust type or
  IPC field - they are documented as a frontend-derivable computation
  (see "Current Song" above) but the current frontend panel does not yet
  compute/display them as distinct labeled states, only the underlying
  data (`currentSong`, pending findings) they would be derived from.

## See also

- [`docs/music-intelligence.md`](music-intelligence.md) - Phase 2.1's
  lyric/title recognition foundation this phase builds on.
- [`docs/intelligence-architecture.md`](intelligence-architecture.md) -
  the shared `IntelligenceContext`/`IntelligenceEngine`/`FindingQueue`
  architecture both recognition paths are built on.
- [`docs/live-speech.md`](live-speech.md) - the `SpeechEngine` pattern
  `AcousticMusicRecognizer` mirrors, including the same "honest model
  absence" precedent `LocalAcousticMusicRecognizer` follows.
- [`docs/music-datasets.md`](music-datasets.md) - dataset import format
  and the licensing policy this phase's fixtures also follow.
