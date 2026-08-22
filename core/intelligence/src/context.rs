//! [`IntelligenceContext`]: the bounded, explicit slice of live-service
//! state an engine is allowed to see. Deliberately not "the database" or
//! "all of `AppState`" - see Phase 2.0 spec sections 14-17.

use chrono::{DateTime, Utc};
use cip_core_ai::TranscriptSegment;
use cip_core_bible::ScriptureContext;
use cip_core_content::ContentMetadata;
use cip_core_sermon::foundation::{Sermon, SermonSection, SermonSegment};
use cip_core_service::ServiceStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::finding::IntelligenceFinding;

/// Maximum sizes for every unbounded-in-principle collection an
/// [`IntelligenceContext`] carries. Chosen conservatively and documented
/// here rather than left as magic numbers at each call site:
///
/// - 20 recent transcript segments: enough real conversational context for
///   an engine to see what led up to the current segment (a pastor
///   typically covers one train of thought in far fewer sentences than
///   this) without holding a whole service's transcript in memory. Matches
///   the order of magnitude the frontend already uses for the same reason
///   (`TRANSCRIPT_LIMIT = 20` in `LiveChurchBrain.tsx`).
/// - 20 recent findings: enough for a later-running engine (or a future
///   cross-domain correlator) to see what's already been found nearby,
///   without accumulating a whole service's finding history.
/// - 20 recent service events: same reasoning, scoped to service-lifecycle/
///   operator-action events rather than findings.
///
/// These are starting defaults, not requirements - construct
/// [`ContextBounds`] directly for a different limit.
pub const DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS: usize = 20;
pub const DEFAULT_MAX_RECENT_FINDINGS: usize = 20;
pub const DEFAULT_MAX_RECENT_SERVICE_EVENTS: usize = 20;
/// Bound for `IntelligenceContext.recent_sermon_segments` (Phase 2.5, per
/// the authoritative Phase 2 roadmap) - same order of magnitude as the
/// other "recent" bounds above, for the same reason: enough for an engine
/// to see what recently belonged to the active sermon without holding a
/// whole sermon's segment history.
pub const DEFAULT_MAX_RECENT_SERMON_SEGMENTS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBounds {
    pub max_recent_transcript_segments: usize,
    pub max_recent_findings: usize,
    pub max_recent_service_events: usize,
    pub max_recent_sermon_segments: usize,
}

impl Default for ContextBounds {
    fn default() -> Self {
        Self {
            max_recent_transcript_segments: DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS,
            max_recent_findings: DEFAULT_MAX_RECENT_FINDINGS,
            max_recent_service_events: DEFAULT_MAX_RECENT_SERVICE_EVENTS,
            max_recent_sermon_segments: DEFAULT_MAX_RECENT_SERMON_SEGMENTS,
        }
    }
}

/// The four conceptual context-window levels an engine can reason about
/// (Phase 2.0 spec section 15). Not a runtime state machine - a selector
/// used with [`IntelligenceContext::window`] to name which slice of the
/// context a caller means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindow {
    /// The single current transcript segment being processed.
    Current,
    /// Recent transcript segments (bounded by `max_recent_transcript_segments`).
    ShortTerm,
    /// Recent service activity: findings and service events (bounded by
    /// `max_recent_findings`/`max_recent_service_events`).
    MediumTerm,
    /// Service-level state: `service_status`, `content_metadata`.
    Service,
}

/// A minimal, storage-agnostic description of a service-lifecycle or
/// operator-action event, for `IntelligenceContext::recent_service_events`.
/// Deliberately not `apps/desktop/src-tauri::timeline::TimelineEntry` -
/// this crate must not depend on the Tauri crate; a caller in that crate
/// converts its own `TimelineEntry` values into these when building a
/// context (see `docs/intelligence-architecture.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEventSummary {
    pub name: String,
    pub occurred_at: DateTime<Utc>,
}

