# Phase 7.3 — Audit

## Baseline

Phase 7.2 shipped `enroll_acoustic_reference`/`list_acoustic_enrollments`
and an in-app enrollment UI. Its own "Known limitations" named the gap
directly: "Removing an enrollment is not supported - an operator can fix
a bad enrollment by re-enrolling the same songId (upsert replaces it),
but there is no dedicated 'remove' command or button." Filed as
deferred work specifically to keep Phase 7.2's diff focused.

## What's actually missing

An operator who enrolled the wrong song, a bad recording, or simply
wants to stop fingerprinting a song has no way to do so short of
manually deleting files inside the app's data directory - a
filesystem-level operation this project's own established discipline
(Phase 3.8.7.1, Phase 4.4, Phase 7.2 itself) treats as something a
command should do, not something an operator should be told to do by
hand.

## Design choice

No genuine fork: this is the direct completion of the CRUD lifecycle
Phase 7.2 started (list, add/replace already exist; remove is the only
gap). One real decision, resolved by precedent: whether to also delete
the enrolled WAV file from disk, not just the manifest entry. Leaving
the file behind would silently accumulate orphaned recordings in the
model directory forever - the same "don't leave stale state behind"
reasoning `write_manifest_entries` itself already documents for
replacing the whole manifest. File deletion is best-effort (a failure
to delete the file must not block removing the manifest entry, since
the entry is what actually matters for recognition - the file is
cleanup) and is treated the same way `enroll_one`'s per-file failure
isolation already works.

## What will be built

- `apps/desktop/src-tauri/src/commands.rs`: new
  `remove_acoustic_reference(song_id)` command - reads the current
  manifest, errors if no entry matches `song_id` (an honest "nothing to
  remove" rather than silently succeeding), writes back the filtered
  list, then best-effort deletes the entry's audio file from the model
  directory (logged, never fatal to the command). Mirrors
  `enroll_acoustic_reference`'s own read-modify-write shape exactly - no
  new crate-level function needed, since `read_manifest_entries`/
  `write_manifest_entries` (Phase 7.2) already provide everything this
  needs.
- Frontend: `commands.ts` gains `removeAcousticReference`;
  `LiveChurchBrain.tsx`'s enrolled-recordings list gains a "Remove"
  button per entry.

## Testing boundary

Matches this file's own established discipline (documented in
`commands.rs`'s test module header): command bodies that are thin
orchestration over already-tested pure/crate-level logic are not
separately unit-tested at the Tauri-command layer (see
`install_whisper_model`, `enroll_acoustic_reference` itself - neither
has a dedicated command-level test). `read_manifest_entries`/
`write_manifest_entries`'s round-trip and full-replace semantics are
already proven in `cip-integrations-music-acoustic`'s Phase 7.2 test
suite; this phase adds frontend command-wrapper tests only, matching
Phase 7.2's own precedent.
