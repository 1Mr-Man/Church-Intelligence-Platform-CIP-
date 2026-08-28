# Phase 3.5 — Professional Church Operator UX & First-Use Interface Redesign

## 1. Baseline Audit

**Started at** `fe081c8` (Phase 3.4's Windows-installer-packaging commit),
tree clean. Confirmed via `git status`, `git branch --show-current`,
`git log -1` before any code was touched. HEAD matched the expected
Phase 3.4 baseline - no divergence from a prior report was found or
assumed.

Read the relevant history (Phase 2.9's Unified Operator Workspace,
Phase 2.10's validation record, Phase 3.0's first-use hardening,
Phase 3.1-3.4's hardware-pilot and Windows-packaging work) and, per this
phase's own instruction, treated the **repository as authoritative over
any prior report's description**. The repository confirmed the prior
phases' own self-descriptions were accurate as far as they went - Phase
2.9 genuinely did build a real unified attention queue/feed - but none of
them had addressed the surrounding visual hierarchy, which is what this
phase's own real-code audit (section 2) found.

## 2. UX Audit (what was wrong)

No screenshot or video was attached to this session; a real Windows
machine had reportedly been used to inspect the interface previously, but
that evidence was not available to this container. Per this phase's own
"do not ignore evidence, do not fabricate physical validation" rule, this
audit is built entirely from **reading the actual React source and CSS**
(`LiveChurchBrain.tsx`, `LiveChurchBrain.css`, `workspace.css`,
`index.css`) - the repository is authoritative, and every claim below is
a specific line/class, not an impression.

**A. What an operator sees on first launch.** A generic-looking single
720px-wide centered column (`'.live-brain { max-width: 720px; }'`) with a
56px marketing-site-style `<h1>` ("CIP — Live Service", inherited from
`index.css`'s landing-page heading styles) above a row of raw technical
badges (`Runtime: tauri`, `Network: online`, `AI: available`,
`Database: connected`) and then a small, plain "Service" panel with a
title input and a Start button - visually identical in weight to every
other panel on the page.

**B. What is visually dominant.** Nothing. Every one of the 20
top-level `<section className="live-brain__panel">` blocks used the exact
same 1px `#ddd` border, the same 8px radius, the same uppercase 0.95rem
grey `<h2>`. Presentation - Section 41's explicit design target and
Section 11's "one of CIP's most important church-facing capabilities" -
had no more visual weight than "Manual Bible Search" or a diagnostic
"Intelligence Status" panel.

**C. What is confusing.** The Approve → Prepare → Display pipeline was
split across three separate, non-adjacent panels: "Pending Suggestions"
(top), "Approved — Ready to Prepare" (middle), "Current Output" (further
down, past Manual Bible Search). An operator moving Scripture toward the
screen had to scroll between three unrelated-looking boxes to complete
one workflow.

**D. What requires unnecessary interpretation.** Raw backend vocabulary
reached the screen unfiltered: `Runtime: tauri`, `NO_AUDIO_DEVICE`,
`IntelligenceFinding`-shaped confidence percentages with no plain-language
gloss, `AudioStatusKind`/`SpeechStatusKind` values displayed via
`.toUpperCase()` with no translation layer.

**E. Information useful during a live service.** Live transcript, current
Scripture/context, the attention queue, the presentation pipeline,
compact audio/speech/Bible/display status.

**F. Information that belongs in diagnostics.** Per-domain raw
finding/review panels that already duplicate the Phase 2.9 unified
feed's review actions (Music/Sermon/Cross-Domain/Content/Service
Intelligence panels, and the raw "Pending Suggestions" list with its
extra edit-in-place tool); manual search/analyze power tools (Manual
Bible Search, the per-domain "Run analysis" buttons); Content Registry
and Intelligence Status `<details>` blocks (already self-labeled
"Diagnostics:" before this phase); the raw technical `StatusBar`;
`PilotDiagnosticsPanel`; Service History.

**G. Controls too small/hidden.** The Start/Display/Prepare buttons used
the same default browser button styling as every other button on the
page - no visual weight distinguishing "the button that puts something in
front of the congregation" from "the button that refreshes a diagnostic
list."

**H. Ambiguous states.** "Approved" (a `Suggestion` status) and "Prepared"
(a `PresentationItem` status) and "Active/On Screen" all rendered as
similar small green/grey text labels with no shared, deliberate visual
language - exactly the "Prepared vs. Displayed confusion" this phase's
spec names as a hard safety risk.

**I. Too many clicks / too much scrolling.** Reaching "Current Output"
required scrolling past Service, Audio & Speech, Current Scripture,
Ambiguous Scripture (when present), Live Transcript, Pending Suggestions,
Approved, Service Timeline, and Manual Bible Search - nine sections
before the presentation controls.

**J/K/L. Promote / collapse / remove from Operator Mode.** Directly
produced the redesign in section 4 below: Service state, transcript,
attention queue, and presentation get promoted to the top and given
distinct visual weight; per-domain raw panels, manual search tools, and
technical diagnostics move to a separate Diagnostics Mode; nothing was
deleted.

## 3. Existing-Contract Inventory (spec section C/22/24/25)

Before writing any new component, the following were read in full and
confirmed unchanged by this phase:

- **Tauri commands** (`apps/desktop/src/lib/commands.ts`): every command
  this phase's new components call
  (`startService`/`pauseService`/`resumeService`/`endService`,
  `previewPresentation`/`preparePresentation`,
  `openPresentationDisplay`/`closePresentationDisplay`,
  `displayPresentation`/`cancelPresentation`/`clearPresentationDisplay`,
  `listAudioDevices`, `getLiveStatus`, `getAppConfig`) already existed
  and is called with an unchanged signature. **Zero new Tauri commands
  were added this phase.**
- **Events**: no event name in `events/eventNames.ts` was touched;
  `liveEvents.ts` subscriptions are unchanged.
- **`UnifiedIntelligenceItem`/`buildUnifiedFeed`/`buildAttentionQueue`**
  (`lib/unifiedFeed.ts`, `lib/attentionQueue.ts`): read in full,
  confirmed to already implement exactly the "one unified,
  confidence-ranked queue across all six domains" the spec asks for
  (Phase 2.9's own canonical acceptance test,
  `lib/operatorWorkflow.test.ts`, still passes unmodified). **Reused
  as-is** - `AttentionQueue`/`IntelligenceFeed` are unchanged this phase.
- **`actionsFor`/`ACTION_LABELS`** (`workspace/actions.ts`): read,
  confirmed to mirror the exact same commands the raw per-domain panels
  call. Unchanged.
- **Domain types** (`domain/*.ts`): read `LiveStatus`, `Suggestion`,
  `PresentationItem`/`PresentationContent`, `AudioDevice`,
  `ContentMetadata` in full before writing any new component, to avoid
  guessing field shapes. Zero domain types were changed.

## 4. Architecture (what changed)

**No backend change.** `git diff --stat` for this phase touches only
`apps/desktop/src/**` - no file under `apps/desktop/src-tauri`, `core/`,
`database/`, `integrations/`, or `ai/` was modified.

**Frontend**: `LiveChurchBrain.tsx` (the single ~2400-line operator
workspace component) gained one new piece of state - `mode: "operator" |
"diagnostics"` - and its render tree was restructured around that mode,
with three new presentational components extracted from code that
already existed inline:

- **`ServiceControlBar.tsx`** (new) - the Priority-1 service state
  control, replacing the old plain "Service" panel. Same
  `startService`/`pauseService`/`resumeService`/`endService` commands.
- **`SystemStatusStrip.tsx`** (new) - the compact, human-language
  Microphone/Speech/Bible/Display row (Priority 6), reading only
  `LiveStatus`/`AudioDevice[]`/`displayWindowOpen` that were already
  fetched.
- **`PresentationCard.tsx`** (new) - merges what were previously two
  separate, non-adjacent sections ("Approved — Ready to Prepare" and
  "Current Output") into one visually dominant card implementing the
  PREPARED / ● ON SCREEN / STOPPED language from spec section 11. Calls
  exactly the six presentation commands those two old sections already
  called - `previewPresentation`, `preparePresentation`,
  `openPresentationDisplay`, `closePresentationDisplay`,
  `displayPresentation`, `cancelPresentation`, `clearPresentationDisplay`.

`AttentionQueue`, `IntelligenceFeed`, `IntelligenceCard`,
`WorkspaceHeader`, and `PilotDiagnosticsPanel` (all Phase 2.9/3.3/3.4
work) are **unchanged** - reused exactly as they were, just repositioned
in the render tree and, for `PilotDiagnosticsPanel`/the raw `StatusBar`,
moved behind the Diagnostics-mode gate.

Visual reordering within Operator Mode uses CSS `order` on
`.live-brain`'s flex children (`.op-order-*` classes in
`LiveChurchBrain.css`) rather than physically relocating the ~1500 lines
of existing, working JSX for Audio & Speech / Current Scripture /
Ambiguous Scripture / Live Transcript - a deliberate risk-reduction
choice for a single, very large component: CSS reordering cannot silently
break a command call or an event handler the way a large cut-and-paste
text move could.

**Two old sections were deleted outright** (not duplicated, not merely
hidden): the pre-3.5 "Approved — Ready to Prepare" and "Current Output"
panels, whose entire functionality now lives in `PresentationCard`. The
`ApprovedSuggestionCard` helper component, now unreferenced, was removed
(caught by `tsc`'s unused-declaration check, not silently left as dead
code).

## 5. Operator Workflow

In Operator Mode (the default on every launch), the page now reads, top
to bottom: **mode toggle** → **Service state** (hero "Ready to Start" or
a compact Live/Paused bar) → **at-a-glance state** (`WorkspaceHeader`) →
**compact system status** (Mic/Speech/Bible/Display) → **Needs
Attention** (the unified queue - Approve/Reject/Accept/Acknowledge/
Review/Dismiss, unchanged from Phase 2.9) → **Recent Intelligence**
(the chronological feed) → **Presentation** (Approve → Prepare → Display
→ Stop, one card) → **Audio & Speech** controls → **Current Scripture** /
**Ambiguous Scripture** (when present) → **Live Transcript**. Every
technical/per-domain/manual-search panel is absent from this view -
switching to Diagnostics Mode reveals them unchanged.

## 6. First Use

Before a service starts, `ServiceControlBar` renders the spec section 6
"READY TO START" hero: a real heading, one sentence of plain-language
orientation, a title field, a large primary "Start Service" button, and a
compact readiness row (Bible / Microphone / Speech, using the shared
good/warn/bad vocabulary - "Optional" for speech when no Whisper model is
configured, never a scary broken-looking red). No source-code editing,
no SQL, no developer tooling is implied or required anywhere in this
view. This phase did not build the full multi-step guided setup wizard
spec section 7 sketches (Welcome → Bible → Microphone → Speech → Display
→ Test → Ready) - the existing `first-use.md` document plus this
redesigned hero already give an operator every fact they need before
starting, and building a second, separate wizard flow around data that's
already visible here would have been exactly the kind of "endless
redesign" spec section 50 warns against for a UX milestone whose real
gap was hierarchy, not a missing wizard.

## 7. Presentation

`PresentationCard` never uses the word "ready" for two different things:
an approved-but-not-yet-prepared suggestion shows "Approved" with
Preview/Prepare actions; a prepared-but-not-displayed item shows "Ready
to Present" with a primary Display action; the one item actually on the
projector shows "● On Screen" in the good-status color with only a Stop
action. `Prepared` and `Active` are always visually and textually
distinct - the explicit-activation safety model (no automatic
presentation, no automatic publishing) is unchanged; every state
transition here still requires the same explicit operator click it did
before this phase.

## 8. Audio

`SystemStatusStrip` translates `AudioStatusKind`/`SpeechStatusKind`
directly into the shared vocabulary (Ready / Listening / Error / Not
configured / Optional — not configured) - never a raw enum string. The
existing Audio & Speech card's error notices
("SPEECH UNAVAILABLE...", "NO_AUDIO_DEVICE...") were left as previously
written (Phase 3.0 already gave them a concrete next step - the exact
Whisper model path, a Retry button); this phase did not find a genuine
regression in that copy worth touching, per the "don't overdesign"
instruction.

## 9. Intelligence

All six domains (Bible, Music, Sermon, Service, Content, Cross-Domain)
still surface through exactly one queue and one feed -
`buildAttentionQueue`/`buildUnifiedFeed`, unmodified. No second feed, no
second queue, no second intelligence context, no new confidence or
correlation logic was introduced. The raw per-domain panels (Music
Intelligence, Sermon Foundation, Sermon Intelligence, Cross-Domain
Intelligence, Content Intelligence, Service Intelligence, and the raw
"Pending Suggestions" list) still exist, byte-for-byte, in Diagnostics
Mode - available for the rare case (a typo correction via the "Edit"
action) the unified feed's uniform approve/reject doesn't cover, and for
manual search/analyze power-user tools.

## 10. Diagnostics

Diagnostics Mode contains, unchanged: `PilotDiagnosticsPanel` (Phase
3.3/3.4 - machine/build/database/Whisper/display detail), the raw
`StatusBar` (Runtime/Network/AI/Database technical badges), "Pending
Suggestions" (with its Edit tool), "Service Timeline", "Manual Bible
Search", "Diagnostics: Content Registry", "Diagnostics: Intelligence
Status", "Music Intelligence", "Sermon Foundation", "Sermon
Intelligence", "Cross-Domain Intelligence", "Content Intelligence",
"Service Intelligence", and "Service History". Nothing here was deleted
or weakened - only relocated behind the mode toggle.

## 11. Error UX

Existing error notices (speech unavailable, audio error, no device) were
reviewed against the "what happened / why / what can I do" pattern and
found to already follow it (Phase 3.0's own hardening); this phase
carried them forward unchanged rather than rewriting working copy for
its own sake.

## 12. Accessibility

The mode toggle uses `role="group"`/`aria-label="View mode"` with
`aria-pressed` on each button (a real toggle-button pattern, not a
color-only signal). Every status indicator in `SystemStatusStrip`/
`ServiceControlBar`/`PresentationCard` pairs a colored dot with a text
label - color is never the only signal, consistent with the existing
`.live-brain__badge` convention this phase extended rather than replaced.
No nested interactive elements were introduced. A full screen-reader
pass on the actual Windows machine was not performed (see section 14).

## 13. Human Usability

**Not performed.** No physical Windows machine and no human operator were
available to this container - see section 14/17.

## 14. Windows Validation

**Not performed on physical hardware.** This container has no Windows
machine. The Windows NSIS installer was rebuilt from this phase's exact
source (`pilot-evidence` / `release/windows/` - see section 17) using the
same cross-compilation path Phase 3.4 established and verified working;
that rebuild is Environment A/B evidence (a successful cross-compile),
never Environment C.

## 15. Physical Display Validation

**HOLD - NOT AVAILABLE.** No second display/projector is connected to
this container.

## 16. Physical Microphone Validation

**HOLD - NOT AVAILABLE.** No microphone exists in this container
(`/dev/snd` absent, re-confirmed this phase).

## 17. What Is Proven

- `PROVEN_AUTOMATED`: 768 default-feature Rust tests, 7 + 204
  whisper-feature Rust tests, 179 frontend tests - all green, including
  Phase 2.9's canonical `operatorWorkflow.test.ts` acceptance test,
  unmodified and still passing against the unchanged `unifiedFeed`/
  `attentionQueue` modules this redesign reuses. `cargo fmt`/`clippy`/
  `tsc`/`vite build`/`oxlint` all clean, zero new warnings.
- `PROVEN_XVFB`: a fresh Xvfb relaunch of the rebuilt Linux binary
  (containing this phase's redesigned frontend) starts cleanly, migrates
  0 times on a second launch, and re-confirms the BSB dataset intact -
  see `pilot-evidence/3.5/software/`. This proves desktop/runtime
  correctness for the rebuilt bundle, never human usability.

## 18. What Is Not Proven / Not Available

- `NOT_AVAILABLE`: real Windows launch, real human usability testing,
  real microphone, real Whisper transcription, real second
  display/projector, real 60-90 minute service, real operator task
  timing, real installer walkthrough on Windows, real crash/recovery on
  Windows, real multi-hour stability on the target machine.
- These are the same items every prior hardware-pilot phase (3.1-3.4)
  recorded as blocked in this container, for the same reason: no physical
  Windows machine, microphone, or projector has ever been present here.

## 19. Release Gate

See the mandatory gate block at the end of this document and the final
chat report. **HOLD** for every physical-hardware/human item; **PASS**
for every automated/code-level item; the redesigned Operator Mode and
first-use hero are functional and regression-tested, but that is
`PROVEN_AUTOMATED`/`PROVEN_XVFB` evidence, never `VERIFIED_WINDOWS` or
`VERIFIED_PHYSICAL_HARDWARE` - this document does not upgrade one into
the other anywhere.

============================================================
CIP PHASE 3.5 — OPERATOR UX GATE
============================================================

```
SOFTWARE REGRESSION:
    PASS

OPERATOR UI:
    PASS

    Functional and regression-tested (Environment A/B). Not the same
    claim as physical-hardware usability - see below.

FIRST-USE EXPERIENCE:
    PASS

    The "Ready to Start" hero, readiness row, and mode toggle are
    implemented and build/type-check/lint clean. Never tested with a
    real, uncoached human operator (HOLD on that specific claim - see
    REAL WINDOWS UX below).

REAL WINDOWS UX:
    HOLD (NOT AVAILABLE - no physical Windows machine in this container)

REAL MICROPHONE:
    HOLD (NOT AVAILABLE)

REAL WHISPER:
    HOLD (NOT AVAILABLE)

REAL SECOND DISPLAY:
    HOLD (NOT AVAILABLE)

PRESENTATION READABILITY:
    HOLD (NOT AVAILABLE)

REAL SERVICE WORKFLOW:
    HOLD (NOT AVAILABLE)

CRASH/RECOVERY:
    PASS (Environment B - Xvfb relaunch of the rebuilt binary; not a
    Windows-hardware claim)

INSTALLER:
    PASS (cross-compiled NSIS .exe rebuilt from this phase's source,
    checksum-verified - see release/windows/; never installed on a real
    Windows machine)

OFFLINE:
    PASS (no new network dependency; cargo tree unchanged)

SECURITY:
    PASS (no new Tauri capability, no new command, no new exec surface)

LICENSING:
    PASS (BSB unchanged: 66 books / 1,189 chapters / 31,086 verses /
    checksum d4335582ff26a3ac / verified_public_domain; no new
    translation; no scraping)
```

------------------------------------------------------------

FINAL UX STATUS:

    HOLD

------------------------------------------------------------

MANDATORY STATEMENT:

Real Windows/hardware/human-operator UX validation was not performed in
this build environment. Environment C evidence is BLOCKED for every
physical claim above. Automated and Xvfb evidence does not substitute
for it. Final UX status remains HOLD pending that validation on the
actual HP EliteBook 830 G6, exactly as every prior hardware-pilot phase
(3.1 through 3.4) has honestly reported for the same, unchanged reason.
