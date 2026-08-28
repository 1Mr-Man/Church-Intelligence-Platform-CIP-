//! Presentation preparation: builds real, `BibleProvider`-sourced content
//! for a presentation item and renders it deterministically, shared by the
//! preview (no persistence) and prepare (persists) command paths so both
//! always produce identical content for the same input - Phase 1.4 section
//! 37's content-integrity requirement ("text must come from BibleProvider,
//! never AI-generated, paraphrased, or substituted").
//!
//! Deliberately Tauri-agnostic (plain `&Connection`/`&dyn BibleProvider` +
//! domain types, no `AppHandle`/`State`), mirroring `pipeline.rs` - see its
//! docs for why (no `tauri::test` harness in this project; command *logic*
//! is kept independently unit-testable here, the command function itself
//! stays a thin wrapper).
//!
//! ## PREPARED vs DISPLAYING (Phase 1.4 section 3, extended by the local
//! presentation display foundation)
//!
//! Preparation (this module's original job) only ever produces items in
//! `PresentationItemStatus::Prepared` (or, via [`cancel_item`], `Stopped`).
//! APPROVED (a suggestion) is not the same as PREPARED (a presentation
//! item): [`ensure_suggestion_approved`] is the only bridge between the
//! two, and it is never bypassed.
//!
//! The local presentation display foundation adds the second, later half
//! of the lifecycle: [`prepare_to_activate`]/[`commit_activation`] (the
//! `Prepared -> Active` transition, committed only after the real display
//! window operation in `apps/desktop/src-tauri/src/commands.rs` has
//! already succeeded - never before) and [`stop_active_item`] (`Active ->
//! Stopped`). Nothing in this module ever opens a window or touches Tauri;
//! that stays in `commands.rs`/`presentation_display.rs`, keeping this
//! module exactly as Tauri-agnostic and independently testable as it
//! always was. See `docs/presentation.md`'s "Local display architecture"
//! section.

use cip_core_ai::SuggestionStatus;
use cip_core_bible::{get_verse_range, BibleProvider, BibleProviderError, ScriptureReference};
use cip_core_presentation::{PresentationContent, PresentationItem, PresentationItemStatus};
use cip_presentation_renderer::{render_content, RenderError, RenderedSlide};
use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use crate::persistence::{self, PersistError};

