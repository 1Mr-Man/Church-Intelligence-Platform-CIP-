//! Phase 2.0 intelligence architecture wiring - Tauri-agnostic (plain
//! functions over domain types, no `AppHandle`/`State`, matching the
//! established `content.rs`/`presentation.rs` orchestration-module
//! pattern) so it's directly unit-testable.
//!
//! This module builds a real [`IntelligenceEngineRegistry`] with the Bible
//! compatibility adapter registered against this app's real
//! `SqliteBibleProvider`, and a helper that assembles a real
//! [`IntelligenceContext`] from this app's actual transcript/service/
//! content-registry state. Nothing here is called from the live transcript
//! pipeline (`pipeline.rs::handle_final_transcript` is unchanged) - the
//! registry and context builder are exercised only by the
//! `get_intelligence_capabilities` diagnostic command and by this module's
//! own tests. See `docs/intelligence-architecture.md`.

use cip_core_bible::{BibleProvider, ScriptureContextManager};
use cip_core_content::ContentRegistry;
use cip_core_intelligence::{
    BibleIntelligenceEngine, ContextBounds, IntelligenceContext, IntelligenceDomain,
    IntelligenceEngineRegistry, IntelligenceFinding, ServiceEventSummary,
};
use cip_core_service::ServiceSession;
use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use crate::timeline::TimelineEntry;

