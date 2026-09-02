# Phase 7.3 — Real Audio Fingerprinting: Remove Enrollment

## Baseline

Phase 7.2 shipped `enroll_acoustic_reference`/`list_acoustic_enrollments`
and an in-app enrollment UI. Its own "Known limitations" named the gap
directly: "Removing an enrollment is not supported - an operator can fix
a bad enrollment by re-enrolling the same songId (upsert replaces it),
but there is no dedicated 'remove' command or button." See
`docs/phase-7-3-audit.md`.

## Audit finding

An operator who enrolled the wrong song, a bad recording, or simply
wants to stop fingerprinting a song had no way to do so short of
manually deleting files inside the app's data directory - a
filesystem-level operation this project's own established discipline
(Phase 3.8.7.1, Phase 4.4, Phase 7.2 itself) treats as something a
command should do, not something an operator should be told to do by
hand.

## Design choice

No genuine fork: this is the direct completion of the CRUD lifecycle
Phase 7.2 started (list, add/replace already existed; remove was the
only gap). One real decision, resolved by precedent: whether to also
delete the enrolled WAV file from disk, not just the manifest entry.
Leaving the file behind would silently accumulate orphaned recordings in
the model directory forever - the same "don't leave stale state behind"
reasoning `write_manifest_entries` itself already documents for
replacing the whole manifest. File deletion is best-effort (a failure to
delete the file must not block removing the manifest entry, since the
entry is what actually matters for recognition - the file is cleanup)
and is treated the same way `enroll_one`'s per-file failure isolation
already works.

## What was built

- **`apps/desktop/src-tauri/src/commands.rs`**: new
  `remove_acoustic_reference(song_id)` command - reads the current
  manifest via `read_manifest_entries`, errors with an honest "nothing
  to remove" `AppError` if no entry matches `song_id` (never silently
  succeeds), writes back the filtered list via `write_manifest_entries`,
  then best-effort deletes the entry's audio file from the model
  directory (a delete failure is logged as a warning, never surfaced as
  a command error). Mirrors `enroll_acoustic_reference`'s own
  read-modify-write shape exactly - no new crate-level function needed,
  since `read_manifest_entries`/`write_manifest_entries` (Phase 7.2)
  already provide everything this needs.
- **`apps/desktop/src-tauri/src/lib.rs`**: registered
  `commands::remove_acoustic_reference` in the invoke handler.
- **`apps/desktop/src/lib/commands.ts`**: new
  `removeAcousticReference(songId: string): Promise<void>` wrapper.
- **`apps/desktop/src/components/LiveChurchBrain.tsx`**: the enrolled-
  recordings list gains a "Remove" button per entry, calling
  `removeAcousticReference` and then refreshing the enrollment list;
  restructured to use the existing `.live-brain__suggestion-header`
  flex-row pattern already used elsewhere in the same panel, for visual
  consistency with the rest of the Music Intelligence panel.

## Testing boundary

Matches this codebase's own established discipline (documented in
`commands.rs`'s test module header, and reaffirmed in
`docs/phase-7-3-audit.md`): command bodies that are thin orchestration
over already-tested pure/crate-level logic are not separately
unit-tested at the Tauri-command layer (`install_whisper_model` and
`enroll_acoustic_reference` follow the same rule - neither has a
dedicated command-level test). `read_manifest_entries`/
`write_manifest_entries`'s round-trip and full-replace semantics are
already proven in `cip-integrations-music-acoustic`'s Phase 7.2 test
suite (unchanged, still 48 tests); this phase adds frontend
command-wrapper tests only, matching Phase 7.2's own precedent.

## Full regression result

`cargo fmt/clippy/check/test --workspace`: clean, both feature configs
(0 new Rust tests - `cip-integrations-music-acoustic` stays at 48 tests,
per the audit doc's explicit testing-boundary decision; 0 regressions
anywhere else). Frontend `typecheck`/`lint`/`test`/`build`: clean, 261 ->
262 tests (1 new: `removeAcousticReference` forwarding, folded into the
existing outside-Tauri-rejection test), same 5 pre-existing lint
warnings as before this phase.

## Windows rebuild

This phase changes Rust code compiled into the desktop binary (like
Phase 7.1/7.2) - see
`pilot-evidence/7.3/windows/installer-contents-verification.json` for
direct binary proof.

## Architectural safety diff

- Exactly one new Tauri command (`remove_acoustic_reference`), zero new
  events, zero new database schema, zero new crate-level functions (it
  composes Phase 7.2's `read_manifest_entries`/`write_manifest_entries`
  as-is).
- `core/music` (the domain contract crate) is untouched.
- `list_acoustic_enrollments`/`enroll_acoustic_reference` and every
  existing acoustic-recognition code path are byte-identical to before -
  this phase is purely additive.
- `remove_acoustic_reference` never mutates `AppState.acoustic_recognizer`
  - the currently-active recognizer is unaffected by any call to it
  until the next restart, exactly matching enrollment's own restart-
  required discipline.
- File deletion is best-effort and strictly ordered after the manifest
  write succeeds: an entry can never be "half removed" (present in the
  manifest but file gone), and a file-delete failure never leaves the
  manifest entry behind.

## Environment A / B / C

- **Environment A** (this container): PASSED - full regression green.
- **Environment B**: unavailable, pre-existing container limitation.
- **Environment C**: NOT YET VERIFIED - the decisive pending gate is
  the operator's own real-hardware test: with at least one enrollment
  present, click Remove on it, confirm it disappears from the list and
  the underlying WAV file is gone from the model directory, restart CIP,
  and confirm that song is no longer recognized.

## Known limitations

- **No confirmation prompt before removing** - clicking Remove acts
  immediately, matching every other operator-decision button in this
  panel (Accept/Reject on findings behave the same way).
- **Still no real recorded audio in this repository or container** -
  Phase 7.1's central limitation is unchanged by this phase.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- Real-hardware Environment C verification, covering the full
  enroll/list/remove lifecycle end to end.
- The four other master-architecture gaps `docs/phase-7-audit.md`
  identified, still open: internet/hybrid intelligence, multi-language
  support, church/user roles & permissions, OBS/vMix/livestream
  integration.

## Final gate

Environment A: **PASS**. Environment C: **PENDING**. This phase
completes the enrollment CRUD lifecycle (list, add/replace, remove)
behind one new command mirroring an established pattern - it introduces
no new backend surface beyond that command, and changes no other
recognizer's or command's behavior.
