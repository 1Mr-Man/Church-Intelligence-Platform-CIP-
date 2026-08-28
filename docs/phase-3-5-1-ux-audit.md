# Phase 3.5.1 — UX Audit (Real Windows Evidence Correction)

## 0. Evidence status (read this first)

This phase's instructions treat a supplied Windows screenshot (and optionally a
video) as REAL Environment C evidence and require it to be the primary visual
reference for this audit.

**Neither the screenshot nor a video is present in this environment.** No image
was attached to the task message, and a filesystem search of the working tree,
`/tmp`, and this session's scratchpad found no screenshot or video file. This is
recorded honestly rather than worked around: no visual observations in this
document are based on a real screenshot. Section 1 below is a **source-code
audit only** (Environment A) — it identifies concrete, verifiable-in-source
defects that would produce a cramped, gray, "engineering console" appearance on
a real 1920x1080 Windows laptop screen, but every claim is traceable to a file
and line, not to a picture.

If the screenshot/video become available in a later turn, this section must be
re-opened and the findings re-checked against it before the gate can move past
`NOT VERIFIED — REAL HARDWARE UNAVAILABLE` for the visual-appearance claims.

## 1. Source-code audit findings

### P0-1 — The entire app is still wrapped in the unmodified Vite/Tauri template shell

`apps/desktop/src/index.css:84-94`:

```css
#root {
  width: 1126px;
  max-width: 100%;
  margin: 0 auto;
  text-align: center;
  border-inline: 1px solid var(--border);
  min-height: 100svh;
  ...
}
```

`#root` wraps the entire React tree, including `LiveChurchBrain`. Three
consequences, all still present on HEAD:

1. `text-align: center` is inherited by every element that doesn't override it.
   `LiveChurchBrain.css`/`workspace.css` never reset `text-align` on `.live-brain`
   or its panels, so **every unstyled paragraph, list item, and `<dd>` value in
   the app is center-aligned** — the Live Transcript list, the Service Timeline,
   recent-references chips, WorkspaceHeader's status values, notice banners. Left-
   aligned body text is the baseline expectation for an application; centered
   prose reads as an unfinished landing page, not production software.
2. `width: 1126px` (a fixed `width`, not a `max-width`) caps the *entire
   application* at 1126px on any screen wider than that — including the HP
   EliteBook 830 G6's 1920x1080 panel. Phase 3.5 widened `.live-brain` to
   `max-width: 1180px` (`LiveChurchBrain.css:15`), but that change has **no
   visible effect**, because the parent `#root` clamps everything to 1126px
   first. Roughly 40% of the screen width sits unused on a real laptop.
3. `border-inline: 1px solid var(--border)` draws a permanent vertical rule down
   both sides of that narrow column, reinforcing the "boxed debug panel" look
   the spec's audit checklist calls out (item D, "overly large bordered
   containers").

This is a leftover from the original `create-tauri-app` template's marketing
landing page CSS (`h1 { font-size: 56px; ... }` a few lines below is the same
template artifact) and was never removed when `LiveChurchBrain` became the
actual application. **This is very likely the single largest reason the real
Windows screenshot still looks cramped and gray despite Phase 3.5's layout
work** — the widening never reached the screen.

**Severity: P0.** **Fix:** remove `text-align: center`, `border-inline`, and the
fixed `width` from `#root`; let `.live-brain`'s own `max-width` govern the
content column.

### P0-2 — No color actually differentiates anything outside a handful of badges

`--status-good/warn/bad/neutral` tokens exist (`index.css:19-30`), but they are
applied in only three places: `op-badge`, `op-status-strip__item`, and
`live-brain__badge`/`live-brain__notice`. Everywhere else — including the parts
of the screen the spec explicitly wants colorful — the app is single-hue gray
text on a near-white (or, in dark mode, near-black) background with one purple
accent reserved for the mode-toggle pill and links:

