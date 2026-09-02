# Phase 7.2 — Real Audio Fingerprinting: Enrollment Workflow

## Baseline

Phase 7.1 replaced `LocalAcousticMusicRecognizer`'s honest Phase 2.2
scaffold with a real, offline spectral landmark hashing recognizer. Its
own "Known limitations"/"Deferred work" named the most obvious next gap:
creating `acoustic-model.json` and gathering reference WAV files was a
manual, file-system-level operator task, undocumented anywhere an
operator would actually look. See `docs/phase-7-2-audit.md`.

## Audit finding

No enrollment path existed anywhere in the app - an operator would need
to hand-write JSON matching `local.rs`'s (then-private) manifest schema
and manually place a WAV file at a path they referenced correctly. This
project had already solved the identical problem twice before: Phase
3.8.7.1 (Whisper model) and Phase 4.4 (embedding model + tokenizer) both
use a native file picker + an `install_*` Tauri command that validates,
copies, and logs "restart CIP for it to take effect."

## Design choice

No genuine fork: the precedent was already set twice. Two real decisions,
both resolved by following it rather than guessing:

1. **Live reconfigure vs. restart-required** - `AppState`'s recognizers/
   engines are all built once at startup and never hot-swapped; this
   phase follows that discipline exactly rather than inventing a new
   live-reload mechanism this codebase has nowhere else.
2. **How an operator identifies which song a recording belongs to** -
   reused `search_music` (Phase 2.1) as-is rather than building new
   search logic.

## What was built

- **`integrations/music-acoustic/src/local.rs`**: `Manifest`/
  `ManifestSong` made `pub` (gained `Serialize` alongside their existing
  `Deserialize`, so they round-trip); `enroll_one`'s WAV-decode/validation
  logic extracted into a shared `decode_reference_wav`, with a new `pub
  fn validate_reference_wav` wrapping it - the exact same check both
  enrollment and the new Tauri command use, so a file that passes
  validation is guaranteed to enroll identically, never merely similarly.
  New `pub fn read_manifest_entries`/`write_manifest_entries` for
  whole-manifest read/replace (an absent or empty manifest reads back as
  an empty list, not an error - the same "nothing configured yet is not
  a failure" discipline `resolve` itself already follows).
- **`apps/desktop/src-tauri/src/commands.rs`**: `list_acoustic_enrollments`
  (reads the manifest's current entries) and
  `enroll_acoustic_reference(song_id, content_id, source_path)`
  (validate -> copy into the model directory -> upsert the manifest entry
  by `song_id`, replacing any prior enrollment of the same song -> log
  "restart to take effect"), placed alongside and mirroring
  `install_whisper_model`/`install_embedding_model_file` exactly.
- **Frontend**: `domain/music.ts` gains `AcousticEnrollment`; `commands.ts`
  gains `listAcousticEnrollments`/`enrollAcousticReference`;
  `LiveChurchBrain.tsx`'s Music Intelligence panel gains a "Reference
  recordings for real audio fingerprinting" `<details>` section: a list
  of currently-enrolled songs, a song search box (reusing `searchMusic`),
  and a native file picker (`@tauri-apps/plugin-dialog`, filtered to
  `.wav`) that enrolls the selected file once a song is picked.

## Full regression result

`cargo fmt/clippy/check/test --workspace`: clean, both feature configs
(11 new tests in `cip-integrations-music-acoustic`, 258 -> 261 frontend
tests, 0 regressions anywhere else). Frontend `typecheck`/`lint`/`build`:
clean, same 5 pre-existing lint warnings as before this phase.

## Windows rebuild

This phase changes Rust code compiled into the desktop binary (like
Phase 7.1, unlike Phase 6.x) - see
`pilot-evidence/7.2/windows/installer-contents-verification.json` for
direct binary proof.

## Architectural safety diff

- Zero new Tauri commands beyond the two documented above, zero new
  events, zero new database schema.
- `core/music` (the domain contract crate) is untouched.
- `NullAcousticMusicRecognizer`/`ScriptedAcousticMusicRecognizer` and
  every existing acoustic-recognition code path are byte-identical to
  before - this phase is purely additive (a way to populate the manifest
  Phase 7.1 already read).
- `enroll_acoustic_reference` never mutates `AppState.acoustic_recognizer`
  - the currently-active recognizer is unaffected by any call to it until
  the next restart, exactly matching Whisper/embedding model install.

## Environment A / B / C

- **Environment A** (this container): PASSED - full regression green,
  including the new "a file that passes validation also enrolls
  successfully" test proving `validate_reference_wav` and `enroll_one`
  share one decode path.
- **Environment B**: unavailable, pre-existing container limitation.
- **Environment C**: NOT YET VERIFIED - the decisive pending gate is
  the operator's own real-hardware test: search for a real song already
  in a Music dataset, select it, pick a real WAV recording via the native
  file picker, confirm the enrollment appears in the list, restart CIP,
  confirm Diagnostics Mode's acoustic status reports `Available`, then
  play that song live and confirm it is recognized.

## Known limitations

- **Removing an enrollment is not supported** - an operator can fix a
  bad enrollment by re-enrolling the same song (upsert replaces it), but
  there is no dedicated "remove" command/button. A small, clean follow-up,
  not attempted here to keep this phase's diff focused.
- **No live re-recording from within the app** - enrollment imports an
  existing WAV file; it does not capture audio live via `cpal`. Matches
  every other model-provisioning flow in this codebase (all import an
  existing file).
- **Still no real recorded audio in this repository or container** -
  Phase 7.1's central limitation is unchanged by this phase; this phase
  only makes it easier for an operator to supply real audio themselves.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- A "remove enrollment" command/button.
- Real-hardware Environment C verification, now unblocked by this
  phase's enrollment UI.
- The four other master-architecture gaps `docs/phase-7-audit.md`
  identified, still open.

## Final gate

Environment A: **PASS**. Environment C: **PENDING**. This phase adds an
in-app enrollment workflow behind existing, already-shipped commands'
own conventions - it introduces no new backend surface beyond two
commands mirroring an established pattern, and changes no other
recognizer's or command's behavior.