#[derive(Debug, Error)]
pub enum PresentationError {
    #[error("not a recognized scripture reference: {0}")]
    InvalidReference(String),
    #[error("verse not found: {0}")]
    VerseNotFound(String),
    #[error(transparent)]
    BibleProvider(#[from] BibleProviderError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Persistence(#[from] PersistError),
    #[error("only an approved suggestion can be prepared for presentation")]
    SuggestionNotApproved,
    #[error("suggestion is not a scripture reference")]
    SuggestionNotScripture,
    #[error("presentation item {0} is not prepared (currently {1:?}) and cannot be cancelled")]
    NotCancelable(Uuid, PresentationItemStatus),
    /// Only a `Prepared` item can be displayed - an already-`Active` or
    /// already-`Stopped` item cannot be re-displayed by this path (the
    /// operator would prepare a fresh item instead).
    #[error("presentation item {0} is not prepared (currently {1:?}) and cannot be displayed")]
    NotDisplayable(Uuid, PresentationItemStatus),
    /// At most one presentation item may be `Active` at a time (spec
    /// section 10) - the operator must explicitly stop the item named here
    /// before displaying another.
    #[error("presentation item {0} is already active; stop it before displaying another")]
    AlreadyActive(Uuid),
    /// The real local display window could not be opened/updated - never
    /// a reason to claim `Active` anyway (spec section 8).
    #[error("presentation display window unavailable: {0}")]
    DisplayUnavailable(String),
}

/// `"ROM 8:28"` -> `("ROM", 8, 28, None)`; `"ROM 8:28-31"` -> `("ROM", 8,
/// 28, Some(31))`. Reverses `ScriptureReference`'s own `Display` impl -
/// the same parse `commands::parse_display_reference` performs, kept as a
/// separate copy here (rather than a shared import) so this module has no
/// dependency on `commands.rs`, matching `pipeline.rs`'s existing
/// independence from it.
///
/// Phase 3.6: previously silently discarded everything after a `-` (only
/// the range's start verse was ever prepared/displayed - see
/// `docs/phase-3-6-church-libraries.md`'s Bible Library audit finding).
/// Now returns the end verse too so [`build_scripture_slide`] can render
/// the full range, matching what Bible Library "browse a range, prepare
/// it" actually needs.
fn parse_display_reference(
    text: &str,
) -> Result<(String, u32, u32, Option<u32>), PresentationError> {
    let invalid = || PresentationError::InvalidReference(text.to_string());
    let (book, rest) = text.rsplit_once(' ').ok_or_else(invalid)?;
    let (chapter_str, verse_str) = rest.split_once(':').ok_or_else(invalid)?;
    let chapter: u32 = chapter_str.parse().map_err(|_| invalid())?;
    let (verse, verse_end) = match verse_str.split_once('-') {
        Some((start, end)) => (
            start.parse().map_err(|_| invalid())?,
            Some(end.parse().map_err(|_| invalid())?),
        ),
        None => (verse_str.parse().map_err(|_| invalid())?, None),
    };
    Ok((book.to_string(), chapter, verse, verse_end))
}

/// Looks up a scripture reference against the real local `BibleProvider`
/// and deterministically renders it. Used identically by preview (which
/// discards the result after returning it) and prepare (which additionally
/// persists it) - see this module's docs.
///
/// Never uses AI-generated or web-sourced text (section 6/37): the verse
/// text is exactly what `BibleProvider::get_verse` returns, nothing else.
pub fn build_scripture_slide(
    provider: &dyn BibleProvider,
    translation_id: &str,
    reference_display: &str,
) -> Result<(PresentationContent, RenderedSlide), PresentationError> {
    let (book, chapter, verse, verse_end) = parse_display_reference(reference_display)?;

    let text = match verse_end {
        None => {
            let scripture_reference =
                ScriptureReference::single(translation_id, &book, chapter, verse);
            provider
                .get_verse(&scripture_reference)?
                .ok_or_else(|| PresentationError::VerseNotFound(reference_display.to_string()))?
                .text
        }
        Some(verse_end) => {
            let verses =
                get_verse_range(provider, translation_id, &book, chapter, verse, verse_end)
                    .map_err(|_| PresentationError::VerseNotFound(reference_display.to_string()))?;
            if verses.is_empty() {
                return Err(PresentationError::VerseNotFound(
                    reference_display.to_string(),
                ));
            }
            verses
                .into_iter()
                .map(|v| format!("{} {}", v.reference.verse_start, v.text))
                .collect::<Vec<_>>()
                .join(" ")
        }
    };

    let content = PresentationContent::Scripture {
        reference: reference_display.to_string(),
        translation_id: translation_id.to_string(),
        text,
    };
    let slide = render_content(&content)?;
    Ok((content, slide))
}

/// A suggestion must be `Approved` before it can be prepared for
/// presentation (Phase 1.4 sections 3/4/16) - a detected reference never
/// bypasses human approval on its way to presentation, regardless of
/// confidence.
pub fn ensure_suggestion_approved(status: SuggestionStatus) -> Result<(), PresentationError> {
    if status != SuggestionStatus::Approved {
        return Err(PresentationError::SuggestionNotApproved);
    }
    Ok(())
}

/// Persists a `Prepared` presentation item built from `content`, recording
/// which template rendered it and (when present) which suggestion it came
/// from - the automatic-detection-path vs. manual-search-path distinction
/// Phase 1.4 section 16 requires stay visible in the record. Does not emit
/// events or write to the timeline; those are the caller's responsibility
/// (see `commands.rs`), matching `pipeline.rs`'s persistence/side-effect
/// split.
pub fn persist_prepared_item(
    conn: &Connection,
    service_id: Uuid,
    content: PresentationContent,
    template: &str,
    source_suggestion_id: Option<Uuid>,
) -> Result<PresentationItem, PresentationError> {
    let mut item = PresentationItem::prepare(service_id, content).with_template(template);
    if let Some(suggestion_id) = source_suggestion_id {
        item = item.with_source_suggestion(suggestion_id);
    }
    persistence::persist_presentation_item(conn, &item)?;
    Ok(item)
}

/// Cancels ("retracts") a still-`Prepared` item, reusing the existing
/// `Stopped` status (section 3: "use actual existing project naming
/// conventions... do not invent states the architecture cannot support").
/// Unambiguous in practice because nothing in this phase ever transitions
/// an item to `Active` first.
pub fn cancel_item(
    conn: &Connection,
    item_id: Uuid,
) -> Result<PresentationItem, PresentationError> {
    let current = persistence::get_presentation_item(conn, item_id)?;
    if current.status != PresentationItemStatus::Prepared {
        return Err(PresentationError::NotCancelable(item_id, current.status));
    }
    Ok(persistence::update_presentation_item_status(
        conn,
        item_id,
        PresentationItemStatus::Stopped,
    )?)
}

/// Validates and renders a `Prepared` item for real local display - never
/// persists anything (spec section 8: never mark `Active` before the real
/// display operation has succeeded). The caller (`commands::display_presentation`)
/// must actually open/update the display window using the returned
/// `RenderedSlide` before calling [`commit_activation`]; if it can't, this
/// item must stay `Prepared`.
///
/// Rejects with [`PresentationError::AlreadyActive`] when another item for
/// the same service is already `Active` (spec section 10: "at most one
/// active presentation item at a time") - the operator must stop it first
/// rather than this silently replacing it.
pub fn prepare_to_activate(
    conn: &Connection,
    item_id: Uuid,
) -> Result<(PresentationItem, RenderedSlide), PresentationError> {
    let current = persistence::get_presentation_item(conn, item_id)?;
    if current.status != PresentationItemStatus::Prepared {
        return Err(PresentationError::NotDisplayable(item_id, current.status));
    }
    let already_active = persistence::list_presentation_items(
        conn,
        current.service_id,
        Some(PresentationItemStatus::Active),
    )?;
    if let Some(existing) = already_active.into_iter().next() {
        return Err(PresentationError::AlreadyActive(existing.id));
    }
    let slide = render_content(&current.content)?;
    Ok((current, slide))
}

/// Commits the `Prepared -> Active` transition - call only after the real
/// display window operation has already succeeded (see [`prepare_to_activate`]'s
/// docs). Re-validates the item is still `Prepared` rather than trusting
/// time has stood still since the earlier call.
pub fn commit_activation(
    conn: &Connection,
    item_id: Uuid,
) -> Result<PresentationItem, PresentationError> {
    let current = persistence::get_presentation_item(conn, item_id)?;
    if current.status != PresentationItemStatus::Prepared {
        return Err(PresentationError::NotDisplayable(item_id, current.status));
    }
    Ok(persistence::update_presentation_item_status(
        conn,
        item_id,
        PresentationItemStatus::Active,
    )?)
}

/// Stops whichever presentation item is currently `Active` for `service_id`,
/// if any - safe and idempotent when nothing is active (spec section 9:
/// "operation should be safe and idempotent... do not crash"), returning
/// `Ok(None)` rather than an error in that case. Used both by the explicit
/// operator Stop/Clear-Display action and by the display window's own
/// manual-close reconciliation (`commands::clear_active_presentation`), so
/// both paths leave persistence in the exact same state.
pub fn stop_active_item(
    conn: &Connection,
    service_id: Uuid,
) -> Result<Option<PresentationItem>, PresentationError> {
    let active = persistence::list_presentation_items(
        conn,
        service_id,
        Some(PresentationItemStatus::Active),
    )?;
    let Some(item) = active.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(persistence::update_presentation_item_status(
        conn,
        item.id,
        PresentationItemStatus::Stopped,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_ai::{Suggestion, SuggestionKind};
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations, seed::apply_dev_seed};
    use cip_integrations_bible::SqliteBibleProvider;

    fn seeded_provider() -> SqliteBibleProvider {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        SqliteBibleProvider::new(conn)
    }

    fn migrated_conn() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        conn
    }

    #[test]
    fn builds_real_bible_text_for_a_known_reference() {
        let provider = seeded_provider();
        let (content, slide) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();

        let PresentationContent::Scripture {
            reference,
            translation_id,
            text,
        } = content
        else {
            panic!("expected scripture content");
        };
        assert_eq!(reference, "ROM 8:28");
        assert_eq!(translation_id, "KJV");
        assert!(text.contains("all things work together for good"));
        assert_eq!(
            slide.template,
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE
        );
        assert_eq!(slide.heading, "ROM 8:28");
    }

    #[test]
    fn preview_and_prepare_paths_produce_identical_content_for_the_same_reference() {
        let provider = seeded_provider();
        let (content_a, slide_a) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        let (content_b, slide_b) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        assert_eq!(content_a, content_b);
        assert_eq!(slide_a, slide_b);
    }

    #[test]
    fn builds_real_bible_text_for_a_verse_range() {
        // Phase 3.6: a range must include every verse's text, not just the
        // first one - see docs/phase-3-6-church-libraries.md's Bible
        // Library audit finding (the range used to be silently truncated).
        let provider = seeded_provider();
        let (content, slide) = build_scripture_slide(&provider, "KJV", "ROM 8:29-30").unwrap();

        let PresentationContent::Scripture {
            reference, text, ..
        } = content
        else {
            panic!("expected scripture content");
        };
        assert_eq!(reference, "ROM 8:29-30");
        assert!(text.contains("foreknow"), "must include verse 29's text");
        assert!(text.contains("justified"), "must include verse 30's text");
        assert_eq!(slide.heading, "ROM 8:29-30");
    }

    #[test]
    fn rejects_an_inverted_verse_range() {
        let provider = seeded_provider();
        let err = build_scripture_slide(&provider, "KJV", "ROM 8:31-29").unwrap_err();
        assert!(matches!(err, PresentationError::VerseNotFound(_)));
    }

    #[test]
    fn rejects_a_reference_not_in_the_local_database() {
        let provider = seeded_provider();
        let err = build_scripture_slide(&provider, "KJV", "ROM 999:999").unwrap_err();
        assert!(matches!(err, PresentationError::VerseNotFound(_)));
    }

    #[test]
    fn rejects_an_unavailable_translation_rather_than_substituting_one() {
        // The local database has no NIV rows at all (only KJV is seeded) -
        // this must report clearly rather than silently falling back to a
        // different translation (section 6/7).
        let provider = seeded_provider();
        let err = build_scripture_slide(&provider, "NIV", "ROM 8:28").unwrap_err();
        assert!(matches!(err, PresentationError::VerseNotFound(_)));
    }

    #[test]
    fn rejects_malformed_reference_text() {
        let provider = seeded_provider();
        assert!(matches!(
            build_scripture_slide(&provider, "KJV", "garbage").unwrap_err(),
            PresentationError::InvalidReference(_)
        ));
    }

    #[test]
    fn ensure_suggestion_approved_rejects_anything_but_approved() {
        assert!(ensure_suggestion_approved(SuggestionStatus::Approved).is_ok());
        for status in [
            SuggestionStatus::Pending,
            SuggestionStatus::Edited,
            SuggestionStatus::Rejected,
        ] {
            assert!(matches!(
                ensure_suggestion_approved(status).unwrap_err(),
                PresentationError::SuggestionNotApproved
            ));
        }
    }

    #[test]
    fn persist_prepared_item_records_template_and_source_suggestion() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Presentation Test");
        persistence::persist_service(&conn, &session).unwrap();
        let suggestion = Suggestion::new(
            session.id,
            SuggestionKind::Scripture {
                reference: "ROM 8:28".into(),
            },
            ConfidenceResult::new(0.95, ConfidenceSource::Heuristic, None),
        );
        persistence::persist_suggestion(&conn, &suggestion).unwrap();

        let provider = seeded_provider();
        let (content, _) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        let item = persist_prepared_item(
            &conn,
            session.id,
            content,
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE,
            Some(suggestion.id),
        )
        .unwrap();

        assert_eq!(item.status, PresentationItemStatus::Prepared);
        assert_eq!(item.source_suggestion_id, Some(suggestion.id));
        assert_eq!(
            item.template.as_deref(),
            Some(cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE)
        );

        let reloaded = persistence::get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(reloaded, item);
    }

