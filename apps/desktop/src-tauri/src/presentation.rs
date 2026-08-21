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
//! ## PREPARED vs DISPLAYING (Phase 1.4 section 3)
//!
//! This module only ever produces items in `PresentationItemStatus::Prepared`
//! (or, via [`cancel_item`], `Stopped`). Nothing here writes `Active` -
//! that's reserved for a future real display/output integration. APPROVED
//! (a suggestion) is not the same as PREPARED (a presentation item):
//! [`ensure_suggestion_approved`] is the only bridge between the two, and
//! it is never bypassed.

use cip_core_ai::SuggestionStatus;
use cip_core_bible::{BibleProvider, BibleProviderError, ScriptureReference};
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
}

/// `"ROM 8:28"` -> `("ROM", 8, 28)`. Reverses `ScriptureReference`'s own
/// `Display` impl - the same parse `commands::parse_display_reference`
/// performs, kept as a separate copy here (rather than a shared import)
/// so this module has no dependency on `commands.rs`, matching
/// `pipeline.rs`'s existing independence from it.
fn parse_display_reference(text: &str) -> Result<(String, u32, u32), PresentationError> {
    let invalid = || PresentationError::InvalidReference(text.to_string());
    let (book, rest) = text.rsplit_once(' ').ok_or_else(invalid)?;
    let (chapter_str, verse_str) = rest.split_once(':').ok_or_else(invalid)?;
    let chapter: u32 = chapter_str.parse().map_err(|_| invalid())?;
    let verse: u32 = verse_str
        .split('-')
        .next()
        .unwrap_or(verse_str)
        .parse()
        .map_err(|_| invalid())?;
    Ok((book.to_string(), chapter, verse))
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
    let (book, chapter, verse) = parse_display_reference(reference_display)?;
    let scripture_reference = ScriptureReference::single(translation_id, &book, chapter, verse);
    let verse_row = provider
        .get_verse(&scripture_reference)?
        .ok_or_else(|| PresentationError::VerseNotFound(reference_display.to_string()))?;

    let content = PresentationContent::Scripture {
        reference: reference_display.to_string(),
        translation_id: translation_id.to_string(),
        text: verse_row.text,
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
}