- `IntelligenceCard`'s domain badge (`workspace.css:50-58`,
  `.workspace-card__domain { border: 1px solid currentColor; }`) inherits
  whatever the surrounding text color is — **Bible, Music, Sermon, Service,
  Content, and Correlation items in the Attention Queue and Intelligence Feed
  are visually identical.** This is the "what CIP detected" surface (spec
  section 10) and it currently carries zero color signal.
- `.live-brain__panel { background: var(--bg); }` — panels are the *same
  color as the page background*, so they read as outlined boxes, not elevated
  cards. There is no depth hierarchy between "Presentation" (should dominate
  the screen per spec section 11) and "Manual Bible Search" (should recede).

**Severity: P0.** **Fix:** a real semantic color system (below) applied to
domain badges, card surfaces, and the Presentation card specifically.

### P0-3 — `WorkspaceHeader` is a raw, uppercase, jargon-heavy status dump at the top of Operator Mode

`apps/desktop/src/components/workspace/WorkspaceHeader.tsx:44-109` renders a
`<dl>` grid immediately below the service control bar with entries including:

- `"UNKNOWN / UNKNOWN"` for Audio/Speech before the first status poll resolves
  (`(status?.audioStatus ?? "unknown").toUpperCase()`)
- `"ACOUSTIC: UNKNOWN"` — "Acoustic" is an internal engineering term for the
  music-recognition subsystem that no church operator has been introduced to
  anywhere else in the UI
- A raw backend error string surfaced as an HTML `title` tooltip:
  `dd title={status?.audio.streamError ?? undefined}`
- `licensingStatus.replace(/_/g, " ").toUpperCase()` — internal licensing-gate
  vocabulary ("PUBLIC DOMAIN CONFIRMED" etc.) shown unconditionally, not gated
  to Diagnostics

Every value in this grid is ALL CAPS with no color and no icon — it is,
verbatim, a debug table. And every genuinely useful fact in it (Bible status,
Audio/Speech status, Output/Display status) already has a better, Phase-3.5
home elsewhere on the same screen: `SystemStatusStrip` (Bible/Mic/Speech/
Display) and `PresentationCard` (Output). `WorkspaceHeader` only contributes
three facts nothing else shows: Service Phase, active Sermon/Speaker, and
Current Song.

**Severity: P0.** **Fix:** delete the duplicated fields, keep the three unique
ones, and render them as the same colorful status-pill language the rest of
Phase 3.5.1 introduces — not a `<dl>`.

### P1-1 — `App.css`'s diagnostic-details styling hardcodes light-only colors

