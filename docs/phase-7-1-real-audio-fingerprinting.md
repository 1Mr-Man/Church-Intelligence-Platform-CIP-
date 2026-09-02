# Phase 7.1 — Real Audio Fingerprinting

## Baseline

Phase 6 (Operator Ergonomics) closed with all 8 of its own audit's gaps
addressed. Rather than continue slicing Phase 6, `docs/phase-7-audit.md`
re-verified the five master-architecture gaps `docs/phase-4-master-plan-gap-audit.md`
identified and left unordered. The user was asked to choose Phase 7's
theme among them; the question went unanswered in-session, so this phase
proceeds with the audit's own recommended, most contained option: **real
audio fingerprinting**.

## Audit finding

`integrations/music-acoustic`'s `LocalAcousticMusicRecognizer` was, since
Phase 2.2, a deliberately honest scaffold: real configuration and status
resolution, but `recognize()` always returned `Unavailable`/`Error` -
its own module docs named the exact seam a future phase would fill
("Phase 2.2 does not choose or implement a specific acoustic
fingerprint/embedding model architecture ... This struct is therefore
the real, compiling, testable *boundary* ... with a clearly documented
seam"). No algorithm existed anywhere in the codebase to recognize a
song from its audio - `core/music`'s only recognition path was lyric/
title text matching (Phase 2.1).

## Design choice

No genuine architectural fork existed for *what* to build: the seam was
already named (`AcousticMusicRecognizer::recognize()`), the manifest
convention already existed (`MODEL_MANIFEST_FILENAME`/model directory),
and the desktop wiring (`apps/desktop/src-tauri/src/lib.rs`,
`config.rs`) already resolves an acoustic model directory and passes it
to `LocalAcousticConfig` unconditionally - a genuine backend was always
going to plug into this exact seam with zero desktop-level changes. The
one real engineering decision was *which* fingerprinting algorithm:
spectral landmark (constellation) hashing - the technique described in
Wang, "An Industrial-Strength Audio Search Algorithm" (2003), the
published algorithm behind Shazam - was chosen because it is: (a) fully
offline (no network, no cloud API), (b) proven and well-understood
rather than a novel approach this project would need to validate from
scratch, (c) genuinely testable without real recordings (its
correctness properties - self-match, cross-song rejection, time-shift
invariance, noise tolerance - all hold on synthetic signals), and (d)
implementable in pure Rust with no C/C++ toolchain dependency (matching
this project's Windows cross-compilation discipline exactly the way
`hound` for WAV I/O and `rustfft` for the FFT were chosen over
alternatives that pull in native libraries).

## What was built

- **`integrations/music-acoustic/src/fingerprint.rs`** (new): the
  algorithm itself, with no knowledge of songs/datasets/files -
  `fingerprint(samples: &[i16]) -> Vec<Landmark>` runs a Hann-windowed
  STFT (`rustfft`, 1024-sample window, 512-sample hop), picks the
  strongest spectral peak per logarithmic frequency band per frame (the
  "constellation"), and pairs each peak with up to 3 nearby later peaks
  within a bounded target zone to produce landmark hashes (`u32`,
  packing both peaks' frequency bins and their frame delta). A
  `FingerprintIndex` accumulates `hash -> [(song_id, anchor_frame)]`
  during enrollment and, at query time, looks up every query landmark's
  hash and votes on `reference_frame - query_frame` per song - a real
  match concentrates votes on one offset (the query is one contiguous
  clip of the reference), while unrelated songs' coincidental hash
  collisions scatter across many offsets and never accumulate a
  majority.
- **`integrations/music-acoustic/src/local.rs`** (rewritten):
  `LocalAcousticMusicRecognizer::configure()` now parses the model
  manifest's extended schema (`{"songs": [{"songId", "contentId",
  "audioPath"}]}`), reads each named WAV file (`hound`, 16-bit PCM,
  downmixed to mono by channel averaging if needed), and enrolls it into
  a `FingerprintIndex` - once, at construction, exactly like
  `WhisperSpeechEngine::load`'s "fail/succeed at load time" pattern. One
  bad reference file (missing, malformed) is skipped and reported in
  `status_reason()`, not fatal to every other song. Status becomes
  genuinely `Available` once at least one song enrolls; `recognize()`
  queries the index, filters candidates to the caller's `content_ids`
  (via a `song_id -> content_id` map built at enrollment), and maps vote
  count to a conservative, capped `ConfidenceResult` (never `1.0` -
  acoustic fingerprinting is not scripture-reference-exact certainty).
- No change anywhere else in the codebase - `core/music`'s
  `AcousticMusicRecognizer` trait, `apps/desktop/src-tauri`'s
  orchestration/commands/events, and the frontend's Music Intelligence
  panel all already handle `Available`/`Unavailable`/`Error` and real
  `AcousticRecognitionCandidate`s from Phase 2.2 - this phase changes
  which concrete answers those existing paths receive, not their shape.

## New dependencies

`rustfft` (pure-Rust FFT) and `hound` (pure-Rust WAV codec) - both added
only to `integrations/music-acoustic`. Neither requires a C/C++
toolchain or native library, matching this project's existing
Windows-cross-compilation discipline (the same reasoning that has kept
`whisper-rs`/`candle` as the only native-toolchain dependencies, both
already solved by `scripts/build-windows-whisper.sh`).

## Algorithm correctness (synthetic audio)

Real recordings do not exist anywhere in this repository or container -
`docs/phase-4-master-plan-gap-audit.md`'s "Music Library is legitimately
empty in a production build" finding, unchanged since Phase 2.7.1. The algorithm's
correctness is therefore proven against synthetic multi-tone signals
(deterministic sine-wave sums, no audio file dependency, matching this
project's established synthetic-fixture discipline for pure-logic
tests):

- A clip matches itself with a large majority of its landmark hashes
  agreeing on one (zero) offset.
- Two acoustically different synthetic songs do not cross-match (the
  wrong song's vote count is held to under 1/4 of the right song's).
- A 3-second excerpt cropped from 2 seconds into an 8-second synthetic
  song still matches, via a nonzero but consistent offset - proving
  landmark hashing's core time-shift-invariance property, not just
  literal identity matching.
- Adding a moderate level of synthetic white noise to a copy of an
  enrolled song still matches.
- Silence, and querying an empty index, produce zero matches - never a
  fabricated result.
- The full enrollment -> WAV file -> real `FingerprintIndex` -> query
  round trip is proven with real (test-generated) WAV files via
  `hound`, not just the pure algorithm in isolation.

## Full regression result

`cargo fmt --all -- --check`: clean (one pass of `cargo fmt --all`
applied before commit). `cargo clippy --workspace --all-targets -D
warnings`: clean, both with and without `--features whisper,semantic-search`.
`cargo check --workspace`: clean, both feature configs. `cargo test
--workspace`: every crate passing, both feature configs (37 new tests in
`cip-integrations-music-acoustic`, 0 regressions anywhere else). Frontend
(`npm run typecheck`/`lint`/`test`/`build`): all clean/unchanged - this
phase touches zero frontend files, and 258/258 frontend tests still
pass.

## Windows rebuild

This phase changes Rust code compiled into the desktop binary (unlike
Phase 6.2-6.8's frontend-only slices) - a genuine Windows rebuild with
direct binary verification is required. See
`pilot-evidence/7.1/windows/installer-contents-verification.json`.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  schema - the entire change is behind `AcousticMusicRecognizer`, a
  trait that already existed and was already wired end to end since
  Phase 2.2.
- `core/music` (the domain contract crate) is untouched - it must never
  depend on a specific implementation, and does not; the new algorithm
  lives entirely in `integrations/music-acoustic`, matching the
  project's own architectural boundary rule exactly.
- `NullAcousticMusicRecognizer` and `ScriptedAcousticMusicRecognizer`
  (used whenever no real model is configured, and in tests) are
  byte-identical to before - this phase only changes
  `LocalAcousticMusicRecognizer`'s behavior once a real manifest is
  configured, which remains `None`/absent by default in every shipped
  build.
- The manifest schema is additive and backward-compatible in spirit
  with the Phase 2.2 convention (`MODEL_MANIFEST_FILENAME`, same
  directory location) - an operator who has not yet created a manifest
  sees the exact same `Unavailable` behavior as every prior phase.

## Environment A / B / C

- **Environment A** (this container): PASSED - full regression green as
  detailed above, including the real WAV-enrollment-to-recognition round
  trip against synthetic audio.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, real recorded music): NOT
  YET VERIFIED, and more consequentially so than most prior phases -
  this algorithm has never been tested against a real hymn/worship
  recording captured by a real microphone in a real room. Real-world
  audio carries reverb, multiple simultaneous voices/instruments,
  background noise, and level variation none of the synthetic test
  fixtures reproduce. The decisive pending gate is the operator's own
  real-hardware test: create an `acoustic-model.json` manifest naming
  one or more real WAV reference recordings of songs already in a Music
  dataset, restart the app, confirm Diagnostics Mode's acoustic status
  reports `Available`, then play (or have the app capture) one of those
  songs live and confirm it is correctly recognized as a `Pending`
  suggestion with a plausible confidence score - and, just as
  importantly, confirm an unrelated song or ambient service audio does
  NOT produce a false match.

## Known limitations

- **No real recorded reference audio exists anywhere in this
  repository or container** - the algorithm is proven on synthetic
  signals only; its real-world false-positive/false-negative rate
  against actual congregational singing, live instrumentation, and room
  acoustics is genuinely unknown until Environment C testing happens.
  This is the same honest gap this project has carried for the Whisper
  model and the semantic-search embedding model - a real, external
  asset this container's network policy and the absence of a licensed
  song library both prevent it from obtaining.
- **No enrollment UI** - creating `acoustic-model.json` and gathering
  reference WAV files is a manual, file-system-level operator task in
  this phase; a future phase could add an in-app "record/import a
  reference clip for this song" flow the way Bible/Whisper model
  installation already has a native file picker (Phase 3.8.7.1).
  Deliberately not attempted here to keep this phase's scope to "does
  the algorithm work," not "is it convenient to configure."
- **The manifest has no schema-version field** - a genuinely future
  concern only if the fingerprint hash packing (`WINDOW_SIZE`/
  `HOP_SIZE`/band edges) ever changes, since that would invalidate every
  previously-enrolled fingerprint; documented in `fingerprint.rs`'s own
  module docs rather than solved preemptively for a version-1 format.
- **`MIN_VOTES` (the minimum landmark-hash agreement threshold) is a
  fixed constant, not operator-tunable** - chosen conservatively from
  the synthetic test suite's own vote counts; real-world audio's actual
  vote distribution (louder/quieter recordings, more/less reverberant
  rooms) may call for tuning once Environment C data exists. Not made
  configurable preemptively, matching this project's "don't add a
  tunable before real evidence says it's needed" discipline.
- **Enrollment reads the entire reference WAV file into memory at
  once** - fine for hymn/song-length clips (a few minutes), not
  designed for hour-long source files; no such use case exists in this
  project's scope.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- Real-hardware Environment C verification with actual recorded music.
- An in-app enrollment/reference-recording workflow.
- Operator-tunable `MIN_VOTES` and/or band-edge configuration, if real
  data shows the current defaults need adjustment.
- The four other master-architecture gaps `docs/phase-7-audit.md`
  identified (multi-language support, church/user roles & permissions,
  internet/hybrid intelligence, OBS/vMix/livestream integration) -
  still open, still the user's choice for a future Phase 7.x or later
  top-level phase.

## Final gate

Environment A: **PASS**. Environment C: **PENDING**, and unusually
consequential for this phase specifically (no real audio has ever been
enrolled or queried against anywhere). This phase adds a real, offline,
from-scratch-implemented audio fingerprinting algorithm behind an
existing, already-shipped trait boundary - it introduces no new backend
surface (commands/events/schema) and changes no other recognizer's
behavior.
