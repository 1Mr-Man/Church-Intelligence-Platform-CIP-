# Phase 3.8.2 — Audit

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `1da6279` (Phase 3.8.1, "Service Replay Intelligence +
  Professional Live-Service Operator Workspace")
- Working tree at start: clean

This audit was written before any Phase 3.8.2 code changed, per this
phase's own "audit first" requirement. It investigates the real Windows
evidence the operator supplied (Evidence A-D) and traces each to a
concrete, provable cause in the actual current code, or explicitly marks
a claim `NOT VERIFIED` where this environment cannot prove it.

## B. Evidence A — Replay blocked without an active service

**Code**: `ServiceReplay.tsx`'s `startReplay()`:

```ts
if (!serviceActive) {
  throw new Error("Start a service first - Service Replay needs an active service, exactly like real speech would.");
}
```

**Root cause, confirmed**: this is a genuine UX dead-end, not a backend
defect. The Replay button is enabled whenever a transcript is loaded,
regardless of service state, so an operator who imports a transcript and
immediately presses "Start Replay" hits this thrown error every time -
there is no guidance, and no automatic recovery. The backend has no
objection to starting a service first; nothing prevents combining the two
actions.

## C. Evidence B — "a service is already active"

**Code**: `commands::start_service` (`commands.rs:520`) calls
`ensure_no_active_service(...)` before creating a session - a legitimate,
correct invariant (CIP must never run two concurrent services). The
error text itself, and the guard, are correct and must be preserved.

**Root cause, confirmed**: the defect is at the call site, not the guard.
`ServiceReplay.tsx`'s `runFullService()` calls `commands.startService(...)`
unconditionally, with no check of `serviceActive` first - so pressing
"Run Full Service" while a replay-driven service is already active
reproduces this exact error. `startTestService()` similarly has no
disabled/hint state beyond the existing conditional render (the "Start
Service" button is already hidden once `serviceActive` is true, but "Run
Full Service" is not).

## D. Evidence C — "Nothing detected yet" / "No theme or key points detected yet"

**Root cause, confirmed by direct causal chain, not by re-guessing**:
Evidence C is a **downstream consequence of Evidence A**, not an
independent defect. If replay is blocked before it starts (Evidence A),
zero segments are ever fed to `processTestTranscript`/
`analyzeBibleTranscript`/`analyzeSermonTranscript` - so the real engines
never had any input to work with. "Nothing detected yet" was completely
honest given zero segments processed. Phase 3.8.1 already proved (via
`phase_3_8_1_service_replay_progressive_intelligence_acceptance`) that
when segments genuinely are fed through, real Bible and Sermon findings
are produced and appear progressively. There is no separate "UI fetches
stale state" or "engine broken" defect to fix here - fixing Evidence A
(section E below) is what allows Evidence C to resolve itself. This is a
deterministic, threshold-gated engine (`SermonIntelligenceEngine`): a
transcript that never triggers enough repetition/structural evidence for
a theme legitimately produces no theme, and that must remain visible as
an honest "nothing yet," never fabricated - the existing copy already
says this correctly and is not changed.

## E. Evidence D — "CIP Presentation Display (Not Responding)"

Traced the complete display lifecycle:

1. `commands::display_presentation` (`commands.rs:1797`): renders the
   slide, calls `presentation_display::open_display_window(&app)`
   (creates the second `WebviewWindow` if not already open), then
   **immediately** calls `commit_activation` and `emit(&app,
   AppEvent::PresentationStarted, payload)` - no wait, no delay, no
   confirmation that the new window's JavaScript has finished loading.
2. `presentation_display.rs`'s `open_display_window`: `WebviewWindowBuilder::build()`
   returns once the native window/webview object is created. On Windows
   this uses WebView2, whose actual page load (`index.html` fetch, React
   mount, the `useEffect` in `PresentationDisplay.tsx` running and calling
   `liveEvents.onPresentationStarted(...)`) happens asynchronously, on the
   webview's own thread, **after** `build()` already returned to the
   Rust caller.
3. `PresentationDisplay.tsx` (`main.tsx` routes the `"display"`-labeled
   window here): its **only** source of truth is the
   `onPresentationStarted`/`onPresentationStopped` event listeners
   registered in a `useEffect` on mount. There is **no call to
   `get_presentation_display_state` or any other command to hydrate
   current state** - if the `PresentationStarted` event fires before this
   component has mounted and subscribed (step 1 happening faster than
   step 2's JS-side readiness, which is entirely plausible and, on a
   first-run WebView2 environment initialization, could take noticeably
   longer than a warm one), the event is missed **permanently** - the
   window opens and stays blank until the *next* presentation state
   change, which may never come again in that operator's Prepare → Preview
   → Display session.

**Confirmed, provable defect**: `PresentationDisplay.tsx` has no
initial-state synchronization mechanism, even though the exact command it
would need (`get_presentation_display_state`) already exists and is
already used by the main window for the same purpose. This is a real,
reproducible race condition regardless of platform - it does not require
Windows-specific behavior to exist, only requires the JS side to mount
slower than the Rust side re-emits, which is a realistic ordering on any
platform and is what "blank screen" symptoms would look like.

**What this audit can and cannot verify about "Not Responding"
specifically**: "Not Responding" (a Windows shell/window-manager label) is
a stronger symptom than "blank" - it implies the window's message loop
itself stopped responding, not merely that its content never updated. This
environment has no access to WinDbg, ETW, or any Windows process
inspection tool, and no physical Windows machine, so **the exact
mechanism behind the literal "Not Responding" title bar text is `NOT
VERIFIED`** - it may be: (a) the operator's honest reading of a window
that loaded but never displayed anything (a blank/frozen-looking window is
routinely misdescribed by Windows/end users as "not responding" even when
its message loop is fine), (b) a genuine WebView2 environment
initialization stall (a known class of first-run WebView2 issue,
unrelated to this codebase), or (c) something else entirely. This audit
does **not** claim to have proven which. It **does** identify and fix a
real, independently provable architectural gap (no initial-state
hydration) that removes the most likely proximate cause of a
never-updating display window, and adds a second, independently provable
fix (section F below) for a related close/reopen race - both are
justified on their own merits regardless of which exact failure mode the
operator's screenshot captured.

## F. Display window close/reopen — a second, related defect found

**Code**: `commands::close_presentation_display` (`commands.rs:1902`)
calls only `presentation_display::close_display_window(&app)`, which
calls `window.close()`. It does **not** call `clear_active_presentation`
itself. The only path that stops the active item on close is the
`Destroyed` window-event handler registered in `open_display_window`,
which fires **asynchronously**, after the OS has actually finished
destroying the window - which happens *after* `window.close()` (and
therefore the `close_presentation_display` command) has already returned
to the frontend.

**Root cause, confirmed by direct code trace**: if the operator's next
action after "Close Display" is fast enough (e.g., a scripted or a
quick UI reopen-and-display-another sequence), `prepare_to_activate` for
the *new* item can run **before** the `Destroyed` handler has reconciled
the *old* item to `Stopped` - `prepare_to_activate` (`presentation.rs:221`)
explicitly checks `already_active` and returns
`PresentationError::AlreadyActive(existing.id)` in that case. This exactly
matches the class of defect the spec's "Display Window Reopen Test"
(section 9) and completion criteria ("no orphaned active presentation
state," "display can close and reopen") are testing for - a real,
reproducible race, not a hypothetical one, confirmed by reading the two
code paths directly (no event ordering guarantee exists between "Close
Display command returns" and "Destroyed handler runs").

## G. Long real transcript / performance

Traced `segmentTranscript` (Phase 3.8.1, unchanged this phase) and
`playLoop`: segmentation is synchronous but O(n) over the transcript
(paragraph split, then per-paragraph sentence split/chunk, no nested
scan) - a multi-thousand-word transcript segments in well under a frame.
`playLoop` processes exactly one segment at a time, awaiting each
command call before advancing, exactly as required; nothing loads or
processes the "entire transcript as one huge synchronous operation."
Confirmed no O(n²) construct exists in `replay.ts`. This architecture
already satisfies section 12's performance requirement; the fix this
phase adds (section E/F) does not change this path. Section G's
completion bar is therefore proven by re-running the existing
`replay.test.ts` suite against a synthetic long transcript (added this
phase) rather than by any code change to segmentation itself - the 3.8.1
audit already fixed the segment-count and chunk-size defect, and nothing
new is required here.

## H. Gap register

| # | Gap | Category | Fix location |
|---|-----|----------|---------------|
| 1 | Replay blocked with no recovery path (Evidence A) | Service lifecycle UX | `ServiceReplay.tsx` — combined Start-Service-and-Replay |
| 2 | "Run Full Service" can hit an avoidable already-active error (Evidence B) | Service lifecycle UX | `ServiceReplay.tsx` — disable/guard when a service is already active |
| 3 | Nothing detected (Evidence C) | Downstream of #1, no independent fix needed | closed by fixing #1 |
| 4 | Display window can miss `PresentationStarted` if it mounts after the event fires (Evidence D contributor) | Presentation display race | `commands.rs` (`PresentationDisplayState` gains `activeSlide`) + `PresentationDisplay.tsx` (pull on mount) |
| 5 | Close Display does not synchronously reconcile the active item, risking a reopen failure | Presentation display race | `commands.rs` (`close_presentation_display` calls `clear_active_presentation` synchronously) |

No new Tauri command is required for either presentation fix - both are
additive changes to already-existing commands (`get_presentation_display_state`
gains one response field; `close_presentation_display`'s existing body
gains one already-existing, already-idempotent internal call). No new
database migration, no new intelligence engine, no change to the Bible
provider, Bible schema, or any intelligence engine's detection logic is
justified or made.

## I. Implementation plan

1. `ServiceReplay.tsx`: `startReplay()` starts a service automatically
   (reusing the existing `serviceTitle` input) when none is active,
   instead of throwing - a true combined action, guarded by the same
   `serviceActive` check so a second service is never created when one
   already exists. `runFullService()` gains an explicit guard (disabled
   button + inline hint) when a service is already active, so it can
   never reach the backend's already-active error from a normal click.
2. `commands.rs`: extend `PresentationDisplayState` with
   `active_slide: Option<RenderedSlide>`, computed via the existing, pure
   `cip_presentation_renderer::render_content` when an active item
   exists (mirrors exactly what `display_presentation` already does for
   the live-event payload - no second rendering system). `close_presentation_display`
   calls `clear_active_presentation` synchronously before closing the
   window, so the active item is guaranteed `Stopped` by the time the
   command returns, regardless of whether/when the `Destroyed` handler
   later fires (which remains for the manual-OS-close case and is now
   provably idempotent either way).
3. `PresentationDisplay.tsx`: on mount, calls
   `commands.getPresentationDisplayState()` once, alongside subscribing to
   the existing events, and hydrates from `activeItem`/`activeSlide` if
   present - closing the race regardless of which arrives first.
4. `domain/presentation.ts`: mirror the new `activeSlide` field.
5. New Rust tests: `close_presentation_display`/`clear_active_presentation`
   synchronous-reconciliation behavior; `get_presentation_display_state`
   returning a real rendered slide for a genuinely active item, and
   `None` when nothing is active.
6. New frontend tests: `PresentationDisplay.tsx`'s mount-hydration
   behavior (a mocked `getPresentationDisplayState` populates the slide
   without any event ever firing); `ServiceReplay.tsx`'s combined
   start-and-replay behavior and the guarded Run Full Service button.
7. **Correction, confirmed by direct filesystem check**: the operator's
   real transcript file (`tactiq-free-transcript-k-PJ1yu1pZQ.txt`) was
   never actually transferred into this environment - `find / -iname
   "*tactiq*"` and `find / -iname "*k-PJ1yu1pZQ*"` both return no results.
   This session cannot use it, and does not claim to. A new long-transcript
   regression test instead uses a project-authored synthetic transcript of
   comparable real-world scale (multi-thousand-word, many paragraphs, no
   blank-line breaks in places, mixed with real Scripture references) to
   prove segmentation/replay handles a realistically large import without
   hanging or producing an unreasonable segment count - not to assert any
   specific detected reference (the real detector decides). If the operator
   provides the actual file in a future session, it should be used
   directly at that point.
8. Full regression, Windows/Linux rebuild, docs, evidence, commit.

Proceeding to implementation as scoped above.
