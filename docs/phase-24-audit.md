# Phase 24: Dual Live/Preview Slide Panel

## Trigger

Direct operator follow-up after Phase 23's layout-shell milestone: "Continue
there next" - the dual Live/Preview panel with slide thumbnails, the most
visually distinctive element of the operator's own reference images
(professional live-service software - ProPresenter/EasyWorship-style) that
Phase 23 explicitly deferred as a separate milestone.

## What was actually missing

Phase 23 gave the stage column real structure, but `PresentationCard` (the
component occupying it) has always been a text-list of items with badges
("Approved"/"Ready to Present"/"On Screen") and inline text previews - never
a visual rendering of the actual slide the way the reference images show.
Two real, previously-unused pieces of data made a genuine fix possible
without any new backend work:

- `PresentationDisplayState.activeSlide` (`RenderedSlide`) has existed
  since Phase 1.4 and is fetched by `get_presentation_display_state` on
  every mount - but `LiveChurchBrain.tsx`/`ServiceReplay.tsx` only ever
  read `state.activeItem` from that response, discarding `activeSlide`
  entirely.
- `PresentationDisplayPayload` (the `PRESENTATION_STARTED` event payload)
  carries the same pairing - `{ item, slide }` - and both files' event
  handlers destructured only `{ item }`.

The display window itself (`PresentationDisplay.tsx`) has rendered from
exactly this `RenderedSlide` shape since Phase 1.4; the operator's own
window simply never showed it. This phase's Live panel is that same data,
surfaced, not a new rendering system.

## Design decision: additive, not a `PresentationCard` rewrite

A new component, `LivePreviewStage`, was added *above* `PresentationCard`
in the stage column rather than replacing any of its logic:

- **Live panel**: `activeSlide` (see above) - wired via a new `activeSlide`
  state, set from the existing `getPresentationDisplayState` mount effect
  and the existing `onPresentationStarted`/`onPresentationStopped` event
  handlers, all of which were already firing for other reasons.
- **Preview panel**: the same `previews` map `PresentationCard`'s own
  existing "Preview" button already populates via the existing
  `preview_presentation` command - a new `previewSelectionId` state (set
  on every Preview click) picks which entry to show. No new command, no
  second preview mechanism - one more place to see data that command
  already returns.
- **Queue strip**: a horizontal row of prepared items below the two
  panels, each clickable. Deliberately calls the *exact same* `onDisplay`
  handler `PresentationCard`'s own "Display" button already calls - a
  second visual affordance for an already-existing, already-deliberate
  single action, not a new one. Per this project's own explicit-
  activation safety model (`docs/presentation.md`), a click sends the
  item live immediately, exactly like the existing button; it is not a
  "select to preview" affordance, since a *prepared* item (as opposed to
  an *approved* one) has no separate re-renderable preview state without
  a new backend call this phase does not add - see "Known limitations."

`PresentationCard` itself was not touched beyond two additions: its
`onPreviewApproved` handler now also records `previewSelectionId`
alongside its existing `previews` update.

## Real-browser verification

Both screens were rendered end-to-end in headless Chromium with a mocked
Tauri bridge (the same local verification technique Phase 23 established -
see its own audit doc for why this app needs it to get past the login
gate), this time with a populated mock: an active display item with a
real `RenderedSlide` (Romans 8:28) and two prepared items (John 3:16,
Psalm 23:1).

