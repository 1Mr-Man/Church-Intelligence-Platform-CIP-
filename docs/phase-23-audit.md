# Phase 23: Broadcast/Production-Dashboard Layout Shell

## Trigger

Direct operator feedback, with four reference images: CIP's own actual Live
Service screen (a single stacked column - "Ready to Start" / Service
Snapshot / Needs Attention / Intelligence Feed, one after another); two
professional live-service production tools (resembling ProPresenter/
EasyWorship) showing a schedule/playlist rail, a dual Live/Preview stage
panel with slide thumbnails, and a scripture panel; and a dense
Bible-study-tool screenshot (Logos) the operator did not choose. Asked two
clarifying questions before starting: which milestone to build first
("Layout shell first" was chosen over "Live/Preview panel first" or "full
visual pass on current layout") and which screens to redesign ("Live
Service + Service Replay" was chosen over "Live Service only" or "all
screens").

## What "shallow" meant, concretely

`docs/phase-3-5-*` (Phase 3.5.1) had already committed CIP to a real,
professional dark theme (deep navy/charcoal surfaces, a full semantic
status-color system) explicitly modeled on "real production church-AV
software (ProPresenter, OBS, vMix)" - so the operator's complaint was never
about color or density. It was structural: every section of the Live
Service screen - service controls, status snapshot, attention queue,
intelligence feed, the presentation card, audio/speech controls, scripture
context, live transcript - rendered in one single stacked column,
regardless of how operationally different those sections are (background
service status vs. the thing actually being displayed to the congregation
vs. supporting intelligence). The reference images' shared trait, across
both chosen ones, was a genuine multi-column layout with distinct zones.

## Design decision: a 3-column grid via `grid-column`, not `grid-template-areas`

`LiveChurchBrain.tsx` is 2,896 lines; this file's own Phase 3.5 comment
already flags "large text moves" as a real correctness risk in a component
this size. A named `grid-template-areas` layout would require every
section to become a contiguous, uniquely-named grid item - which, given
the many non-contiguous sections that belong in the same column (e.g.
`IntelligenceFeed` sits between `AttentionQueue` and `PresentationCard` in
the JSX but belongs in a different column than either), would mean
physically relocating a large fraction of the file's JSX.

Instead: three columns are declared via `grid-template-columns` alone, and
each *child* independently declares which column it belongs to via a
plain `grid-column: 1/2/3` utility class (`.live-brain__col-rail`/
`-stage`/`-intel`). This let every section be *wrapped* in place -
`<div className="live-brain__col-rail">...</div>` around existing JSX -
rather than moved. No section's own internal JSX changed at all; only
new wrapping `<div>`s were added, and existing top-level elements were
grouped into contiguous same-column spans where possible. Verified this
carries zero JSX-correctness risk by confirming `tsc -b` still compiles
cleanly (a mismatched or dropped tag fails loudly) and all 303 existing
frontend tests still pass unchanged.

Column assignment:

- **Rail** (left, ~240-300px): `ServiceControlBar`, `WorkspaceHeader`,
  `SystemStatusStrip`, the setup-gaps notice, `AttentionQueue` - "what's
  the state of the service and what needs my attention."
- **Stage** (center, flexible): `PresentationCard` (the Approve → Prepare
  → Display pipeline - already the closest existing analogue to the
  reference images' Live/Preview panel), plus (operator mode only) Audio
  & Speech, Current Scripture/Active Context, and Live Transcript - "the
  thing actually on-air and the controls that feed it."
- **Intel** (right, ~280-340px): `IntelligenceFeed`, `DisplayRegistryPanel`,
  `ProductionIntegrationPanel`, `CongregantCompanionPanel` - supporting
  detail, secondary to the stage.

Diagnostics Mode's own large stack of `<details>` panels (troubleshooting/
advanced content, not what the reference images depict) is deliberately
**left outside the grid**, unchanged - this phase's scope is the
operator's live-service view, not the diagnostics/developer view.

## A real bug found and fixed during this phase: CSS Grid sparse packing

Wrapping non-contiguous sections in repeated `.live-brain__col-rail`/
`-stage`/`-intel` divs interacts with CSS Grid's default *sparse*
auto-placement algorithm in a way that is easy to get wrong: when a later
same-column item follows a taller item in a *different* column (in DOM
order), the sparse algorithm's placement cursor has already advanced past
the row where the earlier same-column item ended, so the later item gets
pushed down - leaving a visible dead gap in that column, not a compact
stack. This was not caught by any automated check (typecheck/tests/build
all stayed green through it) - it was only found by actually rendering the
page and looking, exactly why this phase did that rather than relying on
static checks alone. `grid-auto-flow: dense` fixes it (the browser
backfills earlier holes instead of only ever moving forward); confirmed
fixed by re-rendering both screens after the change.

## Real-browser verification (not just typecheck/tests/build)

Both screens were rendered end-to-end in headless Chromium and visually
inspected as real screenshots, not inferred from code alone - the standing
"start the dev server and use the feature in a browser" requirement for UI
changes. This app requires a Tauri backend for real IPC, so a `window.__TAURI_INTERNALS__` mock (in-memory fixture responses per command name,
covering `get_app_config`/`get_live_status`/`get_pilot_diagnostics`/
`list_operator_accounts`/etc.) was used to get past the login gate and
render the actual component tree with real CSS layout in a real browser
engine - not a jsdom snapshot, which cannot lay out CSS Grid. This is a
local verification technique for this session only; nothing about the
mock was committed to the repository.

- **Live Service** (`LiveChurchBrain.tsx`, operator mode, idle/first-run
  state): confirmed a genuine 3-column shape - rail (Ready to Start,
  Service Snapshot, status badges, Setup remaining, Needs Attention),
  stage (Presentation, Audio & Speech, Current Scripture, Live
  Transcript), intel (Intelligence Feed, Display Setup, Production
  Integration, Congregant Companion) - with no mid-column gaps after the
  `grid-auto-flow: dense` fix.
- **Service Replay** (`ServiceReplay.tsx`): confirmed the same 3-column
  shape using the identical `.live-brain__grid`/`col-rail`/`col-stage`/
  `col-intel` classes (they are plain global CSS, not scoped to
  `.live-brain`, so no new CSS was needed for this screen beyond a
  `.library-page--replay-grid` max-width override, scoped narrowly so
  BibleLibrary/MusicLibrary/HistoryView - which share the exact same
  `.library-page`/`.library-page--history` base classes - are unaffected).

## Why this doesn't disturb anything downstream

- Zero Rust changes. Zero new Tauri commands, events, migrations, or
  schema changes. Zero changes to any command handler, business logic,
  or data shape.
- Every wrapped component's own props, internal JSX, and behavior are
  byte-for-byte unchanged - only new wrapping `<div>`s with layout-only
  classes were added around them.
- `.live-brain__grid`/`col-rail`/`col-stage`/`col-intel` are new,
  additive CSS classes; no existing class's definition was changed
  (`.live-brain`'s own `max-width` was widened from 1180px to 1600px to
  give the new columns room, and `.library-page--replay-grid` is a new,
  narrowly-scoped modifier - neither affects any other screen's existing
  layout, confirmed by BibleLibrary/MusicLibrary/HistoryView all
  continuing to use the unmodified `.library-page` base rule alone).
- Diagnostics Mode's large detail-panel stack, `BibleLibrary`,
  `MusicLibrary`, and `HistoryView` are entirely untouched.

## Full regression result

`npm run typecheck` 0 errors, `npm run lint` the same 5 pre-existing
warnings (unchanged), `npm run test -- --run` 303/303 (unchanged - this
phase added no new component logic, only layout wrapping), `npm run
build` clean. No Rust code was touched this phase, so the Rust regression
suite and Windows rebuild are unaffected and were not re-run - `git
status` confirms no Rust files changed.

## Known limitations (honest, not deferred silently)

- **This is Milestone 1 of the redesign only** (the operator's own chosen
  first step). The reference images' most visually distinctive element -
  a true dual Live/Preview panel with slide thumbnails - is not built
  this phase; `PresentationCard` (reused as-is in the stage column) is
  the closest existing analogue (Prepared → Displayed, not a literal
  side-by-side Live/Preview pair with thumbnail strip) but is not a new
  component. That remains a real, separate future milestone.
- Diagnostics Mode keeps its original single-stacked-column layout
  entirely unchanged - by the operator's own chosen scope, not an
  oversight.
- **This exact change has not been confirmed on real Windows hardware
  running the actual compiled desktop app** - verification in this
  container used a mocked Tauri bridge in headless Chromium, a real
  browser engine's CSS Grid implementation, but not the real WebView2
  renderer real operator hardware uses. The decisive test is a real
  operator opening CIP Desktop on Windows and confirming the 3-column
  layout renders correctly across their actual screen size(s).
- The 1200px collapse-to-single-column breakpoint (for windows/screens
  narrower than the 3-column shape needs) is reasoned, not empirically
  tested against a real narrow-window CIP session.

## Final gate

Environment A (typecheck/lint/test/build, plus real-browser-engine visual
verification of both redesigned screens via headless Chromium with a
mocked Tauri bridge): PASS. Environment C (a real operator confirming the
new layout on real Windows hardware, at their actual screen resolution):
not yet performed.