/// The bounded contextual information available to an
/// [`crate::IntelligenceEngine`] for one `analyze` call. Every collection
/// here is truncated to `bounds` at construction time via [`Self::build`] -
/// there is no way to end up with an unbounded `IntelligenceContext`
/// short of constructing the struct's fields directly (which nothing
/// outside this crate's tests does).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceContext {
    pub service_id: Uuid,
    pub service_status: Option<ServiceStatus>,
    pub current_transcript_segment: Option<TranscriptSegment>,
    pub recent_transcript_segments: Vec<TranscriptSegment>,
    pub active_scripture_context: Option<ScriptureContext>,
    pub recent_findings: Vec<IntelligenceFinding>,
    pub recent_service_events: Vec<ServiceEventSummary>,
    pub content_metadata: Vec<ContentMetadata>,
    /// The currently active Sermon Foundation entity (Phase 2.5, per the
    /// authoritative Phase 2 roadmap), if any - `None` both when no
    /// sermon is active and when the caller never attached sermon context
    /// at all (see [`Self::with_sermon_context`]). Never set directly by
    /// [`Self::build`], so every one of this crate's existing call sites
    /// (Bible/Music/Sermon/CrossDomain/Service adapters and their tests)
    /// remains valid, unmodified source - the same additive-extension
    /// discipline `IntelligenceFinding`'s own `with_*` builder methods
    /// already establish.
    pub active_sermon: Option<Sermon>,
    /// The sermon section open at the moment this context was built, if
    /// any.
    pub current_sermon_section: Option<SermonSection>,
    /// Bounded by `bounds.max_recent_sermon_segments` - see
    /// [`Self::with_sermon_context`].
    pub recent_sermon_segments: Vec<SermonSegment>,
    pub bounds: ContextBounds,
}

impl IntelligenceContext {
    /// Build a context, truncating every bounded collection to `bounds`.
    /// Callers may pass collections of any size (including, deliberately,
    /// 10,000+ transcript segments - see the boundary test) in
    /// chronological order (oldest first); only the most recent
    /// `bounds.max_*` entries are kept.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        service_id: Uuid,
        service_status: Option<ServiceStatus>,
        current_transcript_segment: Option<TranscriptSegment>,
        recent_transcript_segments: Vec<TranscriptSegment>,
        active_scripture_context: Option<ScriptureContext>,
        recent_findings: Vec<IntelligenceFinding>,
        recent_service_events: Vec<ServiceEventSummary>,
        content_metadata: Vec<ContentMetadata>,
        bounds: ContextBounds,
    ) -> Self {
        Self {
            service_id,
            service_status,
            current_transcript_segment,
            recent_transcript_segments: truncate_to_most_recent(
                recent_transcript_segments,
                bounds.max_recent_transcript_segments,
            ),
            active_scripture_context,
            recent_findings: truncate_to_most_recent(recent_findings, bounds.max_recent_findings),
            recent_service_events: truncate_to_most_recent(
                recent_service_events,
                bounds.max_recent_service_events,
            ),
            content_metadata,
            active_sermon: None,
            current_sermon_section: None,
            recent_sermon_segments: Vec::new(),
            bounds,
        }
    }

    /// Attach Sermon Foundation context (Phase 2.5, per the authoritative
    /// Phase 2 roadmap) additively, after [`Self::build`] - never a
    /// required constructor argument, so no existing caller of `build`
    /// needs to change. `recent_sermon_segments` is truncated to this
    /// context's own `bounds.max_recent_sermon_segments`, the same
    /// most-recent-first truncation every other bounded collection here
    /// already uses.
    pub fn with_sermon_context(
        mut self,
        active_sermon: Option<Sermon>,
        current_sermon_section: Option<SermonSection>,
        recent_sermon_segments: Vec<SermonSegment>,
    ) -> Self {
        self.active_sermon = active_sermon;
        self.current_sermon_section = current_sermon_section;
        self.recent_sermon_segments = truncate_to_most_recent(
            recent_sermon_segments,
            self.bounds.max_recent_sermon_segments,
        );
        self
    }

    /// Retrieve the slice of context named by `window` - a documentation-
    /// grade accessor rather than a data transformation (see
    /// [`ContextWindow`]'s docs for what each level means).
    pub fn window(&self, window: ContextWindow) -> ContextWindowView<'_> {
        match window {
            ContextWindow::Current => {
                ContextWindowView::Current(self.current_transcript_segment.as_ref())
            }
            ContextWindow::ShortTerm => {
                ContextWindowView::ShortTerm(&self.recent_transcript_segments)
            }
            ContextWindow::MediumTerm => {
                ContextWindowView::MediumTerm(&self.recent_findings, &self.recent_service_events)
            }
            ContextWindow::Service => {
                ContextWindowView::Service(self.service_status, &self.content_metadata)
            }
        }
    }
}

