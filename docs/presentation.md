# Presentation Foundation & Real-Service Validation (Phase 1.4)

This document explains what Phase 1.4 added on top of Phase 1.3's
operator workflow: a real presentation preparation path from an approved
suggestion (or a manual reference) through to persisted, prepared output,
and the validation that proves the whole pipeline - detection through
presentation - behaves correctly and never bypasses the operator.

**Core principle, unchanged and made explicit by this phase's data model:
APPROVED is not PREPARED, and PREPARED is not DISPLAYING.** Approval means
the operator accepted a suggestion. Preparation means real, local-Bible
content has been rendered and persisted, ready for output. Displaying
means an explicit output/display action has occurred - and nothing in
this codebase can perform that action yet, so nothing here ever reaches
it. See "No automatic preparation, no automatic projection" below.

**Not in this phase:** OBS/vMix integration, projector/window output, any
real "display" action, song/hymn recognition, sermon intelligence,
content generation, cloud sync. See [`README.md`](../README.md) for the
full phase boundary.

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
| DISPLAYING         | `PresentationItemStatus::Active` - **never set by anything in this phase**; reserved for a future real display integration |
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

Only `KJV` is seeded in this development environment
(`database/seeds/dev_seed.sql`), so translation handling is validated
against that one translation; broader multi-translation validation awaits
a fuller Bible dataset - no additional translations were invented for
testing, per the phase's own constraint. `DEFAULT_TRANSLATION_ID`
(`state.rs`) is still `"KJV"` and is applied consistently by every
presentation command; nothing here assumes or silently substitutes a
different translation than the one requested.

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
2. **Nothing ever sets a `PresentationItem` to `Active`.** No code path
   in this codebase writes that status - `persist_prepared_item` always
   inserts `Prepared`, and `cancel_item` only ever transitions
   `Prepared -> Stopped`. The acceptance test asserts this explicitly
   after every step, and the restart-recovery test (below) asserts a
   prepared item is still exactly `Prepared` after reopening the database
   - a restart can never advance it either.

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

> **Roadmap note.** Under this repository's authoritative Phase 2 roadmap,
> the cross-domain correlation engine referenced above is reserved for
> formal validation as Phase 2.8; the roadmap's actual Phase 2.4 is
> Service Intelligence. The "Phase 2.4" label on the correlation engine is
> a historical artifact from before this roadmap was adopted and is not
> rewritten.
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

Every command validates its own input the same way every earlier
command does (empty strings, malformed ids, invalid state transitions)
and returns `AppError` on failure - none expose internal Rust types the
frontend doesn't need.

## Events

Two new `AppEvent` variants: `PresentationPreviewed` (emitted by both
preview commands) and `PresentationCancelled` (emitted by
`cancel_presentation`). `PresentationPrepared` already existed and is
still emitted by both `prepare_presentation` and
`create_manual_presentation`. `PresentationStarted`/`PresentationStopped`
remain declared but **unused** - reserved for a future real display
integration; nothing in this phase emits them, matching the "do not emit
DISPLAYING unless real display output exists" constraint.

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
  service with a `● PREPARED (automatic|manual)` status line and a Cancel
  action, or a `NOTHING PREPARED` message when empty. It never renders
  `PROJECTED`/`DISPLAYING`, since nothing here can produce that state.

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

## Limitations

- **Real Whisper acoustic validation** was not re-run for this phase -
  Phase 1.2's documented model-download blocker in this environment is
  unchanged, and this phase adds no new audio/speech code. The
  deterministic transcript-input acceptance test remains authoritative,
  per Phase 1.2's own documented limitation (see
  [`docs/live-speech.md`](live-speech.md)).
- **Real audio hardware validation** was likewise not re-run; no new
  audio code was added in this phase.
- **No real display/output integration exists** - OBS, vMix, a
  projector, or any actual "put pixels on a second screen" mechanism is
  explicitly out of scope. `PresentationItemStatus::Active` and the
  `PRESENTATION_STARTED`/`PRESENTATION_STOPPED` events exist as reserved
  architecture, not as implemented behavior.
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
cargo test -p cip-desktop --lib presentation::   # presentation.rs unit tests
cargo test -p cip-desktop --lib pipeline::       # canonical acceptance + restart + offline
pnpm vitest run                          # frontend domain/command/event tests
```
