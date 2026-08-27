# Presentation Foundation & Real-Service Validation (Phase 1.4)

This document explains what Phase 1.4 added on top of Phase 1.3's
operator workflow: a real presentation preparation path from an approved
suggestion (or a manual reference) through to persisted, prepared output,
and the validation that proves the whole pipeline - detection through
presentation - behaves correctly and never bypasses the operator. **A
later addition, described in "Local display foundation" below, closes the
one remaining gap this section originally documented: CIP can now
actually put a prepared item onto a local display window under explicit
operator control.**

**Core principle, unchanged and made explicit by this phase's data model:
APPROVED is not PREPARED, and PREPARED is not DISPLAYING.** Approval means
the operator accepted a suggestion. Preparation means real, local-Bible
content has been rendered and persisted, ready for output. Displaying
means an explicit output/display action has occurred. As of the "Local
display foundation" addition below, that action is real: an explicit
operator Display click opens/updates a second window and only then
commits `Prepared -> Active`. Nothing else - no finding, no candidate, no
preview, no automatic pipeline step - can ever cross that boundary. See
"No automatic preparation, no automatic projection" below.

**Not in this phase (Phase 1.4 itself):** OBS/vMix integration, real
"display" action, song/hymn recognition, sermon intelligence, content
generation, cloud sync. See [`README.md`](../README.md) for the full
phase boundary. **The local display window** (a second Tauri window
showing a prepared item on a projector/TV/secondary monitor) **was added
later** - see "Local display foundation" below - but OBS/vMix/NDI/
streaming/multi-output integration remains explicitly out of scope for
that addition too.

## Local display foundation

This section documents the local presentation display window: the
smallest real display output CIP has, added on top of everything else
this document describes without changing any of it. It answers "can CIP
actually put a prepared item onto a screen" with **yes, under explicit
operator control, via a second local Tauri window** - while leaving OBS,
vMix, NDI, streaming, and multi-output entirely out of scope.

### Why a second Tauri window, and nothing else

CIP already renders every prepared item as a `RenderedSlide` (see
"Renderer & template" above); the only missing piece was somewhere to
actually show it outside the operator's own control window. A second
`tauri::WebviewWindow` - the same frontend bundle, branching on its own
window label - is the smallest possible real answer: no second build, no
new rendering logic, no new content model, no OBS/NDI/vMix output stage.
`apps/desktop/src-tauri/src/presentation_display.rs` owns window
lifecycle only (open/close/detect); `apps/desktop/src-tauri/src/presentation.rs`
owns every decision about whether an item may be displayed and the actual
`Prepared -> Active -> Stopped` persistence transitions, matching this
module's existing Tauri-agnostic, independently-unit-tested pattern.

### Lifecycle: Prepared -> Active -> Stopped, for real this time

```
PREPARED --[operator: Display]--> ACTIVE --[operator: Stop, or manual window close]--> STOPPED
```

No new `PresentationItemStatus` variant was added - `Active` already
existed in the data model and the database `CHECK` constraint already
allowed it (see "Data model" above); this addition is the first code path
that actually uses it. The transition is deliberately split into two
phases so an item is **never** marked `Active` before the real display
operation has actually succeeded:

1. **`prepare_to_activate`** (pure, no persistence): confirms the item is
   currently `Prepared`, confirms no other item for the same service is
   already `Active` (at most one `Active` item at a time, enforced here),
   and renders the content via the existing `render_content()`. Any
   failure here (wrong status, another item already active, unrenderable
   content) returns an error and touches no database row.
2. The Tauri command layer then performs the real side effect: opens (or
   focuses) the display window and pushes the rendered slide to it.
3. **`commit_activation`** (persistence only): only once step 2 has
   actually returned success does this transition the row from `Prepared`
   to `Active` and emit `PresentationStarted` (see "Events" below).

If step 2 fails (e.g. the window fails to open), step 3 never runs - the
item stays `Prepared`, exactly as if Display had never been clicked.

