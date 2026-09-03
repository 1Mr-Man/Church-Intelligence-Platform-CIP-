# Phase 18 Audit: "No sound was detected" (Stereo Mix loopback capture)

## Trigger

Two real Windows pilot screenshots (build `0c25bd20e743`, the Phase 17 artifact) plus "No
sound was detected": the operator selected `Stereo Mix (Realtek(R) Audio)` as the audio input
device (a loopback/monitor device that mirrors whatever the computer is playing, not a
microphone) while a YouTube church-service video played, clicked Start Listening, and the
Audio & Speech panel showed `NO SIGNAL — audio device is capturing but no sound is being
detected` throughout. System Diagnostics confirmed: Audio chunks received: 22669 (a live,
continuously-running capture stream, not a stalled one), Inferences: 0 succeeded / 0 attempted
(75 windows skipped - classified as silence), Speech pipeline health: Normal.

## What was audited

Traced the full capture path end to end to rule out a CIP code defect before concluding
anything about the operator's own Windows configuration:

1. **Device selection** (`integrations/audio/src/lib.rs::find_device`) - matches the exact
   device name the operator selected (`Stereo Mix (Realtek(R) Audio)`) against
   `host.input_devices()`. Confirmed correct: not silently falling back to the system's global
   default device.
2. **Stream negotiation** (`run_worker`'s `WorkerCommand::Start` handler) - calls
   `device.default_input_config()` on that *specific* `Device` instance (Stereo Mix's own
   default format), not a global default. Confirmed correct.
3. **Downmix** (`downmix_to_i16`) - averages every interleaved channel per frame
   (`data.chunks(channels)`), using cpal's own negotiated `channels` count, not a hardcoded
   mono assumption. Confirmed correct for a 2-channel (or any-N-channel) Stereo Mix stream.
4. **RMS/level computation** (`rms_level`) - a standard root-mean-square over the downmixed
   i16 samples, clamped to `0.0..=1.0`. Confirmed correct.
5. **Resampling** (`resample_pcm16`, Phase 3.8.6) - the diagnostics screenshot's own
   "480 samples @ 48000 Hz, resampled to 160 samples" line is exactly the correct 48000→16000
   Hz ratio (3:1), confirming this stage is working as designed.
6. **Whisper's own independent silence gate** (`ai/speech/src/whisper.rs::is_silence`,
   `SILENCE_RMS_THRESHOLD = 0.01`) - a *second*, independently-computed RMS over the resampled
   16kHz buffer, in a different crate, also classified every window as silent (75/75 windows
   skipped).

Two independently-implemented RMS computations, on two different buffers, in two different
crates, agree the actual captured samples are silent. This is strong evidence against a
computation bug in either path (a bug would have to coincidentally reproduce in both
independent implementations) and strong evidence *for* the conclusion that genuinely
near-silent PCM data is reaching the application - i.e., real audio is not actually flowing
through the Stereo Mix loopback at the OS level, not a CIP defect misreporting audio that is
present.

## Conclusion

No CIP code defect was found in the capture/downmix/resample/silence-detection path for this
report. The most likely real-world cause, consistent with this evidence, is one of the
well-documented Windows Stereo Mix quirks outside this application's control:

- Stereo Mix's own Windows recording level (Sound Settings > Recording > Stereo Mix >
  Properties > Levels) is muted or at 0% - a common out-of-the-box state on many OEM
  installs, unrelated to whether the device is selected as CIP's input.
- The audio actually being played (the YouTube tab) is not routed through the same device
  Windows treats as the default *playback* output that Stereo Mix mirrors (e.g., a different
  output device, or a virtual audio device such as the "Iriun Webcam" software visible enabled
  in the operator's own Sound settings screenshot, could be involved).
- The system or per-app (browser) volume was low enough that Stereo Mix's own mirrored signal
  fell under the classification floor.

None of these are things a cross-platform Rust audio library can detect or correct - cpal has
no API to read a Windows recording-device's own level/mute state.

## What was built instead: a loopback-aware diagnostic message

The existing `NO SIGNAL`/`LOW SIGNAL` message (`apps/desktop/src/lib/format.ts::describeAudioSignal`,
Phase 14) was written assuming a physical microphone: "move the microphone closer or raise its
gain." For a loopback device like Stereo Mix, that advice is not just unhelpful, it's actively
wrong - there is no physical microphone to move and no per-device gain slider in CIP itself.
This is a real, addressable gap this operator's own report exposed.

`describeAudioSignal` now takes an optional `deviceName` parameter (the backend's own resolved
`AudioEngineStatus.selectedDevice`, accurate even when the operator left CIP's device picker on
"Default device"). When the device name matches a known Windows loopback/monitor pattern
(`stereo mix`, `wave out mix`, `what u hear`, `loopback`, `monitor of` - case-insensitive), both
the `NO SIGNAL` and `LOW SIGNAL` messages are replaced with loopback-specific guidance pointing
at the device's own Windows recording level and the default playback-output routing, instead of
microphone-gain advice that cannot apply. Every existing caller/test defaults `deviceName` to
`null` and keeps the original physical-microphone wording unchanged.

## Explicitly deferred

- No attempt to auto-detect or auto-fix the underlying Windows audio routing - no
  cross-platform API exists for this, and guessing at "the real fix" without being able to test
  on real Windows hardware would risk shipping something untested and possibly wrong.
- No change to `SILENCE_RMS_THRESHOLD` or any detection threshold - the evidence points at zero
  signal reaching the app, not a threshold miscalibration; loosening a threshold would not fix
  genuinely silent input.
- No new Rust code, no schema change - this is a pure frontend message-classification fix.

## Testing boundary

`isLoopbackDeviceName`/`describeAudioSignal`'s loopback branch are pure and fully unit-tested
(5 new tests in `format.test.ts`: loopback `NO SIGNAL` guidance never mentions "microphone",
loopback `LOW SIGNAL` guidance mentions "volume" not a microphone gain adjustment, case-
insensitive/alias detection, and two tests confirming a normal microphone device and a `null`
device name both keep the exact original wording unchanged).

## Full regression result

Frontend only (no Rust touched): `npm run typecheck` 0 errors, `npm run lint` same 5
pre-existing warnings (unchanged), `npm run test -- --run` 303/303 (up from 298, the 5 new
tests), `npm run build` clean.

## Architectural safety

- Zero new Tauri commands, zero new events, zero backend changes of any kind.
- `describeAudioSignal`'s new parameter is optional and defaults to `null` - every existing
  call site keeps its exact prior behavior unless it now also passes a device name.
- Bible/Sermon/Service/Music detection are entirely unaffected - this only changes a status
  message string in the Live Service UI.

## Known limitations (honest, not deferred silently)

- This does not and cannot fix the underlying Windows audio routing - it only tells the
  operator where to look. Whether Stereo Mix ever actually captures audio on this specific
  machine remains outside CIP's control and untested by this phase.
- The loopback device-name pattern list is a best-effort match against known real Windows
  loopback device names, not exhaustive - an unusual or renamed loopback device would still get
  the generic microphone-oriented message.
- This exact rebuilt artifact has not yet been installed or launched on real Windows hardware -
  see `physicalHardwareStatement` item 27 in the updated release manifest.

## Final gate

Environment A (typecheck/lint/test/build): PASS. Environment C (the operator checking Stereo
Mix's own Windows recording level per this new guidance, and/or re-testing with a physical
microphone to confirm CIP's detection pipeline itself works once real audio actually reaches
it): not yet performed - carried forward into `physicalHardwareStatement`.
