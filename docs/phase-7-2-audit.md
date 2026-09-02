# Phase 7.2 — Audit

## Baseline

Phase 7.1 replaced `LocalAcousticMusicRecognizer`'s honest scaffold with a
real, offline spectral landmark hashing recognizer. Its own "Known
limitations"/"Deferred work" sections named the most obvious next gap:
"No enrollment UI - creating `acoustic-model.json` and gathering
reference WAV files is a manual, file-system-level operator task."

## What's actually missing

An operator today, to enroll one song, would need to: hand-write JSON
matching `local.rs`'s private `Manifest`/`ManifestSong` schema, know the
exact `MODEL_MANIFEST_FILENAME`/`ACOUSTIC_MODEL_DIR_NAME` path
convention, and manually place a WAV file at a path they reference
correctly - a task realistically only the person who built this feature
could do without documentation open in another window. This is the same
gap Phase 3.8.7.1 closed for the Whisper model (native file picker +
`install_whisper_model`) and Phase 4.4 closed for the embedding model
(`install_embedding_model_file`/`install_embedding_tokenizer_file`) -
both established, working precedents this phase reuses rather than
inventing a new pattern.

## Design choice

No genuine fork: the precedent is already set twice (Whisper, embedding
model) and this phase's job is to extend it to a case with N entries
instead of 1 fixed path. Two real decisions, both resolved by following
existing precedent rather than guessing:

1. **Live reconfigure vs. restart-required.** `install_whisper_model`
   and `install_embedding_model_file` both explicitly log "restart CIP
   for it to take effect" - `AppState`'s recognizers/engines are built
   once at startup (`create_acoustic_recognizer`, called from `run()`)
   and never hot-swapped. This phase follows the exact same discipline:
   enrollment writes files and updates the manifest; the in-memory
   `FingerprintIndex` picks it up on the next restart, exactly like a
   newly-installed Whisper model does. Introducing live hot-swap here
   alone, with no other model in this codebase doing so, would be a new
   pattern this phase has no mandate to invent.
2. **How an operator identifies which song a recording belongs to.**
   `search_music` (Phase 2.1) already lets an operator find a song by
   title/number/lyric text across enabled datasets and returns
   `songId`/`source` (content id) - reused as-is for the picker, no new
   search logic.

## What will be built

- `integrations/music-acoustic/src/local.rs`: `Manifest`/`ManifestSong`
  made `pub` (with `Serialize` added alongside their existing
  `Deserialize`, so they round-trip); `enroll_one`'s WAV-validation logic
  extracted into a new `pub fn validate_reference_wav` both enrollment
  and the new command call, so a candidate file is checked with the
  exact same rule either way (no second, slightly-different validator to
  drift out of sync); new `pub fn read_manifest_entries`/
  `write_manifest_entries` helpers for whole-manifest read/upsert/save.
- `apps/desktop/src-tauri/src/commands.rs`: `list_acoustic_enrollments`
  (read-only, returns the manifest's current entries) and
  `enroll_acoustic_reference(song_id, content_id, source_path)` (validate
  -> copy into the acoustic model dir -> upsert the manifest entry -> log
  "restart to take effect"), mirroring `install_whisper_model`'s shape.
- Frontend: `domain/music.ts` gains an `AcousticEnrollment` mirror;
  `commands.ts` gains the two wrappers; `LiveChurchBrain.tsx`'s Music
  Intelligence panel gains a collapsible "Reference recordings" section
  (mirroring the panel's existing "Manual / test music transcript entry"
  `<details>` pattern immediately below it) with a song search box, a
  native file picker (reusing `@tauri-apps/plugin-dialog`'s `open()`,
  filtered to `.wav`), and a list of currently-enrolled songs.

## Deliberately out of scope

- **Removing an enrollment.** Genuinely useful, but not blocking - an
  operator can already fix a bad enrollment by re-enrolling the same
  `songId` (upsert replaces it). A dedicated remove command is a small,
  clean follow-up, not attempted here to keep this phase's diff focused.
- **Recording audio live from within the app** (as opposed to importing
  an existing file) - a materially bigger feature (reusing `cpal`
  capture, a record/stop UI, trimming) that the master-plan gap list
  never asked for; importing an existing file is what every other model
  provisioning flow in this codebase already does.
- **Live hot-swap of the recognizer** - see design choice 1 above.