#[derive(Debug, Error)]
pub enum ContextBuildError {
    #[error(transparent)]
    Persistence(#[from] crate::persistence::PersistError),
    #[error(transparent)]
    ContentRegistry(#[from] cip_core_content::ContentRegistryError),
}

/// Build a registry with the real Bible compatibility adapter registered.
/// `Music`/`Sermon`/`Content`/`CrossDomain` are deliberately left
/// unregistered - Phase 2.0 does not implement them (see the crate-level
/// `ABSOLUTE NON-GOALS` in the Phase 2.0 spec), so `registry.resolve(...)`
/// for those domains correctly returns `None` rather than a fake engine.
pub fn build_registry(
    bible_provider: Box<dyn BibleProvider>,
    translation_id: &str,
) -> IntelligenceEngineRegistry {
    let mut registry = IntelligenceEngineRegistry::new();
    let bible_engine = BibleIntelligenceEngine::new(bible_provider, translation_id);
    registry
        .register(Box::new(bible_engine))
        .expect("Bible is the first and only registration for its domain");
    registry
}

/// Every [`IntelligenceDomain`] Phase 2.0 reserves the shape for, in a
/// fixed, stable order - used by the diagnostic command so the frontend
/// always sees every domain, not just the ones with a registered engine.
pub const ALL_DOMAINS: [IntelligenceDomain; 6] = [
    IntelligenceDomain::Bible,
    IntelligenceDomain::Music,
    IntelligenceDomain::Service,
    IntelligenceDomain::Sermon,
    IntelligenceDomain::Content,
    IntelligenceDomain::CrossDomain,
];

/// Assemble a real [`IntelligenceContext`] from this app's actual state -
/// the "boundary" half of Phase 2.0 spec section 25/26/27: proves the
/// shared context can be built from real transcript/service/content-
/// registry data, not just synthetic test fixtures. Exercised by this
/// module's own tests against a real `SqliteBibleProvider`/
/// `SqliteContentRegistry`, and (Phase 2.2) by the acoustic worker
/// (`acoustic.rs`), which is the first real caller to pass a non-empty
/// `recent_findings` - see that parameter's own docs.
///
/// `recent_findings` is an explicit parameter, not read from a database,
/// because Phase 2.0 findings are deliberately in-memory only (see
/// `docs/intelligence-architecture.md`'s persistence-decision section) -
/// a caller with no findings source of its own (like
/// `commands::analyze_music_transcript`, unchanged since Phase 2.1) passes
/// `Vec::new()` and gets exactly the behavior it always has; a caller that
/// does have one (Phase 2.2's acoustic worker, reading
/// `AppState.intelligence_findings`) can now let continuity/fusion see it,
/// without this function reaching into `AppState` itself - it stays
/// Tauri-agnostic like the rest of this module.
pub fn build_intelligence_context(
    conn: &Connection,
    content_registry: &dyn ContentRegistry,
    service: Option<&ServiceSession>,
    context_manager: &dyn ScriptureContextManager,
    recent_timeline: &[TimelineEntry],
    recent_findings: Vec<IntelligenceFinding>,
    bounds: ContextBounds,
) -> Result<IntelligenceContext, ContextBuildError> {
    let service_id = service.map(|s| s.id).unwrap_or_else(Uuid::nil);
    let recent_transcript_segments = match service {
        Some(s) => crate::persistence::list_transcript_segments(
            conn,
            s.id,
            bounds.max_recent_transcript_segments as u32,
        )?,
        None => Vec::new(),
    };
    let current_transcript_segment = recent_transcript_segments.last().cloned();
    let content_metadata = content_registry.list(None)?;
    let recent_service_events = recent_timeline
        .iter()
        .map(|entry| ServiceEventSummary {
            name: entry.event_name.clone(),
            occurred_at: entry.created_at,
        })
        .collect();

    Ok(IntelligenceContext::build(
        service_id,
        service.map(|s| s.status),
        current_transcript_segment,
        recent_transcript_segments,
        context_manager.active_context(),
        recent_findings,
        recent_service_events,
        content_metadata,
        bounds,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_bible::DefaultScriptureContextManager;
    use cip_database::{open_in_memory, run_migrations, seed::apply_dev_seed};
    use cip_integrations_bible::SqliteBibleProvider;
    use cip_integrations_content::SqliteContentRegistry;

    fn seeded_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        apply_dev_seed(&conn).unwrap();
        conn
    }

    #[test]
    fn registry_has_a_real_bible_engine_and_nothing_else() {
        let conn = seeded_db();
        let provider = Box::new(SqliteBibleProvider::new(conn));
        let registry = build_registry(provider, "KJV");

        assert!(registry.resolve(IntelligenceDomain::Bible).is_some());
        assert!(registry.resolve(IntelligenceDomain::Music).is_none());
        assert!(registry.resolve(IntelligenceDomain::Sermon).is_none());
        assert!(registry.resolve(IntelligenceDomain::Content).is_none());
    }

    #[test]
    fn context_builds_from_real_app_state_pieces_with_no_active_service() {
        let conn = seeded_db();
        let content_conn = open_in_memory().unwrap();
        let mut content_conn = content_conn;
        run_migrations(&mut content_conn).unwrap();
        let registry = SqliteContentRegistry::new(content_conn);
        let context_manager = DefaultScriptureContextManager::new("KJV");

        let context = build_intelligence_context(
            &conn,
            &registry,
            None,
            &context_manager,
            &[],
            Vec::new(),
            ContextBounds::default(),
        )
        .unwrap();

        assert!(context.recent_transcript_segments.is_empty());
        assert!(context.current_transcript_segment.is_none());
        assert!(context.active_scripture_context.is_none());
    }

    #[test]
    fn context_reflects_a_real_active_scripture_context() {
        let conn = seeded_db();
        let content_conn = open_in_memory().unwrap();
        let mut content_conn = content_conn;
        run_migrations(&mut content_conn).unwrap();
        let registry = SqliteContentRegistry::new(content_conn);
        let mut context_manager = DefaultScriptureContextManager::new("KJV");
        context_manager.resolve(cip_core_bible::PartialScriptureReference {
            book: Some("ROM".to_string()),
            chapter: Some(8),
            ..Default::default()
        });

        let context = build_intelligence_context(
            &conn,
            &registry,
            None,
            &context_manager,
            &[],
            Vec::new(),
            ContextBounds::default(),
        )
        .unwrap();

        let active = context.active_scripture_context.unwrap();
        assert_eq!(active.book, "ROM");
        assert_eq!(active.chapter, 8);
    }
}
