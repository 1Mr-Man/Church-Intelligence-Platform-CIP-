# First Use — Operator Guide

This document explains how to launch and use the Church Intelligence
Platform (CIP) for a real service, written for a church operator, not a
developer. It assumes no knowledge of Rust, Tauri, SQLite, or CIP's
internal architecture. For the underlying engineering evidence behind
every claim here, see [`docs/phase-2-validation.md`](phase-2-validation.md)
and [`docs/phase-3-first-use.md`](phase-3-first-use.md).

## Quick Start

1. **Launch CIP.** Double-click the application. The first launch takes a
   few seconds longer than later ones (it sets up the local database and
   installs the Bible dataset); every launch after that is fast.
2. **Check readiness.** Look at the **Live Service** header at the top of
   the screen. It shows Service, Phase, Sermon, Speaker, Current song,
   **Bible**, Scripture, Audio/Speech, Acoustic, and Output at a glance.
   `Bible` should read `Berean Standard Bible — ENABLED (VERIFIED PUBLIC
   DOMAIN)`. If it instead says `NOT AVAILABLE`, see Troubleshooting below.
3. **Start service.** Use the **Service** panel's "Start Service" button.
4. **Enable microphone OR choose manual transcript.** In the **Audio &
   Speech** panel, either pick a microphone and click "Start Listening,"
   or open **Manual / test transcript entry** and type what is being said.
   Both paths feed the exact same detection and intelligence pipeline —
   nothing else in CIP behaves differently depending on which one you use.
5. **Review intelligence.** As Scripture, music, sermon points, and other
   findings are detected, they appear in the **Needs Attention** queue at
   the top of the workspace, ranked by confidence — never hidden because
   another domain has more items.
6. **Confirm Scripture.** Approve or reject each detected reference in the
   **Pending Suggestions** panel (or from the Attention Queue directly).
7. **Prepare presentation.** Once a reference is approved, click "Prepare"
   to stage it for display. It is not shown to the congregation yet.
8. **Display.** Use the **Current Output** panel's "Display" button to
   show it on the presentation display window. The header's `Output`
   field will read `ACTIVE — ON SCREEN`.