**Stop** (explicit operator action), **Close** (explicit operator action
on the display window itself), and a **manual close** of the display
window (the operator's OS window controls, Alt+F4, etc.) all converge on
the same shared reconciliation function,
`commands::clear_active_presentation`: blank the display, transition the
`Active` item to `Stopped`, and emit `PresentationStopped`. This means
closing the display window by hand can never leave CIP in a state where
the database says "Active" but nothing is actually showing - the same
guarantee an explicit Stop gives.

### Restart safety

An unclean shutdown (crash, force-quit) could in principle leave a row
persisted as `Active` with no window actually open. `lib.rs`'s `setup()`
runs `persistence::reconcile_stale_active_presentation_items()` -
`UPDATE presentation_items SET status = 'stopped' WHERE status = 'active'`
- once, before any window or command exists, so a restart can never be
mistaken for "still displaying." This was verified against the real
on-disk development database, not just an in-memory test fixture: a real
`Active` row was inserted directly via the same persistence functions the
app itself uses, the real compiled binary was launched under Xvfb, and
its own log file recorded `reconciled 1 presentation item(s) left active
by a previous run to stopped` - see "Desktop runtime verification" in
this phase's implementation report for the full trace. A relaunch with
nothing stale to reconcile logs nothing (the sweep is a silent no-op),
matching `reconcile_stale_active_presentation_items_is_a_safe_no_op_when_nothing_is_active`.

### The display window is a passive renderer only

`capabilities/display.json` grants the `display` window `core:default`
only - the exact same minimal grant the `main` window already has. This
application has no filesystem, shell, HTTP, or dialog plugin installed at
all, so the display window has no more capability surface than `main`
ever did, and specifically: no database connection, no ability to call
any operator command, no filesystem/shell/network access. It only ever
listens for the existing `PRESENTATION_STARTED`/`PRESENTATION_STOPPED`
events (the same public event bus every other window already uses) and
renders whatever `RenderedSlide` arrives, or a blank screen when none is
active. It carries no operator controls (no Stop button, no menu) - all
control lives in the operator's own `main` window.

### Operator workflow

In the **Current Output** panel: each `Prepared` item gets a **Display**
button (disabled while another item is already `Active`, for the
at-most-one-active invariant). An **Open Display** / **Close Display**
button pair controls the display window independently of any content
being shown on it. Once an item is `Active`, it appears in its own card
with a **Stop** button. Duplicate Display clicks, a Display click with no
display window open yet (it opens automatically), and closing the
display window mid-show are all handled - see "Testing" below for the
specific tests covering each.

### What remains explicitly out of scope

OBS/vMix/NDI integration, streaming output, multiple simultaneous
displays/outputs, a real second monitor in this development environment
(see "Desktop runtime verification" in the implementation report for
exactly what was and wasn't verified), and any visual/typographic
redesign of `RenderedSlide` beyond what "Renderer & template" above
already describes.

## What already existed, and what this phase added

Phase 1.0-1.3 already had `core/presentation::PresentationItem`, a
`presentation_items` table, `presentation/renderer::Renderer` (a
`NullRenderer` stub), and one command (`prepare_presentation`) that built
a `Scripture` item from an approved suggestion. Phase 1.4 did not replace
any of that - it extended it:

- `PresentationItem` gained two nullable fields: `source_suggestion_id`
  (which suggestion, if any, this item came from) and `template` (which
  rendering template produced it). Both default to `None`/unset via
  `PresentationItem::with_source_suggestion()`/`.with_template()`, so
  every Phase 1.0-1.3 construction call site still compiles unchanged.
- `presentation/renderer` gained a real, deterministic
  `render_content()` function and a `RenderedSlide` output type,
  alongside the existing `Renderer` trait/`NullRenderer` (untouched).
- A new `apps/desktop/src-tauri/src/presentation.rs` module (mirroring
  `pipeline.rs`'s Tauri-agnostic, directly-unit-testable pattern) holds
  the shared logic between previewing and preparing, so both paths are
  provably identical.
- `commands.rs` gained `preview_presentation`, `preview_scripture`, and
  `create_manual_presentation`, `list_prepared_presentations`,
  `get_presentation_item`, `cancel_presentation`; `prepare_presentation`
  was refactored to use the new shared module rather than duplicating its
  own Bible lookup, and now also records `source_suggestion_id`.

No duplicate presentation abstraction was created anywhere in this phase.

## Data model

`PresentationItemStatus` still has exactly the three values it had in
Phase 1.0 (`Prepared` / `Active` / `Stopped`) - Phase 1.4 did not invent
new states the architecture can't support. They map conceptually onto the
spec's NOT_PREPARED/PREPARED/DISPLAYING/CANCELLED language as follows:

| Conceptual state | Representation                                   |
| ----------------- | ------------------------------------------------- |
| NOT_PREPARED       | no `presentation_items` row exists yet             |
| PREPARED           | `PresentationItemStatus::Prepared`                 |
| DISPLAYING         | `PresentationItemStatus::Active` - set only by an explicit operator Display action, via `presentation_display.rs`; see "Local display foundation" below |
| CANCELLED          | `PresentationItemStatus::Stopped`, reused as "prepared then retracted" - unambiguous in practice since nothing here ever reaches `Active` first |

`PresentationContent` is unchanged: `Scripture { reference,
translation_id, text }` or `Text { title, body }`.

## Content integrity: real local Bible text only

`presentation::build_scripture_slide()` is the single function both
preview and prepare call to produce `PresentationContent`. It:

1. Parses the display reference (e.g. `"ROM 8:28"`) into book/chapter/verse.
2. Looks up the verse via `BibleProvider::get_verse()` against the
   requested translation - the real, local, SQLite-backed provider, same
   as every earlier phase. No AI model, no web request, no generated or
   paraphrased text is ever involved.
3. If the verse (or translation) isn't found locally, returns
   `PresentationError::VerseNotFound` - it never falls back to a
   different translation or invents missing text. A request for a
   translation this install doesn't have (e.g. `NIV` when only `KJV` is
   seeded) surfaces the same clear "not found" error, not a silent KJV
   substitution.
4. Renders the content via `cip_presentation_renderer::render_content()`.

Because both the automatic (suggestion-based) and manual (reference-based)
paths call this same function, they are provably identical for the same
input - see `presentation::tests::preview_and_prepare_paths_produce_identical_content_for_the_same_reference`
and the canonical acceptance test's manual-path assertion.

### Provenance: where did this Scripture text come from?

`PresentationContent::Scripture`'s `translation_id` (e.g. `"KJV"`) is also
a Content Registry lookup key: Phase 1.5's `core/content::bible_content_id`
convention turns it into `"bible:KJV"`, the id under which that
translation's provenance and licensing metadata is registered (publisher,
copyright, license, distribution permission, dataset version, checksum -
each `UNKNOWN`/`null` rather than guessed if the source dataset didn't say).
So a prepared item's full answer to "where did this text come from" is:
`translation_id` on the item itself, plus `content_registry.get("bible:" +
translation_id)` for everything about that dataset's origin and right to be
displayed. This phase does not add a `content_id` column to
`presentation_items` - the `"bible:<translation_id>"` convention is enough
to derive it without duplicating data that can already be looked up. See
[`docs/content-registry.md`](content-registry.md#provenance-and-traceability) for the
worked example and [`docs/bible-datasets.md`](bible-datasets.md) for how
that metadata gets there in the first place.

### Translations

Only `KJV` (the tiny development fixture) was seeded when this section
was originally written; the real Bible dataset production import
milestone (see [`docs/bible-production-dataset.md`](bible-production-dataset.md))
subsequently installed a second, complete translation - the Berean
Standard Bible (`BSB`) - so both now exist side by side. `DEFAULT_TRANSLATION_ID`
(`state.rs`) remains `"KJV"` (unchanged, to keep every earlier phase's
existing test assumptions intact); every presentation command
(`preview_scripture`, `preview_presentation`, `prepare_presentation`,
`create_manual_presentation`) accepts an optional `translationId`
parameter that defaults to `DEFAULT_TRANSLATION_ID` when omitted, and
now also checks the Content Registry's enabled/disabled status before
resolving any translation (`ensure_translation_selectable` in
`commands.rs`) - nothing here assumes or silently substitutes a
different translation than the one requested, and a disabled translation
is rejected explicitly rather than silently falling back.

## Renderer & template

`presentation/renderer::render_content(&PresentationContent) ->
Result<RenderedSlide, RenderError>` is a pure, deterministic function: no
AI generation, no randomness, no I/O. The same content always produces
the same `RenderedSlide`. It rejects content with an empty reference,
translation id, or verse/body text (`RenderError::InvalidContent`) rather
than ever producing broken output.

`RenderedSlide` is intentionally simple - a structured, un-styled slide:

```rust
pub struct RenderedSlide {
    pub template: String,       // "SCRIPTURE_DEFAULT" or "TEXT_DEFAULT"
    pub heading: String,        // the reference, or a text item's title
    pub body_lines: Vec<String>, // deterministically word-wrapped (42-char safe margin)
    pub footer: Option<String>, // the translation id, for Scripture content
}
```

`SCRIPTURE_DEFAULT` is the one solid, deterministic template this phase
ships (reference, verse text, translation, predictable word-wrapped
layout) - proving `PresentationContent -> Renderer -> Preview`, not final
visual/typographic design, which remains future work. `TEXT_DEFAULT`
exists only so `render_content` is total over `PresentationContent::Text`
too; no design work went into it.

The `Renderer` trait and `NullRenderer` from earlier phases are
unchanged - `render_content` is a separate, lower-level pure function
(content -> structured slide) that a real `Renderer` implementation would
call internally once a real display backend exists.

## Workflow: suggestion to prepared output

```
DETECTED -> SUGGESTED (pending) -> [operator: approve/edit/reject]
                                          |
                                     APPROVED
                                          |
                              [operator: prepare]
                                          |
                                     PREPARED
                                          |
                        (future work: an explicit display action)
                                          |
                                    DISPLAYING
```

**Preview and Prepare are separate actions**, both callable from the
frontend's Preview/Prepare buttons:

- **Preview** (`preview_presentation`/`preview_scripture`) is
  non-mutating: it renders and returns a `PresentationPreview` (content +
  `RenderedSlide`) without touching the database. It is available on a
  suggestion in any status except `Rejected` - deliberately available
  *before* approval, since seeing what a suggestion would look like is
  exactly what helps an operator decide whether to approve it. This also
  fixes a pre-1.4 bug: the operator's "Preview" button previously called
  the approval-gated `prepare_presentation` directly, which threw on any
  still-pending suggestion.
- **Prepare** (`prepare_presentation`) persists a `presentation_items`
  row and is strictly gated on `SuggestionStatus::Approved`
  (`presentation::ensure_suggestion_approved`) - a suggestion that is
  `Pending`, `Edited`, or `Rejected` cannot be prepared. An edited
  suggestion's *edited* reference is what gets prepared (the suggestion
  row itself already carries the edited `SuggestionKind` by the time
  `prepare_presentation` reads it - there is no separate "original vs.
  edited" branch to get wrong). An `Ambiguous` detection never reaches
  this path at all until the operator has explicitly resolved it into a
  concrete suggestion (Phase 1.3's `resolve_ambiguous_reference`).

**Manual creation** (`create_manual_presentation`) skips suggestions
entirely: given a reference string and an active service, it looks up and
prepares content exactly the way `prepare_presentation` does, with
`source_suggestion_id` left `None`. This is the fallback that keeps
presentation preparation working with no audio, no speech engine, and no
network - see "Offline and failure-fallback behavior" below.

## No automatic preparation, no automatic projection

Two boundaries this phase explicitly proves, not just documents:

1. **A detected/suggested Scripture never automatically becomes a
   prepared presentation item.** `pipeline.rs`'s
   `handle_final_transcript` (the detection pipeline) never calls
   anything in `presentation.rs` - the two are connected only through the
   `ai_suggestions` table, and only an explicit `prepare_presentation` or
   `create_manual_presentation` call ever inserts a `presentation_items`
   row. Proven directly by
   `pipeline::tests::phase_1_4_presentation_foundation_acceptance`, which
   asserts the `presentation_items` table is still empty immediately
   after a suggestion is created (and still empty after preview, which is
   non-mutating).
2. **Nothing but an explicit operator Display action ever sets a
   `PresentationItem` to `Active`.** `persist_prepared_item` always
   inserts `Prepared`, and `cancel_item` only ever transitions
   `Prepared -> Stopped` - neither path can reach `Active`. The one path
   that can, `presentation::commit_activation`, is called from exactly one
   place: the `display_presentation` Tauri command, itself only reachable
   from the operator's own Display button (see "Local display foundation"
   below). No finding acceptance, no candidate promotion, no preview, and
   no automatic pipeline step calls it. The acceptance test asserts this
   explicitly after every step, and the restart-recovery test (below)
   asserts a prepared item is still exactly `Prepared` after reopening the
   database - a restart can never advance it either, and a stale `Active`
   row left by an unclean shutdown is swept back to `Stopped` before
   anything else runs (see "Local display foundation" > "Restart
   safety").

Phase 2.1's Music Intelligence findings are held to the identical
standard: `apps/desktop/src-tauri/src/music.rs` and
`cip_core_intelligence::FindingQueue` have no dependency on
`cip_core_presentation` at all, so accepting or rejecting a music
finding is structurally incapable of creating, activating, or otherwise
touching a `PresentationItem` - see
[`docs/music-intelligence.md`](music-intelligence.md#findings-and-the-operator-workflow).

The cross-domain correlation engine's `IntelligenceCorrelation`s (built
under an earlier internal "Phase 2.4" label - see the roadmap note below)
are held to the same standard: `apps/desktop/src-tauri/src/cross_domain.rs`
and `cip_core_intelligence::CorrelationQueue` have no dependency on
`cip_core_presentation` at all, so running a cross-domain analysis, or
reviewing/dismissing a correlation, is structurally incapable of creating,
activating, or otherwise touching a `PresentationItem` - see
[`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md).
Service Intelligence's findings (this repository's authoritative Phase
2.4 - see [`docs/service-intelligence.md`](service-intelligence.md)) are
held to the identical standard: `apps/desktop/src-tauri/src/service.rs`
and `FindingQueue` have no dependency on `cip_core_presentation` either,
so mark/correct/acknowledge actions can never create or touch a
`PresentationItem`.

Sermon Foundation (this repository's authoritative Phase 2.5 - see
[`docs/sermon-foundation.md`](sermon-foundation.md)) is held to the same standard:
`apps/desktop/src-tauri/src/sermon_foundation.rs` and its Tauri command handlers in
`commands.rs` have no dependency on `cip_core_presentation` at all, so no start/pause/resume/
end/title/speaker/section/segment-link action can create, activate, or otherwise touch a
`PresentationItem`.

> **Roadmap note.** Under this repository's authoritative Phase 2 roadmap,
> the cross-domain correlation engine referenced above was extended as the
> roadmap's actual Phase 2.8; the roadmap's actual Phase 2.4 is Service
> Intelligence. The "Phase 2.4" label on the correlation engine's original
> build is a historical artifact from before this roadmap was adopted and
> is not rewritten.
Phase 2.2's acoustic-sourced findings are the exact same
`IntelligenceFinding`/`FindingQueue` types, so this guarantee extends to
them automatically - accepting an acoustic finding only ever sets
`AppState.current_song`, never a `PresentationItem`; see
[`docs/acoustic-music.md`](acoustic-music.md#finding-lifecycle---unchanged).
Phase 2.3's Sermon findings hold to the identical standard:
`apps/desktop/src-tauri/src/sermon.rs` has no dependency on
`cip_core_presentation` at all, so accepting or rejecting a sermon
finding (a main point, a theme, a key statement, ...) is structurally
incapable of creating, activating, or otherwise touching a
`PresentationItem` - see
[`docs/sermon-intelligence.md`](sermon-intelligence.md#operator-workflow-tauri-commands).

## Offline and failure-fallback behavior

`presentation.rs` and `presentation/renderer` depend on nothing but
`cip-core-bible`, `cip-core-presentation`, `serde`, `thiserror`, and
`rusqlite` (transitively, through the app) - no HTTP client anywhere in
the dependency graph, the same structural offline guarantee Phase 1.2
established for the detection pipeline (see `docs/live-speech.md`). The
canonical acceptance test's second half repeats the Romans 8:28 lookup
through `create_manual_presentation`'s path (no suggestion, no speech
engine) and asserts it produces byte-identical content to the
suggestion-based path.

Because `preview_scripture`/`create_manual_presentation` never depend on
`SpeechEngine` or `AudioEngine`, a speech engine failure or an audio
device failure (both handled by Phase 1.3's failure recovery) never
blocks presentation preparation - the operator can always fall back to
Manual Bible Search, preview a result, and prepare it.

## Web runtime (Phase 1.2.1) behavior, unchanged

Every new frontend command wrapper (`previewPresentation`,
`previewScripture`, `preparePresentation`, `createManualPresentation`,
`listPreparedPresentations`, `getPresentationItem`,
`cancelPresentation`) goes through the same `invokeCommand()` guard as
every earlier command - outside the Tauri runtime, each rejects with
`TauriUnavailableError` naming itself (e.g. `"preview_scripture" requires
the CIP desktop application...`) rather than throwing a raw `TypeError`.
No fake web presentation backend was introduced.

## Database

`database/migrations/0004_presentation_traceability.sql` (additive only,
matching every earlier migration's discipline) adds two nullable columns
to the existing `presentation_items` table:

- `source_suggestion_id TEXT REFERENCES ai_suggestions(id) ON DELETE SET NULL`
- `template TEXT`

plus an index on `source_suggestion_id`. No new table was created -
`presentation_items` already existed and already had everything else this
phase needed (`service_id`, `content_type`, `content`, `status`,
`created_at`).

## Service timeline integration

Presentation operations do not have a separate timeline - `prepare_presentation`,
`create_manual_presentation`, and `cancel_presentation` all call the same
`record_timeline()` helper Phase 1.3 introduced, writing into the
existing `audit_events` table via `PRESENTATION_PREPARED`/
`PRESENTATION_CANCELLED` events. `preview_*` intentionally does **not**
write a timeline entry - it's non-mutating and would otherwise clutter
the operational history with every hover-preview.

## Tauri commands

| Command                          | Mutates? | Requires                          |
| --------------------------------- | -------- | ----------------------------------- |
| `preview_presentation(suggestionId)` | No       | suggestion exists, status != rejected |
| `preview_scripture(reference)`       | No       | -                                  |
| `prepare_presentation(suggestionId)` | Yes      | suggestion `Approved`, real verse   |
| `create_manual_presentation(reference)` | Yes   | active service, real verse          |
| `list_prepared_presentations()`      | No       | active service                     |
| `get_presentation_item(itemId)`      | No       | item exists                        |
| `cancel_presentation(itemId)`        | Yes      | item currently `Prepared`           |
| `open_presentation_display()`        | No (window only) | -                           |
| `close_presentation_display()`       | Yes (reconciles any `Active` item) | - |
| `get_presentation_display_state()`   | No       | -                                   |
| `display_presentation(itemId)`       | Yes      | item currently `Prepared`, no other item `Active`, real window open succeeds |
| `clear_presentation_display()`       | Yes      | -                                   |

Every command validates its own input the same way every earlier
command does (empty strings, malformed ids, invalid state transitions)
and returns `AppError` on failure - none expose internal Rust types the
frontend doesn't need. The five display commands are documented in full
in "Local display foundation" above.

## Events

Two new `AppEvent` variants added in Phase 1.4: `PresentationPreviewed`
(emitted by both preview commands) and `PresentationCancelled` (emitted
by `cancel_presentation`). `PresentationPrepared` already existed and is
still emitted by both `prepare_presentation` and
`create_manual_presentation`. `PresentationStarted`/`PresentationStopped`
were declared in an earlier phase but left unused until the "Local
display foundation" addition: `PresentationStarted` (item + rendered
slide) is now emitted by `display_presentation` immediately after a real
display-window operation succeeds and the `Active` transition commits;
`PresentationStopped` (item) is emitted by `commands::clear_active_presentation`,
covering explicit Stop, explicit Close, and a manual display-window
close alike.

## Frontend workspace

`LiveChurchBrain.tsx` gained:

- A **Preview** button on every pending suggestion card that now
  genuinely previews (via `preview_presentation`) instead of incorrectly
  calling `prepare_presentation` - the pre-1.4 bug this phase fixes.
- A new **"Approved - Ready to Prepare"** panel: an approved suggestion
  moves here (rather than disappearing) so it has somewhere to be
  Previewed and then explicitly Prepared. It leaves this panel only when
  actually prepared (removed via the `PresentationPrepared` event
  handler, matched by `sourceSuggestionId`).
- Preview/Prepare actions on each Manual Bible Search result, so the
  fully-manual path (no suggestion at all) is reachable from the UI.
- A real **Current Output** panel, replacing the old static "Nothing
  projected" text: lists every currently-`Prepared` item for the active
  service with a `● PREPARED (automatic|manual)` status line, a Cancel
  action, and (since "Local display foundation" above) a **Display**
  button; an active item gets its own card with a **Stop** button; and
  **Open Display**/**Close Display** buttons control the display window
  itself. A `NOTHING PREPARED` message still appears when the list is
  empty.

Presentation preparation and cancellation already appear in the existing
Service Timeline panel (no duplicate timeline was added).

## Testing

- `presentation/renderer`: determinism, empty-field rejection for both
  `Scripture` and `Text` content, word-wrap correctness.
- `apps/desktop/src-tauri/src/presentation.rs`: real-Bible-text lookup,
  preview/prepare content parity, invalid-reference and
  unavailable-translation rejection, the `ensure_suggestion_approved`
  guard, persisted round-trips with/without a source suggestion, and
  cancel (including double-cancel rejection).
- `pipeline.rs`'s `phase_1_4_presentation_foundation_acceptance`: the
  full SERVICE -> TRANSCRIPT -> "Romans 8" -> "verse 28" -> ROM 8:28 ->
  PENDING SUGGESTION -> HUMAN APPROVAL -> PREVIEW -> PREPARE -> real local
  Bible text -> PERSISTED OUTPUT -> SERVICE TIMELINE chain, the explicit
  no-auto-preparation assertion, the no-auto-projection assertion, and
  the offline/manual-path content-parity assertion, all in one
  deterministic test against real SQLite.
- `invalid_scripture_never_produces_a_presentation_item`: an out-of-range
  reference (`ROM 999:999`) is rejected with no presentation item and no
  silent substitution.
- `prepared_presentation_items_survive_a_simulated_restart_and_stay_prepared`:
  a real file-backed SQLite database, closed and reopened, still has the
  prepared item and its timeline entry - and the item is still exactly
  `Prepared`, never advanced toward display by the restart itself.
- Frontend: `domain/contracts.test.ts` (the extended `PresentationItem`
  shape, `RenderedSlide`, `PresentationPreview`), `domain/bible.test.ts`
  (`formatScriptureReference`), `lib/commands.test.ts` and
  `lib/liveEvents.test.ts` (the new command/event wrappers go through the
  same Tauri-runtime guard as every earlier one).

### Local display foundation tests

- `apps/desktop/src-tauri/src/presentation.rs`: `prepare_to_activate`
  rejects a non-`Prepared` item, rejects when another item for the same
  service is already `Active`, and succeeds with a real rendered slide
  for a valid `Prepared` item; `commit_activation` transitions
  `Prepared -> Active` and rejects a non-`Prepared` item;
  `stop_active_item` transitions the active item to `Stopped` and is a
  safe no-op when nothing is active; a full activate-then-stop round trip;
  at-most-one-active-per-service is enforced even across two different
  prepared items.
- `apps/desktop/src-tauri/src/persistence.rs`:
  `update_presentation_item_status_can_activate_a_prepared_item`,
  `reconcile_stale_active_presentation_items_stops_every_active_row_and_leaves_others_untouched`,
  `reconcile_stale_active_presentation_items_is_a_safe_no_op_when_nothing_is_active`.
- `presentation_display.rs` (window lifecycle itself) is deliberately
  **not** unit-tested - it is exercised only by real desktop runtime
  validation under Xvfb, matching this project's established "no
  `tauri::test` harness" convention (window creation, focus, and the
  `Destroyed` event are real OS/Tauri behavior a mock IPC harness cannot
  meaningfully simulate).
- Frontend: `lib/commands.test.ts` (the five new display commands, plus
  the outside-Tauri guard), `lib/liveEvents.test.ts`
  (`onPresentationStarted`/`onPresentationStopped`), `domain/contracts.test.ts`
  (`PresentationDisplayPayload`/`PresentationDisplayState` shapes).

## Limitations

- **Real Whisper acoustic validation** was not re-run for this phase -
  Phase 1.2's documented model-download blocker in this environment is
  unchanged, and this phase adds no new audio/speech code. The
  deterministic transcript-input acceptance test remains authoritative,
  per Phase 1.2's own documented limitation (see
  [`docs/live-speech.md`](live-speech.md)).
- **Real audio hardware validation** was likewise not re-run; no new
  audio code was added in this phase.
- **A local second-window display now exists** (see "Local display
  foundation" above), but **OBS, vMix, NDI, streaming, and multi-output
  remain explicitly out of scope**, as does any real second physical
  monitor/projector - this development environment has none, so desktop
  runtime validation covered real window creation and lifecycle under
  Xvfb (a virtual X server) and real on-disk database state, not an
  actual physical display. See "Desktop runtime verification" in this
  phase's implementation report for exactly what was and wasn't verified;
  it should never be read as claiming physical projector testing that did
  not occur.
- **Only one local translation (KJV)** exists in this environment's seed
  data, so translation-handling behavior is validated against that one
  translation only.
- Visual/typographic design of `SCRIPTURE_DEFAULT` is intentionally
  minimal - proving the render pipeline, not a finished presentation
  designer (explicitly future work, see `README.md`).

## Validating the presentation layer

```sh
cargo test -p cip-presentation-renderer  # renderer determinism/validation
cargo test -p cip-core-presentation      # PresentationItem/with_* builders
cargo test -p cip-desktop --lib presentation::   # presentation.rs unit tests, incl. activate/stop
cargo test -p cip-desktop --lib persistence::    # incl. restart-reconciliation tests
cargo test -p cip-desktop --lib pipeline::       # canonical acceptance + restart + offline
pnpm vitest run                          # frontend domain/command/event tests
```
