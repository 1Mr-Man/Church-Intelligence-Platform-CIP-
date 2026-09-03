# Phase 16: Live Transcript Show/Hide

## Trigger

Direct operator request, made while reviewing real Windows pilot screenshots of Phase 15's own
stall indicator and Bible-detection-to-display pipeline both working live: "Can it pause the live
transcript for now focus major on detecting scripture, music and the intelligences." The screenshots
themselves showed a long, noisy raw transcript (garbled fragments, frequent stall messages) crowding
the panel the operator scrolls past to reach Bible/Sermon/Service/Music detection further down.

## What "pause" means here (verified against the real architecture)

Bible, Sermon, Service, and Music detection are driven entirely by the backend speech worker thread
(`spawn_speech_worker`, `finalize_bible_only`, `route_segment_to_live_intelligence_engines`) - none
of it depends on whether the frontend renders the Live Transcript list. There is nothing to "pause"
in the pipeline sense without breaking Bible/Sermon/Service detection, all of which consume the
transcript text directly - and the operator's own request explicitly wants detection to keep
running ("focus major on detecting scripture, music and the intelligences"). The correct, safe
interpretation is a pure display toggle: hide the transcript list from view while every detection
engine keeps running completely unchanged.

## What was built

- **`apps/desktop/src/lib/transcriptPanel.ts`**: `isLiveTranscriptCollapsed` (pure, tested) - the
  storage-value semantics, mirroring `onboarding.ts`'s own established precedent for a
  `localStorage`-backed UI preference.
- **`apps/desktop/src/components/LiveChurchBrain.tsx`**: a Show/Hide button in the Live Transcript
  panel's own heading. Collapsing it replaces the transcript list (and the Phase 15 stall message,
  which would otherwise repeat under a hidden list) with a one-line note confirming detection keeps
  running. The preference persists across restarts via `localStorage`, wrapped in try/catch mirroring
  `OnboardingWalkthrough.tsx`'s own real-storage-access pattern exactly - a failure to persist just
  means it defaults back to expanded, never blocks anything.

## Explicitly deferred

No backend change of any kind - confirmed unnecessary, since the backend already runs Bible/Sermon/
Service/Music detection independently of the frontend's own rendering. No change to Bible/Sermon/
Service/Music panels themselves - this phase only touches the Live Transcript panel's own visibility.

## Testing boundary

`isLiveTranscriptCollapsed` is pure and fully unit-tested (4 new tests, mirroring
`onboarding.test.ts`'s own exact shape). The component wiring (the button, the `localStorage`
read/write) is thin, mirroring `OnboardingWalkthrough.tsx`'s own untested-directly precedent for
real `localStorage` access - this project has no DOM testing environment.

## Full regression result

- `npm run typecheck` / `npm run lint` (same 5 pre-existing warnings, unchanged) / `npm run test --
  run` (298 passed, up from 294 - the 4 new tests) / `npm run build`: all clean.
- No Rust code touched - `cargo fmt`/`clippy`/`test` are unaffected by this phase; not re-run beyond
  confirming `git status` shows no Rust files changed.

## Architectural safety

- Zero new Tauri commands, zero new events, zero backend changes of any kind.
- Bible/Sermon/Service/Music detection are verified architecturally incapable of being affected by
  this toggle - they are driven entirely by `spawn_speech_worker` on the backend, which has no
  dependency on frontend rendering state.
- Collapsing the panel never discards already-received transcript segments from component state -
  they are simply not rendered; expanding again shows the full list unchanged.

## Known limitations (honest, not deferred silently)

- This is a pure client-side preference (`localStorage`) - it is per-browser-profile/per-install,
  not synced anywhere, consistent with every other UI preference in this project (e.g. the
  onboarding-seen marker).
- This exact rebuilt artifact has not yet been installed or launched on real Windows hardware - see
  `physicalHardwareStatement` item 25 in the updated release manifest.

## Final gate

Environment A (typecheck/lint/test/build, direct binary symbol inspection): PASS. Environment C (a
real operator toggling Hide/Show during a real service and confirming Bible/Sermon/Service/Music
detection keep working unchanged while the transcript list is hidden): not yet performed - carried
forward into `physicalHardwareStatement`.