9. **Stop when finished.** Click "Stop" — the display returns to a blank
   state, and the header's `Output` field returns to `OPEN, NOTHING
   DISPLAYED`.
10. **End service.** Use the Service panel's "End Service" button when the
    service concludes.

Nothing in this workflow is ever automatic: CIP never displays anything
to the congregation, publishes content, or advances a workflow step
without an explicit click from the operator.

## System Requirements

- One desktop computer running Windows, Linux, or macOS (whichever build
  of CIP you were given).
- No internet connection is required for any core capability (Bible
  search, detection, all six intelligence domains, the workspace, or
  presentation display). CIP works fully offline.
- No account, login, or cloud service of any kind.
- A second monitor or projector output is recommended for the
  presentation display window, but CIP will run and can be tested on a
  single screen.

## First Launch

On the very first launch, CIP automatically:

- Creates its local database.
- Installs the complete Berean Standard Bible (BSB) — 66 books, 1,189
  chapters, 31,086 verses, verified public domain. This happens
  automatically, every time, on every computer CIP runs on; you do not
  need to import anything yourself.
- Checks for a local speech-recognition model (see "Speech" below) and an
  audio input device, without requiring either to be present.

If the Bible dataset ever fails to install (this would only happen if the
installation itself is damaged), CIP still launches — it will not crash —
and the header's `Bible` field will read `NOT AVAILABLE` instead of
silently pretending everything is fine. Restart CIP to retry; if the
problem persists, reinstall the application.

## Configuration

CIP needs no manual configuration to run its core workflow. Two optional
settings exist for advanced setups, both plain environment variables set
before launching CIP (ask whoever installed CIP for help with this if you
are not comfortable with environment variables):

| Setting | Purpose | Default |
|---|---|---|
| `CIP_WHISPER_MODEL_PATH` | Exact file path to a local speech-recognition model, if you have one | `<app data>/models/ggml-tiny.en.bin` |
| `CIP_ACOUSTIC_MODEL_DIR` | Directory for a local acoustic (audio-fingerprint) music model, if one is ever installed | `<app data>/models/acoustic` |

Nothing else needs to be configured. There is no settings file to hand-edit
and no database table to create by hand.

### Microphone

CIP lists every audio input device your computer reports. In the **Audio &
Speech** panel, choose one from the dropdown (or leave it on "Default
device") and click "Start Listening." If no microphone is available, the
panel says so and the manual transcript path remains fully usable.

### Speech recognition model

CIP can transcribe live audio to text using a local, offline
speech-recognition engine (Whisper) — but it does not come bundled with
one, and it never downloads one automatically (this is a deliberate
licensing and offline-operation decision, not an oversight). If no model
is configured, the **Audio & Speech** panel shows `SPEECH UNAVAILABLE`
along with the exact file path CIP expects a model at, and how to point
CIP at a different location via `CIP_WHISPER_MODEL_PATH` if you have one
stored elsewhere. **Manual transcript entry remains fully available
either way** — see the Quick Start above.

## Manual Transcript Fallback

If live speech is unavailable — no microphone, no model configured, or a
transient error — open **Manual / test transcript entry** (always visible
in the Audio & Speech panel, regardless of service state) and type what is
being said. It runs through the exact same detection, review, and
presentation pipeline as live speech. This is not a degraded "test mode";
it is a first-class, fully supported way to operate CIP, and it is the
right choice for a church that does not have a speech model configured.

## Service Workflow

- **Start / Pause / Resume / End** a service from the Service panel.
- CIP tracks the current service **phase** (Opening, Worship, Prayer,
  Scripture Reading, Sermon, Offering, Announcement, Closing) based on
  what is said, and shows it in the header. An unexpected transition (for
  example, the service appearing to move backward) is flagged as an
  anomaly for you to review — it never blocks the service or crashes.
- If the transcript goes quiet for a while, CIP marks it "stale" for your
  information only — it never ends the service automatically.
- You can always correct the detected phase manually.

## Sermon Workflow

- Start a sermon from the Sermon Foundation panel, optionally assign a
  title and speaker (speaker attribution is always something you type in
  — CIP never guesses who is speaking from audio).
- As the sermon proceeds, CIP surfaces detected main points, illustrations,
  applications, takeaways, and related Scripture in the Sermon
  Intelligence panel — every one of these is a **suggestion for your
  review**, never a theological claim CIP is asserting on its own.

## Reviewing Intelligence

Every detected item — a Scripture reference, a song, a sermon point, a
content idea, a cross-domain correlation — shows:

- **What domain** it came from (Bible, Music, Sermon, Service, Content,
  Cross-Domain).
- **Confidence** as a percentage.
- **Status** (e.g. Detected, Reviewed, Accepted, Rejected).
- **Evidence** — how many pieces of transcript/context support it.

Nothing is ever auto-accepted, auto-published, or auto-displayed. You
decide.

## Content Candidates and Saved Content

Content Intelligence suggests future-content ideas (a quote, a theme, a
discussion question) drawn from what has already been detected — never
final, polished copy, never a social post, never published automatically.
When you accept one, it moves into the **Saved Content** section (a
collapsible list inside the Content Intelligence panel) so you can copy
its text later — accepting a candidate is never a dead end.

## Scripture Presentation

1. **Prepare** — stages a reference for display. Not visible yet.
2. **Preview** — see exactly what will display, without displaying it.
3. **Display** — shows it on the presentation display window, an explicit
   click every time.
4. **Active** — the header confirms `ACTIVE — ON SCREEN` while it is
   showing.
5. **Stop** — returns the display to a blank state.

Only one item can be active at a time; CIP will not let you display a
second item while one is already showing. If the display window fails to
open for any reason, the item stays in "Prepared" state — it is never
falsely marked as displayed, and you will see a clear error message.

## Shutdown

Simply close CIP. If a presentation item was left active when CIP closes
unexpectedly, the next launch automatically resets it to "Stopped" before
anything else happens — you will never find a stale "on screen" item
haunting a fresh launch.

## Troubleshooting

| Symptom | Likely cause | What to do |
|---|---|---|
| Header shows `Bible: NOT AVAILABLE` | The one-time Bible dataset installation did not complete | Restart CIP. If it recurs, reinstall the application. |
| "SPEECH UNAVAILABLE" notice | No local speech model configured | Use manual transcript entry (fully supported), or configure `CIP_WHISPER_MODEL_PATH` to point at a real model file. |
| "NO_AUDIO_DEVICE" notice | No microphone detected | Connect a microphone, or use manual transcript entry. |
| Audio/Speech header switches to `ERROR` with a specific reason during a service | The microphone was physically disconnected (or otherwise failed) mid-capture | Reconnect the microphone and click "Start Listening" again, or switch to manual transcript entry — the service itself is never interrupted. |
| Manual transcript button is greyed out | No service is currently started | Start a service first — every intelligence action requires an active service. |
| "Presentation won't display" / error banner appears | The display window failed to open (rare — usually a display/driver issue) | The item remains safely "Prepared." Try Display again; if it keeps failing, check your monitor/projector connection and restart CIP. |
| Display window closed unexpectedly | Someone closed the window directly, or it crashed | CIP detects this and reconciles the state automatically — click Display again when ready. |
| Database won't initialize / app won't launch | A serious, rare local storage problem | Restart the computer and try again; contact whoever supports your installation if it persists. |
| Service phase seems "stuck" | Service Intelligence only updates as transcript arrives | Feed more transcript (live or manual), or correct the phase manually from the Service panel. |
| App was closed mid-service and reopened | Restart recovery | Service/sermon history is preserved; a live session does not resume automatically — start a new service if needed. |

## Known Limitations

- **Live speech recognition** requires you to source and configure your
  own local Whisper model — CIP does not include one. Manual transcript
  entry is the fully-supported alternative.
- **Acoustic (audio-fingerprint) music recognition** is not available in
  this build — no real recognition backend exists yet. Text/lyric-based
  music matching is fully functional and does not require this.
- **Physical projector/monitor hardware** has been verified only under a
  virtual display in the development environment that built this
  release — verify your own display hardware works as expected before
  relying on it for a live service.
- **OBS, vMix, and NDI output** are not implemented.
- CIP has one supported display surface: its own local presentation
  window. It is not a multi-monitor production switcher.

None of these limit CIP's core, offline, operator-reviewed workflow —
detecting Scripture, reviewing intelligence across six domains, and
displaying confirmed content — described in the Quick Start above.