`apps/desktop/src/App.css` hardcodes `color: #666`, `color: #444`,
`color: #555`, `border: 1px solid #ddd` for the "Foundation status" details
block. These are invisible or near-invisible against a dark background. Low
traffic (it's a collapsed `<details>`, Phase 1 diagnostics), but it must be
fixed as part of any base-theme change or it becomes unreadable.

**Severity: P1 (becomes P0 if the base theme changes to dark — see below).**

### P1-2 — Button hierarchy has only two tiers, not three

`op-button--primary` (solid accent) and `op-button--danger` (bordered red
text) exist; every other button — Pause, Cancel, Preview, Refresh, Search,
Correct, Select — falls back to the same flat `--code-bg` gray
(`LiveChurchBrain.css:396-404`). Spec section 20 asks for primary/secondary/
tertiary. Currently there is only primary/everything-else.

**Severity: P1.**

### P1-3 — No iconography for domains, only 4 emoji total

`SystemStatusStrip` uses 🎙🧠📖🖥 for Mic/Speech/Bible/Display. Nothing else in
the app — including the Attention Queue and Intelligence Feed domain badges —
carries an icon. Spec section 6 example layouts show icons throughout
(📖🎵🎙🟣🔗⚙).

**Severity: P1.**

### P2-1 — Presentation card is only modestly distinguished from other panels

`op-presentation` uses a 2px border vs. every other panel's 1px, and the same
`--bg` background otherwise (`workspace.css:272-277`). Spec section 11 calls
Presentation "one of the most visually important parts of the interface" — a
1px-vs-2px border difference will not read that way on a real screen.

**Severity: P2.**

### P2-2 — `StatusBar` (Diagnostics) still uses raw backend words

`"Runtime: tauri"`, `"AI: available"` (`LiveChurchBrain.tsx:2087-2099`). This
is already correctly confined to Diagnostics Mode as of Phase 3.5, so it is
low severity — Diagnostics is explicitly allowed to be technical — but the
labels could still be clearer.

**Severity: P2. No change required; noted for completeness.**

### P3 — No transition/state feedback beyond default disabled-button opacity

No listening pulse, no detection-arrival highlight, no presentation-activation
transition. Spec section 21 asks for restrained motion here; currently there
is none. Lowest priority per spec section 50's "do not block on polish."

**Severity: P3.**

## 2. Top 10 problems, ranked

| # | Problem | Severity | Fix |
|---|---|---|---|
| 1 | `#root` landing-page shell (center text, fixed 1126px width, side borders) still wraps the whole app | P0 | Remove the three offending rules from `#root`; let `.live-brain` govern width |
| 2 | No color differentiates domains (Bible/Music/Sermon/Service/Content/Correlation badges are all `currentColor`) | P0 | Real 8-color semantic system, applied to domain badges and card accents |
| 3 | `WorkspaceHeader` is a raw ALL-CAPS `<dl>` with jargon (`ACOUSTIC`, raw licensing status, a raw error tooltip) duplicating what `SystemStatusStrip`/`PresentationCard` already show better | P0 | Trim to the 3 non-duplicated fields (Phase, Sermon/Speaker, Song), restyle as status pills |
| 4 | Panels are the same color as the page background — no elevation/depth hierarchy | P0 | Elevated card surface token, applied via `.live-brain__panel` |
| 5 | `App.css` diagnostics-details block hardcodes light-only hex colors | P1 | Switch to CSS custom properties |
| 6 | Only 2 button tiers (primary/everything-else), no secondary tier | P1 | Add `.op-button--secondary` |
| 7 | Almost no iconography outside `SystemStatusStrip`'s 4 emoji | P1 | Add domain icons to `IntelligenceCard` |
| 8 | Presentation card only 1px more prominent than other panels | P2 | Larger visual weight: gradient/tint, larger heading |
| 9 | `StatusBar` (Diagnostics-only) uses raw backend words | P2 | No change required (Diagnostics is allowed to be technical) |
| 10 | No state-transition motion (listening pulse, detection arrival) | P3 | Deferred — not blocking per spec section 50 |

## 3. What must remain unchanged

Confirmed by reading `LiveChurchBrain.tsx` in full and every `workspace/*.tsx`
component before writing this document:

- Every `commands.*` call and its parameters (Tauri command surface)
- Every `liveEvents.*` subscription and its payload shape (Tauri event surface)
- `buildUnifiedFeed` / `buildAttentionQueue` / `actionsFor` (Phase 2.9 unified
  workspace projection) — untouched, still the single source of "what needs
  attention" / "what happened"
- `PresentationItem`, the renderer, and the explicit-activation state machine
  (Prepared → Active → Stopped) — untouched; Phase 3.5.1 only changes how these
  states are *styled*, never when they transition
- All backend crates (`core/*`, `integrations/*`, `ai/*`, `database/*`,
  `apps/desktop/src-tauri/*`) — zero Rust files require any change to fix the
  findings above. Every defect found is CSS, or a presentational trim of one
  React component (`WorkspaceHeader`) that already receives all the data it
  needs as props.

## 4. Conclusion

Every P0/P1 finding above is fixable entirely within `index.css`, `App.css`,
`LiveChurchBrain.css`, `workspace.css`, and small presentational edits to
`WorkspaceHeader.tsx` and `IntelligenceCard.tsx`. No backend contract, Tauri
command/event, or intelligence-architecture change is required or will be
made. Implementation proceeds directly from this list.