/// The data a [`ContextWindow`] selector resolves to.
#[derive(Debug, Clone, Copy)]
pub enum ContextWindowView<'a> {
    Current(Option<&'a TranscriptSegment>),
    ShortTerm(&'a [TranscriptSegment]),
    MediumTerm(&'a [IntelligenceFinding], &'a [ServiceEventSummary]),
    Service(Option<ServiceStatus>, &'a [ContentMetadata]),
}

/// Keep only the last `max` elements of `items`, preserving order. `items`
/// is assumed chronological (oldest first) - the same convention
/// `persistence::list_transcript_segments` already uses elsewhere in this
/// codebase.
fn truncate_to_most_recent<T>(mut items: Vec<T>, max: usize) -> Vec<T> {
    if items.len() > max {
        items.drain(0..items.len() - max);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};

    fn segment(sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: format!("segment {sequence}"),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    #[test]
    fn empty_context_builds_with_empty_bounded_collections() {
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        assert!(context.recent_transcript_segments.is_empty());
        assert!(context.recent_findings.is_empty());
        assert!(context.current_transcript_segment.is_none());
    }

    #[test]
    fn one_transcript_segment_is_kept_whole() {
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            Some(segment(1)),
            vec![segment(1)],
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        assert_eq!(context.recent_transcript_segments.len(), 1);
        assert_eq!(context.current_transcript_segment.unwrap().sequence, 1);
    }

    #[test]
    fn recent_transcript_segments_are_truncated_to_bounds() {
        let segments: Vec<TranscriptSegment> = (0..100).map(segment).collect();
        let bounds = ContextBounds {
            max_recent_transcript_segments: 5,
            ..ContextBounds::default()
        };
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            segments,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            bounds,
        );
        assert_eq!(context.recent_transcript_segments.len(), 5);
        // Kept the *most recent* 5 (highest sequence numbers), not the
        // first 5.
        let kept: Vec<u64> = context
            .recent_transcript_segments
            .iter()
            .map(|s| s.sequence)
            .collect();
        assert_eq!(kept, vec![95, 96, 97, 98, 99]);
    }

    #[test]
    fn ten_thousand_transcript_segments_never_produce_an_unbounded_context() {
        let segments: Vec<TranscriptSegment> = (0..10_000).map(segment).collect();
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            segments,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        assert_eq!(
            context.recent_transcript_segments.len(),
            DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS,
            "the full 10,000-segment transcript must never be copied into the context"
        );
    }

    #[test]
    fn window_accessors_resolve_to_the_matching_field() {
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            Some(ServiceStatus::Started),
            Some(segment(1)),
            vec![segment(1)],
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        assert!(matches!(
            context.window(ContextWindow::Current),
            ContextWindowView::Current(Some(_))
        ));
        assert!(
            matches!(context.window(ContextWindow::ShortTerm), ContextWindowView::ShortTerm(s) if s.len() == 1)
        );
        assert!(matches!(
            context.window(ContextWindow::Service),
            ContextWindowView::Service(Some(ServiceStatus::Started), _)
        ));
    }

    // --- Sermon Foundation context extension (Phase 2.5) --------------------

    #[test]
    fn a_plain_build_never_carries_sermon_context() {
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        assert!(context.active_sermon.is_none());
        assert!(context.current_sermon_section.is_none());
        assert!(context.recent_sermon_segments.is_empty());
    }

    #[test]
    fn with_sermon_context_truncates_recent_segments_to_bounds() {
        use cip_core_sermon::foundation::SermonSegment;

        let sermon_id = Uuid::new_v4();
        let segments: Vec<SermonSegment> = (0..50)
            .map(|i| SermonSegment::new(sermon_id, Uuid::new_v4(), i, None))
            .collect();
        let bounds = ContextBounds {
            max_recent_sermon_segments: 5,
            ..ContextBounds::default()
        };
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            bounds,
        )
        .with_sermon_context(None, None, segments);

        assert_eq!(context.recent_sermon_segments.len(), 5);
        let kept: Vec<u32> = context
            .recent_sermon_segments
            .iter()
            .map(|s| s.sequence)
            .collect();
        assert_eq!(
            kept,
            vec![45, 46, 47, 48, 49],
            "keeps the most recent, not the first"
        );
    }

    #[test]
    fn ten_thousand_sermon_segments_never_produce_an_unbounded_context() {
        use cip_core_sermon::foundation::SermonSegment;

        let sermon_id = Uuid::new_v4();
        let segments: Vec<SermonSegment> = (0..10_000)
            .map(|i| SermonSegment::new(sermon_id, Uuid::new_v4(), i, None))
            .collect();
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        )
        .with_sermon_context(None, None, segments);

        assert_eq!(
            context.recent_sermon_segments.len(),
            DEFAULT_MAX_RECENT_SERMON_SEGMENTS
        );
    }

    #[test]
    fn context_serializes_with_camel_case_fields() {
        let context = IntelligenceContext::build(
            Uuid::new_v4(),
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ContextBounds::default(),
        );
        let json = serde_json::to_value(&context).unwrap();
        assert!(json.get("recentTranscriptSegments").is_some());
        assert!(json.get("activeScriptureContext").is_some());
        assert!(json.get("contentMetadata").is_some());
        assert!(json.get("activeSermon").is_some());
        assert!(json.get("currentSermonSection").is_some());
        assert!(json.get("recentSermonSegments").is_some());
    }
}