    #[test]
    fn persist_prepared_item_without_a_source_suggestion_is_a_manual_item() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Manual Presentation Test");
        persistence::persist_service(&conn, &session).unwrap();

        let provider = seeded_provider();
        let (content, _) = build_scripture_slide(&provider, "KJV", "JHN 3:16").unwrap();
        let item = persist_prepared_item(
            &conn,
            session.id,
            content,
            cip_presentation_renderer::SCRIPTURE_DEFAULT_TEMPLATE,
            None,
        )
        .unwrap();

        assert_eq!(item.source_suggestion_id, None);
    }

    #[test]
    fn cancel_item_stops_a_prepared_item() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Cancel Test");
        persistence::persist_service(&conn, &session).unwrap();
        let provider = seeded_provider();
        let (content, _) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        let item =
            persist_prepared_item(&conn, session.id, content, "SCRIPTURE_DEFAULT", None).unwrap();

        let cancelled = cancel_item(&conn, item.id).unwrap();
        assert_eq!(cancelled.status, PresentationItemStatus::Stopped);
    }

    #[test]
    fn cancel_item_rejects_an_already_cancelled_item() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Double Cancel Test");
        persistence::persist_service(&conn, &session).unwrap();
        let provider = seeded_provider();
        let (content, _) = build_scripture_slide(&provider, "KJV", "ROM 8:28").unwrap();
        let item =
            persist_prepared_item(&conn, session.id, content, "SCRIPTURE_DEFAULT", None).unwrap();
        cancel_item(&conn, item.id).unwrap();

        let err = cancel_item(&conn, item.id).unwrap_err();
        assert!(matches!(
            err,
            PresentationError::NotCancelable(_, PresentationItemStatus::Stopped)
        ));
    }

    fn prepared_item(conn: &Connection, session_id: Uuid, body: &str) -> PresentationItem {
        let item = PresentationItem::prepare(
            session_id,
            PresentationContent::Text {
                title: None,
                body: body.to_string(),
            },
        );
        persistence::persist_presentation_item(conn, &item).unwrap();
        item
    }

    #[test]
    fn prepare_to_activate_renders_a_prepared_item_without_persisting_anything() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Activate Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "Welcome to service");

        let (loaded, slide) = prepare_to_activate(&conn, item.id).unwrap();
        assert_eq!(loaded.id, item.id);
        assert_eq!(loaded.status, PresentationItemStatus::Prepared);
        assert_eq!(slide.body_lines, vec!["Welcome to service".to_string()]);

        // Still Prepared - prepare_to_activate never mutates persistence.
        let reloaded = persistence::get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(reloaded.status, PresentationItemStatus::Prepared);
    }

    #[test]
    fn prepare_to_activate_rejects_an_item_that_is_not_prepared() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Activate Reject Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "hello");
        cancel_item(&conn, item.id).unwrap();

        let err = prepare_to_activate(&conn, item.id).unwrap_err();
        assert!(matches!(
            err,
            PresentationError::NotDisplayable(_, PresentationItemStatus::Stopped)
        ));
    }

    #[test]
    fn prepare_to_activate_rejects_a_second_item_while_one_is_already_active() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Only One Active Test");
        persistence::persist_service(&conn, &session).unwrap();
        let first = prepared_item(&conn, session.id, "first");
        let second = prepared_item(&conn, session.id, "second");

        commit_activation(&conn, first.id).unwrap();

        let err = prepare_to_activate(&conn, second.id).unwrap_err();
        assert!(matches!(err, PresentationError::AlreadyActive(id) if id == first.id));
    }

    #[test]
    fn commit_activation_transitions_prepared_to_active() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Commit Activation Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "hello");

        let activated = commit_activation(&conn, item.id).unwrap();
        assert_eq!(activated.status, PresentationItemStatus::Active);
        let reloaded = persistence::get_presentation_item(&conn, item.id).unwrap();
        assert_eq!(reloaded.status, PresentationItemStatus::Active);
    }

    #[test]
    fn commit_activation_rejects_an_item_that_is_no_longer_prepared() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Commit Activation Race Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "hello");
        commit_activation(&conn, item.id).unwrap();

        // Simulates two display commands racing: the second call's earlier
        // prepare_to_activate check is now stale.
        let err = commit_activation(&conn, item.id).unwrap_err();
        assert!(matches!(
            err,
            PresentationError::NotDisplayable(_, PresentationItemStatus::Active)
        ));
    }

    #[test]
    fn stop_active_item_transitions_active_to_stopped() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Stop Active Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "hello");
        commit_activation(&conn, item.id).unwrap();

        let stopped = stop_active_item(&conn, session.id).unwrap();
        assert_eq!(
            stopped.map(|i| i.status),
            Some(PresentationItemStatus::Stopped)
        );
    }

    #[test]
    fn stop_active_item_is_a_safe_no_op_when_nothing_is_active() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Stop Nothing Active Test");
        persistence::persist_service(&conn, &session).unwrap();
        let _prepared_but_not_active = prepared_item(&conn, session.id, "hello");

        assert_eq!(stop_active_item(&conn, session.id).unwrap(), None);
    }

    #[test]
    fn activate_then_stop_leaves_exactly_one_historical_stopped_row_never_two_active() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Full Cycle Test");
        persistence::persist_service(&conn, &session).unwrap();
        let item = prepared_item(&conn, session.id, "hello");

        commit_activation(&conn, item.id).unwrap();
        stop_active_item(&conn, session.id).unwrap();

        let all = persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, PresentationItemStatus::Stopped);

        let still_active = persistence::list_presentation_items(
            &conn,
            session.id,
            Some(PresentationItemStatus::Active),
        )
        .unwrap();
        assert!(still_active.is_empty());
    }

    /// Phase 3.8.2 - directly encodes the spec's "Display Window Reopen
    /// Test": Display, Stop, Close, Reopen, Display another, repeated
    /// three times, with no manual intervention between cycles. This is
    /// the invariant `commands::close_presentation_display`'s new
    /// synchronous-reconciliation fix depends on: `stop_active_item` must
    /// always leave the way clear for the very next `prepare_to_activate`
    /// to succeed, with no leftover `Active` row - the exact race that
    /// used to be possible when reconciliation only happened
    /// asynchronously via the display window's `Destroyed` event. Cannot
    /// exercise the Tauri command layer itself (no `tauri::test` harness
    /// in this project - see this module's own docs), so this proves the
    /// invariant at the one layer this project's tests can reach.
    #[test]
    fn three_display_stop_close_reopen_cycles_never_leave_a_stale_active_item() {
        let conn = migrated_conn();
        let session = ServiceSession::start("Reopen Cycle Test");
        persistence::persist_service(&conn, &session).unwrap();

        for cycle in 1..=3 {
            let item = prepared_item(&conn, session.id, &format!("slide {cycle}"));

            // Display.
            let (_, _slide) = prepare_to_activate(&conn, item.id).unwrap();
            commit_activation(&conn, item.id).unwrap();

            // Stop + Close, synchronously reconciled - mirrors
            // `close_presentation_display`'s new call order.
            let stopped = stop_active_item(&conn, session.id).unwrap();
            assert_eq!(
                stopped.map(|i| i.id),
                Some(item.id),
                "cycle {cycle}: stop must resolve the item just displayed"
            );

            // Reopen: nothing must be left Active for the *next* cycle's
            // prepare_to_activate to trip over.
            let still_active = persistence::list_presentation_items(
                &conn,
                session.id,
                Some(PresentationItemStatus::Active),
            )
            .unwrap();
            assert!(
                still_active.is_empty(),
                "cycle {cycle}: no stale Active item may remain after Stop + Close"
            );
        }

        // Three cycles, three distinct historical Stopped rows - never
        // fewer (a dropped cycle) and never an orphaned Active row.
        let all = persistence::list_presentation_items(&conn, session.id, None).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all
            .iter()
            .all(|i| i.status == PresentationItemStatus::Stopped));
    }
}