- **Live Service**: confirmed the Live panel renders "Romans 8:28" in the
  black/gold/white slide styling (matching `PresentationDisplay.css`'s
  real projector output, not this app's own navy theme - see "Why the
  slide box uses different colors" below), the Preview panel shows its
  honest empty state ("Preview an approved item below to see it here"),
  and the queue strip shows both prepared items as clickable cards,
  correctly disabled (matching `PresentationCard`'s own "Display" button
  disabled state) because an item is already active.
- **Service Replay**: confirmed the identical rendering using the same
  `LivePreviewStage` component with no new CSS needed - `workspace.css`'s
  classes are plain global CSS, not scoped to `LiveChurchBrain`.

## Why the slide box uses different colors than the rest of the app

`.live-preview-stage__slide` deliberately reuses `PresentationDisplay.css`'s
exact black background / gold heading / white body / gray footer palette,
not this app's own dark-navy operator theme. It is a miniature of what the
real projector/monitor shows, not a themed operator-UI card - its colors
should match the real output the congregation sees, not the chrome around
it. The panel labels ("LIVE" in `--status-live` blue, "PREVIEW" in neutral
gray) use the app's own theme, since those are operator-facing metadata,
not part of the rendered slide itself.

## Why this doesn't disturb anything downstream

- Zero Rust changes, zero new Tauri commands, zero new events. Every value
  `LivePreviewStage` renders was already being fetched or already existed
  in an event payload this codebase already handles.
- `PresentationCard`'s own rendering, state, and event handling are
  unchanged except the two additive lines above - its existing behavior
  (Approve → Preview → Prepare → Display → Stop, per-screen open/close/
  route controls) is untouched.
- The queue strip's click handler is not a new capability: it is the
  exact same `commands.displayPresentation` call, with the exact same
  `busy` key (`display-${id}`) and the exact same `activeDisplayItem`-
  gated disabled state, `PresentationCard`'s own "Display" button already
  uses - clicking either produces identical backend behavior.
- `PresentationDisplay.tsx` (the real display window) is entirely
  untouched - this phase only stopped throwing away data it already
  produces.

## Testing boundary

No new Rust code exists to test. On the frontend: this is a purely
presentational component composed from data other, already-tested code
paths produce (`previews`, `preparedItems`, `activeDisplayItem`, and now
`activeSlide`/`previewSelectionId` set through the same event handlers
already covered by this app's existing runtime behavior). Consistent with
this project's own established convention - no workspace panel component
(`PresentationCard`, `AttentionQueue`, `IntelligenceFeed`, etc.) has a
dedicated render test; their correctness rests on typecheck, build, and
real-browser visual verification (Environment A/B), not a component test
suite. That verification was performed this phase (see above) rather than
assumed.

## Full regression result

`npm run typecheck` 0 errors, `npm run lint` the same 5 pre-existing
warnings (unchanged), `npm run test -- --run` 303/303 (unchanged - no
Rust logic changed and no new pure-logic helper was added that isn't
already exercised by the same command/event plumbing other tests cover),
`npm run build` clean. No Rust code was touched this phase.

## Known limitations (honest, not deferred silently)

- **The queue strip shows real content, not a pixel-accurate preview of
  the exact slide that will be displayed.** It derives its heading/body
  snippet directly from `PresentationItem.content` (the same data
  `PresentationCard`'s own `itemHeading` already uses), not from a
  `RenderedSlide` - a *prepared* item has no rendered slide fetched into
  frontend memory today (only *approved-and-previewed* suggestions do).
  Adding that would need a new "preview a prepared item" backend
  capability; this phase does not add one, so the queue thumbnail's
  styling approximates a slide (dark box, heading, body text) but is
  built from raw content, not literally the render.
- **Clicking a queue item sends it live immediately** (identical to the
  existing "Display" button), not a click-to-preview-then-confirm flow -
  a deliberate choice matching this project's existing explicit-
  activation safety model rather than inventing a new interaction this
  phase can't fully justify without more backend work.
- **This exact change has not been confirmed on real Windows hardware
  running the actual compiled desktop app** - verification used a mocked
  Tauri bridge in headless Chromium, not the real WebView2 renderer real
  operator hardware uses. The decisive test is a real operator running a
  real service, sending an item live, and confirming the Live panel shows
  the same text the actual projector/monitor shows.

## Final gate

Environment A (typecheck/lint/test/build, plus real-browser-engine visual
verification of both screens' Live/Preview panels with populated slide
content via headless Chromium with a mocked Tauri bridge): PASS.
Environment C (a real operator confirming the Live panel matches the real
display output during an actual service): not yet performed.
