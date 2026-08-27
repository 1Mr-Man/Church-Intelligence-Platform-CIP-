//! [`CrossDomainCorrelationEngine`]: the correlation layer, first built in
//! Phase 2.4 and extended in Phase 2.8 (per the authoritative Phase 2
//! roadmap) once Service Intelligence, Sermon Foundation, and Content
//! Intelligence existed to correlate against. Reads already-produced
//! findings (and, since Phase 2.8, already-produced Content Intelligence
//! candidates) out of a shared [`IntelligenceContext`] and derives
//! [`IntelligenceCorrelation`]s between them - it never calls
//! `BibleIntelligenceEngine`, `MusicIntelligenceEngine`,
//! `SermonIntelligenceEngine`, `ServiceIntelligenceEngine`, or
//! `ContentIntelligenceEngine` directly, and it never mutates a source
//! finding, a content candidate, the transcript, or the active Scripture
//! context. See `docs/cross-domain-intelligence.md` for the full rule
//! catalogue, confidence hierarchy, and design rationale.
//!
//! ## Why this is not an `IntelligenceEngine`
//!
//! [`crate::IntelligenceEngine::analyze`] returns `Vec<IntelligenceFinding>` -
//! a correlation is a structurally different value
//! ([`IntelligenceCorrelation`], deliberately not folded into
//! `IntelligenceFinding`; see `correlation.rs`'s module docs), so this
//! engine does not implement the shared trait and is not registered into
//! `IntelligenceEngineRegistry`. This mirrors Phase 2.2's
//! `MusicIntelligenceEngine::analyze_acoustic`, an inherent method outside
//! the trait for the same reason (its input didn't fit the trait's shape
//! either).
//!
//! ## Determinism and failure isolation
//!
//! Every rule is a plain, pure function of `&AnalysisContext` - no shared
//! mutable state, no randomness beyond `IntelligenceCorrelation::new`'s
//! id/timestamp (excluded from every determinism test's comparison, the
//! same convention Phase 2.1-2.3 established). Each rule runs inside its
//! own `catch_unwind`: a panicking rule contributes zero correlations for
//! that call and every other rule still runs (spec section 23) - this
//! engine's own isolation, since it sits outside
//! `IntelligenceEngineRegistry::analyze_all`'s existing per-engine
//! isolation.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use uuid::Uuid;

use crate::content_candidate::ContentCandidate;
use crate::context::{IntelligenceContext, ServiceEventSummary};
use crate::correlation::{CorrelationKind, IntelligenceCorrelation};
use crate::domain::{AssertionLevel, FindingStatus, IntelligenceDomain};
use crate::engine::IntelligenceError;
use crate::finding::IntelligenceFinding;

// --- rule identity (spec section 34/35: provenance + rule versioning) -----

const RULE_VERSION: &str = "1.0";
const RULE_SCRIPTURE_SERMON: &str = "scripture_sermon_v1";
const RULE_THEME_SCRIPTURE: &str = "theme_scripture_v1";
const RULE_SERMON_MUSIC: &str = "sermon_music_v1";
const RULE_THEME_MUSIC: &str = "theme_music_v1";
const RULE_SCRIPTURE_MUSIC: &str = "scripture_music_v1";
const RULE_SERVICE_TRANSITION: &str = "service_transition_v1";
const RULE_TEMPORAL_ASSOCIATION: &str = "temporal_association_v1";
/// Phase 2.8 (per the authoritative Phase 2 roadmap) - see
/// [`rule_sermon_content`].
const RULE_SERMON_CONTENT: &str = "sermon_content_v1";
/// Phase 2.8 - see [`rule_multi_domain_convergence`].
const RULE_MULTI_DOMAIN_CONVERGENCE: &str = "multi_domain_convergence_v1";

// --- temporal windows (spec section 12) ------------------------------------

/// Two findings sharing at least one transcript segment id - the
/// strongest possible temporal evidence, since it means the same sentence
/// produced both.
const NEAR_WINDOW_SEGMENTS: u64 = 3;
const RECENT_WINDOW_SEGMENTS: u64 = 10;
/// Wall-clock window for [`rule_service_transition`], the one rule that
/// compares a finding's `created_at` against a service event's
/// `occurred_at` rather than transcript-segment sequence (neither carries
/// a segment id) - reuses existing timestamps, introduces no new clock.
const SERVICE_TRANSITION_WINDOW_SECONDS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TemporalTier {
    /// Same transcript segment.
    Immediate,
    /// Within `NEAR_WINDOW_SEGMENTS` sequence numbers of each other.
    Near,
    /// Within `RECENT_WINDOW_SEGMENTS` - deliberately never used to gate
    /// any rule below (spec section 12: proximity alone must stay weak);
    /// kept as a named tier for completeness and future headroom, not
    /// dead code - `temporal_relationship` returns it honestly rather
    /// than collapsing it into `None`.
    Recent,
}

/// Bounded, deterministic temporal relationship between two findings,
/// using only `transcript_segment_ids` (already on every
/// [`IntelligenceFinding`]) and `seq` (segment id -> sequence number,
/// built once per [`CrossDomainCorrelationEngine::analyze`] call from
/// `IntelligenceContext.recent_transcript_segments`). Returns `None` when
/// neither finding's segments are in the bounded recent window at all -
/// never guesses a distance for a segment id it can't resolve.
fn temporal_relationship(
    a: &IntelligenceFinding,
    b: &IntelligenceFinding,
    seq: &HashMap<Uuid, u64>,
) -> Option<TemporalTier> {
    if a.transcript_segment_ids
        .iter()
        .any(|id| b.transcript_segment_ids.contains(id))
    {
        return Some(TemporalTier::Immediate);
    }
    let a_seqs: Vec<u64> = a
        .transcript_segment_ids
        .iter()
        .filter_map(|id| seq.get(id).copied())
        .collect();
    let b_seqs: Vec<u64> = b
        .transcript_segment_ids
        .iter()
        .filter_map(|id| seq.get(id).copied())
        .collect();
    if a_seqs.is_empty() || b_seqs.is_empty() {
        return None;
    }
    let min_distance = a_seqs
        .iter()
        .flat_map(|&x| b_seqs.iter().map(move |&y| x.abs_diff(y)))
        .min()?;
    if min_distance <= NEAR_WINDOW_SEGMENTS {
        Some(TemporalTier::Near)
    } else if min_distance <= RECENT_WINDOW_SEGMENTS {
        Some(TemporalTier::Recent)
    } else {
        None
    }
}

/// The bounded, indexed view of a service's recent findings a rule
/// actually needs - built once per `analyze()` call, never re-derived per
/// rule. Findings with `status == Rejected` or `Expired` are excluded
/// entirely (spec section 31 scenario J: "do not silently treat rejected
/// evidence as accepted truth").
struct AnalysisContext<'a> {
    service_id: Uuid,
    by_domain: HashMap<IntelligenceDomain, Vec<&'a IntelligenceFinding>>,
    segment_sequence: HashMap<Uuid, u64>,
    recent_service_events: &'a [ServiceEventSummary],
    /// Recently-queued Content Intelligence candidates (Phase 2.8), already
    /// excluding `Rejected` ones - see [`rule_sermon_content`]. Never a
    /// re-detection: this is exactly `IntelligenceContext.recent_content_candidates`,
    /// filtered the same way `by_domain` filters findings.
    recent_content_candidates: Vec<&'a ContentCandidate>,
}

impl<'a> AnalysisContext<'a> {
    fn build(context: &'a IntelligenceContext) -> Self {
        let mut by_domain: HashMap<IntelligenceDomain, Vec<&IntelligenceFinding>> = HashMap::new();
        for finding in &context.recent_findings {
            if matches!(
                finding.status,
                FindingStatus::Rejected | FindingStatus::Expired
            ) {
                continue;
            }
            by_domain.entry(finding.domain).or_default().push(finding);
        }
        let segment_sequence = context
            .recent_transcript_segments
            .iter()
            .map(|s| (s.id, s.sequence))
            .collect();
        let recent_content_candidates = context
            .recent_content_candidates
            .iter()
            .filter(|c| c.status != FindingStatus::Rejected)
            .collect();
        Self {
            service_id: context.service_id,
            by_domain,
            segment_sequence,
            recent_service_events: &context.recent_service_events,
            recent_content_candidates,
        }
    }

    fn domain(&self, domain: IntelligenceDomain) -> &[&'a IntelligenceFinding] {
        self.by_domain
            .get(&domain)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The Sermon-domain finding a content candidate was derived from, if
    /// it's still within the bounded recent-findings window - never
    /// guessed, and `None` when the parent finding has aged out (the
    /// candidate simply cannot be correlated in that case, exactly like
    /// [`temporal_relationship`] returning `None` for an unresolved
    /// segment id).
    fn content_candidate_parent(
        &self,
        candidate: &ContentCandidate,
    ) -> Option<&'a IntelligenceFinding> {
        self.domain(IntelligenceDomain::Sermon)
            .iter()
            .find(|f| candidate.source_finding_ids.contains(&f.id))
            .copied()
    }
}

fn confidence(score: f32, reason: impl Into<String>) -> ConfidenceResult {
    ConfidenceResult::new(score, ConfidenceSource::Heuristic, Some(reason.into()))
}

fn evidence_pair(a: Uuid, b: Uuid) -> Vec<crate::evidence::EvidenceSource> {
    vec![
        crate::evidence::EvidenceSource::AnotherFinding { finding_id: a },
        crate::evidence::EvidenceSource::AnotherFinding { finding_id: b },
    ]
}

// --- reference-token extraction (Scripture matching, never fabricated) ----

/// `"ROM 8:28"` -> `("ROM", 8, Some(28))`; `"ROM 8"` -> `("ROM", 8, None)`.
/// Pure syntax over text the Bible/Sermon adapters already produced -
/// never a new Scripture parser (that stays the Bible engine's job).
fn parse_reference_token(token: &str) -> Option<(String, u32, Option<u32>)> {
    let (book_chapter, verse) = match token.split_once(':') {
        Some((bc, v)) => (bc, v.trim().parse::<u32>().ok()),
        None => (token, None),
    };
    let (book, chapter) = book_chapter.trim().rsplit_once(' ')?;
    let chapter: u32 = chapter.trim().parse().ok()?;
    Some((book.trim().to_string(), chapter, verse))
}

/// A Bible-domain finding's summary is always either a bare reference
/// (`"ROM 8:28"`, Direct/Verse/Sequential) or `"Active Scripture Context:
/// ROM 8"` (Chapter) - see `bible_adapter::finding_for_detection`. Never
/// re-detects Scripture; only reads what the Bible engine already wrote.
fn bible_reference_token(finding: &IntelligenceFinding) -> Option<String> {
    if finding.domain != IntelligenceDomain::Bible {
        return None;
    }
    Some(
        finding
            .summary
            .strip_prefix("Active Scripture Context: ")
            .unwrap_or(&finding.summary)
            .to_string(),
    )
}

/// A Sermon-domain finding names a Scripture reference only via its
/// `"Supporting Scripture: ..."` cross-link (see
/// `sermon_adapter::finding_for_scripture_cross_link`) - `core/sermon`
/// never detects Scripture itself.
fn sermon_reference_token(finding: &IntelligenceFinding) -> Option<String> {
    if finding.domain != IntelligenceDomain::Sermon {
        return None;
    }
    finding
        .summary
        .strip_prefix("Supporting Scripture: ")
        .map(|s| s.to_string())
}

fn is_theme_finding(finding: &IntelligenceFinding) -> bool {
    finding.domain == IntelligenceDomain::Sermon && finding.summary.starts_with("Theme: ")
}

/// Sermon findings whose summary shape suggests a worship/closing
/// transition moment - used only to widen `rule_sermon_music`'s `Near`
/// tier eligibility beyond a shared segment; an arbitrary Sermon
/// `Observed` finding merely near a Music finding is not, on its own,
/// evidence of a worship transition.
fn is_transition_shaped(finding: &IntelligenceFinding) -> bool {
    finding.domain == IntelligenceDomain::Sermon
        && (finding.summary.starts_with("Transition: ")
            || finding.summary.starts_with("Possible Conclusion: ")
            || finding.summary.starts_with("Prayer Point: "))
}

fn is_conclusion_shaped(finding: &IntelligenceFinding) -> bool {
    finding.domain == IntelligenceDomain::Sermon
        && (finding.summary.starts_with("Possible Conclusion: ")
            || finding.summary.starts_with("Transition: "))
}

// --- rules ------------------------------------------------------------------

/// Sermon finding names the same Scripture reference as a Bible finding.
/// Exact book+chapter+verse match is the strongest evidence this engine
/// ever produces; a chapter-only match (one side has no verse) is high
/// but not exact.
fn rule_scripture_sermon(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for sermon in ctx.domain(IntelligenceDomain::Sermon) {
        let Some(s_token) = sermon_reference_token(sermon) else {
            continue;
        };
        let Some((s_book, s_chapter, s_verse)) = parse_reference_token(&s_token) else {
            continue;
        };
        for bible in ctx.domain(IntelligenceDomain::Bible) {
            let Some(b_token) = bible_reference_token(bible) else {
                continue;
            };
            let Some((b_book, b_chapter, b_verse)) = parse_reference_token(&b_token) else {
                continue;
            };
            if s_book != b_book || s_chapter != b_chapter {
                continue;
            }
            let (score, reason) = match (s_verse, b_verse) {
                (Some(sv), Some(bv)) if sv == bv => (
                    0.95,
                    "exact shared scripture reference (book, chapter, and verse match)",
                ),
                _ => (
                    0.75,
                    "shared scripture chapter between sermon and Bible findings",
                ),
            };
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![sermon.id, bible.id],
                    vec![IntelligenceDomain::Sermon, IntelligenceDomain::Bible],
                    CorrelationKind::ScriptureSermon,
                    AssertionLevel::Inferred,
                    confidence(score, reason),
                    format!("Sermon references {s_token}, matching Bible finding {b_token}"),
                    RULE_SCRIPTURE_SERMON,
                    RULE_VERSION,
                )
                .with_evidence(evidence_pair(sermon.id, bible.id)),
            );
        }
    }
    out
}

/// Sermon theme candidate near a Bible finding in the transcript -
/// "explicit transcript linkage," per spec section 9, means the theme
/// finding's own evidence segments are the same as or adjacent to the
/// Bible finding's segments; a "Recent" (±10) distance is never enough
/// (spec: "do not use theological assumptions... do not correlate merely
/// because both happened in the same service").
fn rule_theme_scripture(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for theme in ctx
        .domain(IntelligenceDomain::Sermon)
        .iter()
        .filter(|f| is_theme_finding(f))
    {
        for bible in ctx.domain(IntelligenceDomain::Bible) {
            let Some(tier) = temporal_relationship(theme, bible, &ctx.segment_sequence) else {
                continue;
            };
            let score = match tier {
                TemporalTier::Immediate => 0.7,
                TemporalTier::Near => 0.5,
                TemporalTier::Recent => continue,
            };
            let b_ref = bible_reference_token(bible).unwrap_or_else(|| bible.summary.clone());
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![theme.id, bible.id],
                    vec![IntelligenceDomain::Sermon, IntelligenceDomain::Bible],
                    CorrelationKind::ThemeScripture,
                    AssertionLevel::Inferred,
                    confidence(
                        score,
                        format!(
                            "{theme:?} transcript proximity to Scripture finding {b_ref}",
                            theme = tier
                        ),
                    ),
                    format!("{} occurs near Scripture reference {b_ref}", theme.summary),
                    RULE_THEME_SCRIPTURE,
                    RULE_VERSION,
                )
                .with_evidence(vec![
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: theme.id,
                    },
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: bible.id,
                    },
                    crate::evidence::EvidenceSource::Temporal {
                        description: format!("{tier:?} transcript proximity"),
                    },
                ]),
            );
        }
    }
    out
}

/// A Sermon finding shaped like a transition/conclusion/prayer signal
/// (see [`is_transition_shaped`]) close to a Music finding - the
/// "sermon transition + music recognized shortly after" pattern (spec
/// section 10). A non-transition-shaped Sermon finding only qualifies at
/// `Immediate` (the same segment explicitly mentions both).
fn rule_sermon_music(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for sermon in ctx.domain(IntelligenceDomain::Sermon) {
        for music in ctx.domain(IntelligenceDomain::Music) {
            let Some(tier) = temporal_relationship(sermon, music, &ctx.segment_sequence) else {
                continue;
            };
            let score = match tier {
                TemporalTier::Immediate => 0.85,
                TemporalTier::Near if is_transition_shaped(sermon) => 0.7,
                TemporalTier::Near | TemporalTier::Recent => continue,
            };
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![sermon.id, music.id],
                    vec![IntelligenceDomain::Sermon, IntelligenceDomain::Music],
                    CorrelationKind::SermonMusic,
                    AssertionLevel::Inferred,
                    confidence(
                        score,
                        format!("{tier:?} transcript proximity between sermon and music findings"),
                    ),
                    format!(
                        "{} occurs near music finding {}",
                        sermon.summary, music.summary
                    ),
                    RULE_SERMON_MUSIC,
                    RULE_VERSION,
                )
                .with_evidence(vec![
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: sermon.id,
                    },
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: music.id,
                    },
                    crate::evidence::EvidenceSource::Temporal {
                        description: format!("{tier:?} transcript proximity"),
                    },
                ]),
            );
        }
    }
    out
}

/// Sermon theme near a Music finding - temporal proximity only, no
/// semantic/lyric matching (a song's title or lyrics are never compared
/// against the theme label - that would be exactly the kind of
/// unsupported theological/semantic association spec section 11
/// forbids).
fn rule_theme_music(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for theme in ctx
        .domain(IntelligenceDomain::Sermon)
        .iter()
        .filter(|f| is_theme_finding(f))
    {
        for music in ctx.domain(IntelligenceDomain::Music) {
            let Some(tier) = temporal_relationship(theme, music, &ctx.segment_sequence) else {
                continue;
            };
            let score = match tier {
                TemporalTier::Immediate => 0.55,
                TemporalTier::Near => 0.4,
                TemporalTier::Recent => continue,
            };
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![theme.id, music.id],
                    vec![IntelligenceDomain::Sermon, IntelligenceDomain::Music],
                    CorrelationKind::ThemeMusic,
                    AssertionLevel::Inferred,
                    confidence(
                        score,
                        format!("{tier:?} transcript proximity between theme and music findings, no semantic matching"),
                    ),
                    format!("{} occurs near music finding {}", theme.summary, music.summary),
                    RULE_THEME_MUSIC,
                    RULE_VERSION,
                )
                .with_evidence(vec![
                    crate::evidence::EvidenceSource::AnotherFinding { finding_id: theme.id },
                    crate::evidence::EvidenceSource::AnotherFinding { finding_id: music.id },
                    crate::evidence::EvidenceSource::Temporal {
                        description: format!("{tier:?} transcript proximity"),
                    },
                ]),
            );
        }
    }
    out
}

/// Bible finding and Music finding sharing a transcript segment - the
/// only evidence this engine treats as strong enough to name a
/// `ScriptureMusic` relationship (spec section 11: "Song title 'Amazing
/// Grace' + Romans 8 must NOT automatically become a correlation" - mere
/// proximity, without a shared segment, never qualifies here; it may
/// still surface as a low-confidence `TemporalProximity` via
/// [`rule_temporal_association`]).
fn rule_scripture_music(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for bible in ctx.domain(IntelligenceDomain::Bible) {
        for music in ctx.domain(IntelligenceDomain::Music) {
            if temporal_relationship(bible, music, &ctx.segment_sequence)
                != Some(TemporalTier::Immediate)
            {
                continue;
            }
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![bible.id, music.id],
                    vec![IntelligenceDomain::Bible, IntelligenceDomain::Music],
                    CorrelationKind::ScriptureMusic,
                    AssertionLevel::Inferred,
                    confidence(
                        0.8,
                        "Bible and Music findings share the same transcript segment",
                    ),
                    format!(
                        "Scripture finding {} and music finding {} were said in the same breath",
                        bible.summary, music.summary
                    ),
                    RULE_SCRIPTURE_MUSIC,
                    RULE_VERSION,
                )
                .with_evidence(evidence_pair(bible.id, music.id)),
            );
        }
    }
    out
}

/// A sermon conclusion/transition signal coinciding with a
/// service-lifecycle event (`SERVICE_ENDED`/`SERVICE_PAUSED`/
/// `SERMON_STATE_CHANGED`) within [`SERVICE_TRANSITION_WINDOW_SECONDS`] -
/// the "closing/worship transition" pattern (spec section 6), anchored to
/// service events (already in `IntelligenceContext.recent_service_events`)
/// rather than another finding.
fn rule_service_transition(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for sermon in ctx
        .domain(IntelligenceDomain::Sermon)
        .iter()
        .filter(|f| is_conclusion_shaped(f))
    {
        for event in ctx.recent_service_events {
            if !matches!(
                event.name.as_str(),
                "SERVICE_ENDED" | "SERVICE_PAUSED" | "SERMON_STATE_CHANGED"
            ) {
                continue;
            }
            let delta = (event.occurred_at - sermon.created_at).num_seconds();
            if !(0..=SERVICE_TRANSITION_WINDOW_SECONDS).contains(&delta) {
                continue;
            }
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![sermon.id],
                    vec![IntelligenceDomain::Sermon, IntelligenceDomain::Service],
                    CorrelationKind::ServiceTransition,
                    AssertionLevel::Inferred,
                    confidence(
                        0.55,
                        format!(
                            "{} occurred {delta}s before service event {}",
                            sermon.summary, event.name
                        ),
                    ),
                    format!("{} coincides with {}", sermon.summary, event.name),
                    RULE_SERVICE_TRANSITION,
                    RULE_VERSION,
                )
                .with_evidence(vec![
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: sermon.id,
                    },
                    crate::evidence::EvidenceSource::ServiceEvent {
                        description: event.name.clone(),
                    },
                ]),
            );
        }
    }
    out
}

/// A Content Intelligence candidate (Phase 2.7's `ContentCandidate`)
/// relates to a Bible or Music finding, via the candidate's own source
/// Sermon finding's transcript proximity to that finding (Phase 2.8) -
/// never a re-derivation of the candidate itself (its `content_potential`,
/// `title_or_label`, and `working_concept` are read but never recomputed
/// or mutated), and never a correlation between the candidate and its own
/// parent Sermon finding (that link already exists, verbatim, as
/// `ContentCandidate.source_finding_ids` - restating it as a correlation
/// would be a tautology, not a discovery). One tier lower than
/// [`rule_theme_scripture`]'s confidence at each tier, since a content
/// candidate is one derivation step further from the transcript than the
/// Sermon finding it came from.
fn rule_sermon_content(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    for candidate in &ctx.recent_content_candidates {
        let Some(parent) = ctx.content_candidate_parent(candidate) else {
            continue;
        };
        for (domain, other) in ctx
            .domain(IntelligenceDomain::Bible)
            .iter()
            .map(|f| (IntelligenceDomain::Bible, *f))
            .chain(
                ctx.domain(IntelligenceDomain::Music)
                    .iter()
                    .map(|f| (IntelligenceDomain::Music, *f)),
            )
        {
            let Some(tier) = temporal_relationship(parent, other, &ctx.segment_sequence) else {
                continue;
            };
            let score = match tier {
                TemporalTier::Immediate => 0.65,
                TemporalTier::Near => 0.45,
                TemporalTier::Recent => continue,
            };
            out.push(
                IntelligenceCorrelation::new(
                    ctx.service_id,
                    vec![candidate.id, other.id],
                    vec![IntelligenceDomain::Content, domain],
                    CorrelationKind::SermonContent,
                    AssertionLevel::Inferred,
                    confidence(
                        score,
                        format!(
                            "{tier:?} transcript proximity between content candidate's source sermon finding and a {domain:?} finding"
                        ),
                    ),
                    format!(
                        "Content candidate '{}' relates to {}",
                        candidate.title_or_label, other.summary
                    ),
                    RULE_SERMON_CONTENT,
                    RULE_VERSION,
                )
                // Reuses `AnotherFinding` for the candidate's own id as
                // well as `other`'s - both are Uuid-identified derived
                // intelligence objects (see `EvidenceSource`'s docs);
                // introducing a parallel evidence variant only for this
                // one link would violate this crate's reuse discipline.
                .with_evidence(vec![
                    crate::evidence::EvidenceSource::AnotherFinding {
                        finding_id: candidate.id,
                    },
                    crate::evidence::EvidenceSource::AnotherFinding { finding_id: other.id },
                    crate::evidence::EvidenceSource::Temporal {
                        description: format!("{tier:?} transcript proximity (via source sermon finding)"),
                    },
                ]),
            );
        }
    }
    out
}

/// Three or more distinct `IntelligenceDomain`s' findings share the same
/// literal transcript segment (Phase 2.8) - stronger evidence than any
/// single pairwise rule alone, since every domain in the group references
/// the exact same moment, not merely a nearby one. Deliberately scoped to
/// true `IntelligenceFinding` domains (Bible/Music/Sermon/Service); Content
/// candidates are not included here (see [`rule_sermon_content`] for the
/// one rule that does connect them) - a candidate carries no
/// `transcript_segment_ids` of its own to cluster by. Never claims a
/// causal or theological connection, only that the domains were said
/// together (spec: "RELATIONSHIP != COINCIDENCE" still applies - this rule
/// reports the coincidence honestly, as breadth of evidence, not as
/// meaning).
fn rule_multi_domain_convergence(ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    let convergence_domains = [
        IntelligenceDomain::Bible,
        IntelligenceDomain::Music,
        IntelligenceDomain::Sermon,
        IntelligenceDomain::Service,
    ];
    let mut by_segment: HashMap<Uuid, Vec<&IntelligenceFinding>> = HashMap::new();
    for domain in convergence_domains {
        for finding in ctx.domain(domain) {
            for seg_id in &finding.transcript_segment_ids {
                by_segment.entry(*seg_id).or_default().push(finding);
            }
        }
    }
    let mut segment_ids: Vec<Uuid> = by_segment.keys().copied().collect();
    segment_ids.sort_unstable();

    for seg_id in segment_ids {
        let findings = &by_segment[&seg_id];
        let domains_present: Vec<IntelligenceDomain> = convergence_domains
            .iter()
            .copied()
            .filter(|d| findings.iter().any(|f| f.domain == *d))
            .collect();
        if domains_present.len() < 3 {
            continue;
        }
        let mut ids: Vec<Uuid> = findings.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        ids.dedup();
        let score = if domains_present.len() >= 4 {
            0.9
        } else {
            0.85
        };
        let summaries: Vec<&str> = findings.iter().map(|f| f.summary.as_str()).collect();
        out.push(
            IntelligenceCorrelation::new(
                ctx.service_id,
                ids.clone(),
                domains_present.clone(),
                CorrelationKind::MultiDomainConvergence,
                AssertionLevel::Inferred,
                confidence(
                    score,
                    format!(
                        "{} distinct domains share the same transcript segment",
                        domains_present.len()
                    ),
                ),
                format!(
                    "{} domains converge on the same transcript moment: {}",
                    domains_present.len(),
                    summaries.join("; ")
                ),
                RULE_MULTI_DOMAIN_CONVERGENCE,
                RULE_VERSION,
            )
            .with_evidence(
                ids.iter()
                    .map(|id| crate::evidence::EvidenceSource::AnotherFinding { finding_id: *id })
                    .collect(),
            ),
        );
    }
    out
}

/// Fallback: any cross-domain pair at `Immediate`/`Near` proximity not
/// already claimed by a stronger rule - always low confidence, never
/// promoted further (spec section 12). Phase 2.8 adds `Service` to this
/// domain set - previously a Service finding could never participate even
/// in this weakest fallback, leaving Service<->Music and Service<->Scripture
/// with no correlation path at all. No dedicated `ServiceMusic`/
/// `ServiceScripture` `CorrelationKind` was added: no evidence stronger
/// than temporal proximity connects these pairs anywhere in this engine
/// (unlike Sermon<->Service, which keeps its own [`rule_service_transition`]
/// because a sermon conclusion signal is meaningfully, specifically tied to
/// a service-lifecycle event) - inventing a same-strength dedicated kind
/// would only add taxonomy surface without adding informational value.
fn rule_temporal_association(
    ctx: &AnalysisContext,
    claimed: &HashSet<(Uuid, Uuid)>,
) -> Vec<IntelligenceCorrelation> {
    let mut out = Vec::new();
    let domains = [
        IntelligenceDomain::Bible,
        IntelligenceDomain::Music,
        IntelligenceDomain::Sermon,
        IntelligenceDomain::Service,
    ];
    for i in 0..domains.len() {
        for j in (i + 1)..domains.len() {
            for a in ctx.domain(domains[i]) {
                for b in ctx.domain(domains[j]) {
                    if claimed.contains(&pair_key(a.id, b.id)) {
                        continue;
                    }
                    let Some(tier) = temporal_relationship(a, b, &ctx.segment_sequence) else {
                        continue;
                    };
                    let score = match tier {
                        TemporalTier::Immediate => 0.35,
                        TemporalTier::Near => 0.25,
                        TemporalTier::Recent => continue,
                    };
                    out.push(
                        IntelligenceCorrelation::new(
                            ctx.service_id,
                            vec![a.id, b.id],
                            vec![domains[i], domains[j]],
                            CorrelationKind::TemporalProximity,
                            AssertionLevel::Inferred,
                            confidence(
                                score,
                                format!("{tier:?} transcript proximity only - no other evidence connects these findings"),
                            ),
                            format!(
                                "{} and {} occurred near each other in the transcript",
                                a.summary, b.summary
                            ),
                            RULE_TEMPORAL_ASSOCIATION,
                            RULE_VERSION,
                        )
                        .with_evidence(vec![
                            crate::evidence::EvidenceSource::AnotherFinding { finding_id: a.id },
                            crate::evidence::EvidenceSource::AnotherFinding { finding_id: b.id },
                            crate::evidence::EvidenceSource::Temporal {
                                description: format!("{tier:?} transcript proximity"),
                            },
                        ]),
                    );
                }
            }
        }
    }
    out
}

fn pair_key(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

// --- engine ------------------------------------------------------------------

type Rule = fn(&AnalysisContext) -> Vec<IntelligenceCorrelation>;

const STRONG_RULES: &[Rule] = &[
    rule_scripture_sermon,
    rule_theme_scripture,
    rule_sermon_music,
    rule_theme_music,
    rule_scripture_music,
    rule_service_transition,
    rule_sermon_content,
    rule_multi_domain_convergence,
];

/// The Phase 2.4 correlation layer. Stateless (no fields yet - reserved
/// for future configurable thresholds); safe to construct fresh per call
/// or hold for a service's lifetime.
#[derive(Debug, Default, Clone, Copy)]
pub struct CrossDomainCorrelationEngine;

impl CrossDomainCorrelationEngine {
    pub fn new() -> Self {
        Self
    }

    /// Derive every correlation the current bounded context supports.
    /// Deterministic for identical input (spec section 16/56): running
    /// this twice against equivalent contexts produces equivalent
    /// correlations (ignoring `id`/`created_at`, exactly like every other
    /// determinism test in this codebase). Never panics: each rule is
    /// isolated in its own `catch_unwind` (spec section 23).
    pub fn analyze(&self, context: &IntelligenceContext) -> Vec<IntelligenceCorrelation> {
        let ctx = AnalysisContext::build(context);
        let mut all = run_rules(&ctx, STRONG_RULES);

        let mut claimed: HashSet<(Uuid, Uuid)> = HashSet::new();
        for correlation in &all {
            if let [a, b] = correlation.source_finding_ids[..] {
                claimed.insert(pair_key(a, b));
            }
        }
        if let Ok(produced) = catch_unwind(AssertUnwindSafe(|| {
            rule_temporal_association(&ctx, &claimed)
        })) {
            all.extend(produced);
        }

        dedup(&mut all);
        sort_deterministically(&mut all);
        all
    }
}

/// Run every rule in `rules`, isolating each in its own `catch_unwind` - a
/// panicking rule contributes nothing and every other rule still runs.
/// Factored out of `analyze` so the failure-isolation test can inject a
/// deliberately panicking rule alongside real ones without needing a
/// trait-object/mocking layer.
fn run_rules(ctx: &AnalysisContext, rules: &[Rule]) -> Vec<IntelligenceCorrelation> {
    let mut all = Vec::new();
    for rule in rules {
        if let Ok(produced) = catch_unwind(AssertUnwindSafe(|| rule(ctx))) {
            all.extend(produced);
        }
    }
    all
}

/// Deterministic duplicate suppression (spec section 16): keep only the
/// first occurrence of each equivalence class
/// ([`IntelligenceCorrelation::is_equivalent_to`]). A hash-keyed pass
/// (`service_id` + `kind`'s `Debug` form, which is unique per distinct
/// `CorrelationKind` value including each `Other(detail)`, + sorted source
/// ids) rather than the naive O(n^2) "scan every already-kept candidate" -
/// measured to matter at realistic candidate-pool sizes (spec section 22).
fn dedup(correlations: &mut Vec<IntelligenceCorrelation>) {
    let mut seen: HashSet<(Uuid, String, Vec<Uuid>)> = HashSet::with_capacity(correlations.len());
    correlations.retain(|candidate| {
        let mut ids = candidate.source_finding_ids.clone();
        ids.sort_unstable();
        let key = (candidate.service_id, format!("{:?}", candidate.kind), ids);
        seen.insert(key)
    });
}

/// Deterministic ordering (spec section 17): confidence descending, then
/// kind label, then sorted source finding ids, then id as a final stable
/// tiebreak. Never depends on `HashMap` iteration order.
fn sort_deterministically(correlations: &mut [IntelligenceCorrelation]) {
    correlations.sort_by(|a, b| {
        b.confidence
            .score
            .total_cmp(&a.confidence.score)
            .then_with(|| a.kind.label().cmp(b.kind.label()))
            .then_with(|| {
                let mut a_ids = a.source_finding_ids.clone();
                a_ids.sort_unstable();
                let mut b_ids = b.source_finding_ids.clone();
                b_ids.sort_unstable();
                a_ids.cmp(&b_ids)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

// --- storage -----------------------------------------------------------------

/// What happened when a correlation was added - mirrors
/// [`crate::QueueAddOutcome`] exactly, kept as its own type only because
/// [`CorrelationQueue`] holds a different element type
/// (`FindingQueue`/`QueueAddOutcome` are hard-typed to
/// `IntelligenceFinding`, so this is the minimum necessary parallel
/// structure rather than a generalization of the existing one - spec
/// section 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationQueueAddOutcome {
    Added,
    /// An equivalent correlation ([`IntelligenceCorrelation::is_equivalent_to`])
    /// was already queued and not yet dismissed; the new one was
    /// discarded rather than creating an uncontrolled duplicate.
    DuplicateIgnored,
}

/// In-memory queue of cross-domain correlations awaiting operator review -
/// the `IntelligenceCorrelation` counterpart to [`crate::FindingQueue`].
/// No new database table (spec section 29's explicit default): a
/// correlation is derived from findings that themselves already have
/// provenance, so nothing here needs to survive a restart.
#[derive(Default)]
pub struct CorrelationQueue {
    correlations: Vec<IntelligenceCorrelation>,
}

impl CorrelationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a correlation, unless an equivalent one is already queued with
    /// a status that hasn't been resolved yet (`Detected`/`Reviewed`) -
    /// mirrors `FindingQueue::add`'s exact dedup policy.
    pub fn add(&mut self, correlation: IntelligenceCorrelation) -> CorrelationQueueAddOutcome {
        let is_duplicate = self.correlations.iter().any(|existing| {
            matches!(
                existing.status,
                FindingStatus::Detected | FindingStatus::Reviewed
            ) && existing.is_equivalent_to(&correlation)
        });
        if is_duplicate {
            return CorrelationQueueAddOutcome::DuplicateIgnored;
        }
        self.correlations.push(correlation);
        CorrelationQueueAddOutcome::Added
    }

    /// Correlations still awaiting an operator decision
    /// (`Detected`/`Reviewed`), ordered by confidence (highest first).
    pub fn pending(&self) -> Vec<&IntelligenceCorrelation> {
        let mut pending: Vec<&IntelligenceCorrelation> = self
            .correlations
            .iter()
            .filter(|c| matches!(c.status, FindingStatus::Detected | FindingStatus::Reviewed))
            .collect();
        pending.sort_by(|a, b| {
            b.confidence
                .score
                .total_cmp(&a.confidence.score)
                .then(a.id.cmp(&b.id))
        });
        pending
    }

    fn find_mut(&mut self, id: Uuid) -> Result<&mut IntelligenceCorrelation, IntelligenceError> {
        self.correlations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(IntelligenceError::CorrelationNotFound(id))
    }

    pub fn review(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.review();
        Ok(())
    }

    /// Explicit operator dismissal (spec section 25) - changes only this
    /// correlation's own status.
    pub fn dismiss(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.dismiss();
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> Option<&IntelligenceCorrelation> {
        self.correlations.iter().find(|c| c.id == id)
    }

    /// Every correlation ever added, regardless of status, oldest first.
    pub fn all(&self) -> Vec<&IntelligenceCorrelation> {
        self.correlations.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.correlations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.correlations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextBounds;
    use crate::domain::FindingKind;
    use crate::finding::IntelligenceFinding;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult as CR, ConfidenceSource as CS};

    fn segment(sequence: u64) -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence,
            text: format!("segment {sequence}"),
            is_final: true,
            confidence: CR::new(0.9, CS::Model, None),
            start_ms: sequence * 1000,
            end_ms: sequence * 1000 + 900,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finding(
        service_id: Uuid,
        domain: IntelligenceDomain,
        kind: FindingKind,
        assertion_level: AssertionLevel,
        summary: &str,
        segments: &[Uuid],
    ) -> IntelligenceFinding {
        IntelligenceFinding::new(
            service_id,
            domain,
            kind,
            assertion_level,
            CR::new(0.9, CS::Heuristic, None),
            summary,
            "test-engine",
            "1.0",
        )
        .with_transcript_segments(segments.to_vec())
    }

    fn bible_finding(service_id: Uuid, summary: &str, segments: &[Uuid]) -> IntelligenceFinding {
        finding(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            summary,
            segments,
        )
    }

    fn sermon_finding(service_id: Uuid, summary: &str, segments: &[Uuid]) -> IntelligenceFinding {
        finding(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Inferred,
            summary,
            segments,
        )
    }

    fn music_finding(service_id: Uuid, summary: &str, segments: &[Uuid]) -> IntelligenceFinding {
        finding(
            service_id,
            IntelligenceDomain::Music,
            FindingKind::Music,
            AssertionLevel::Suggested,
            summary,
            segments,
        )
    }

    fn build_context(
        service_id: Uuid,
        segments: Vec<TranscriptSegment>,
        findings: Vec<IntelligenceFinding>,
        events: Vec<ServiceEventSummary>,
    ) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            None,
            segments.last().cloned(),
            segments,
            None,
            findings,
            events,
            Vec::new(),
            ContextBounds::default(),
        )
    }

    // --- Scenario A: exact Scripture <-> Sermon reference match ------------

    #[test]
    fn scenario_a_scripture_sermon_exact_reference_match() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        let sermon = sermon_finding(service_id, "Supporting Scripture: ROM 8:28", &[s0.id]);
        let bible = bible_finding(service_id, "ROM 8:28", &[s1.id]);
        let context = build_context(
            service_id,
            vec![s0, s1],
            vec![sermon.clone(), bible.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::ScriptureSermon)
            .expect("expected a ScriptureSermon correlation");
        assert_eq!(hit.confidence.score, 0.95);
        assert!(hit.source_finding_ids.contains(&sermon.id));
        assert!(hit.source_finding_ids.contains(&bible.id));
        assert_eq!(hit.assertion_level, AssertionLevel::Inferred);
    }

    // --- Scenario B: Sermon transition + Music (SermonMusic) ---------------

    #[test]
    fn scenario_b_sermon_music_transition() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        let sermon = sermon_finding(service_id, "Transition: TEACHING -> PRAYER", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s1.id]);
        let context = build_context(
            service_id,
            vec![s0, s1],
            vec![sermon.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::SermonMusic)
            .expect("expected a SermonMusic correlation");
        assert!(hit.source_finding_ids.contains(&sermon.id));
        assert!(hit.source_finding_ids.contains(&music.id));
    }

    // --- Scenario C: explicit transcript linkage (shared segment) ----------

    #[test]
    fn scenario_c_scripture_music_explicit_transcript_linkage() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "JHN 3:16", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let context = build_context(
            service_id,
            vec![s0],
            vec![bible.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::ScriptureMusic)
            .expect("expected a ScriptureMusic correlation from shared-segment evidence");
        assert_eq!(hit.confidence.score, 0.8);
    }

    // --- Scenario D: same findings, far apart -------------------------------

    #[test]
    fn scenario_d_far_apart_findings_produce_no_strong_correlation() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s_far = segment(50);
        let bible = bible_finding(service_id, "JHN 3:16", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s_far.id]);
        let context = build_context(service_id, vec![s0, s_far], vec![bible, music], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(
            correlations.is_empty(),
            "findings 50 segments apart must produce no correlation at all: {correlations:?}"
        );
    }

    // --- Scenario E: temporal proximity only, no other evidence ------------

    #[test]
    fn scenario_e_temporal_proximity_only_yields_low_confidence_fallback() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        // Plain Observed sermon findings with no theme/transition shape,
        // near a music finding - no stronger rule should fire.
        let sermon = sermon_finding(service_id, "Question: Do you believe this?", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s1.id]);
        let context = build_context(
            service_id,
            vec![s0, s1],
            vec![sermon.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert_eq!(correlations.len(), 1);
        assert_eq!(correlations[0].kind, CorrelationKind::TemporalProximity);
        assert!(
            correlations[0].confidence.score < 0.5,
            "temporal-only evidence must stay low confidence"
        );
    }

    // --- Scenario F: duplicate analysis -------------------------------------

    #[test]
    fn scenario_f_duplicate_analysis_produces_an_identical_correlation_set() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        let sermon = sermon_finding(service_id, "Supporting Scripture: ROM 8:28", &[s0.id]);
        let bible = bible_finding(service_id, "ROM 8:28", &[s1.id]);
        let context = build_context(service_id, vec![s0, s1], vec![sermon, bible], Vec::new());

        let engine = CrossDomainCorrelationEngine::new();
        let comparable =
            |cs: &[IntelligenceCorrelation]| -> Vec<(CorrelationKind, Vec<Uuid>, String, f32)> {
                cs.iter()
                    .map(|c| {
                        let mut ids = c.source_finding_ids.clone();
                        ids.sort_unstable();
                        (c.kind.clone(), ids, c.summary.clone(), c.confidence.score)
                    })
                    .collect()
            };
        let first = engine.analyze(&context);
        let second = engine.analyze(&context);
        assert_eq!(comparable(&first), comparable(&second));
    }

    // --- Scenario G: one rule panics -----------------------------------------

    fn panicking_rule(_ctx: &AnalysisContext) -> Vec<IntelligenceCorrelation> {
        panic!("simulated correlation rule panic");
    }

    #[test]
    fn scenario_g_a_panicking_rule_never_stops_the_others() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        let sermon = sermon_finding(service_id, "Supporting Scripture: ROM 8:28", &[s0.id]);
        let bible = bible_finding(service_id, "ROM 8:28", &[s1.id]);
        let context = build_context(service_id, vec![s0, s1], vec![sermon, bible], Vec::new());
        let ctx = AnalysisContext::build(&context);

        let rules: &[Rule] = &[panicking_rule, rule_scripture_sermon];
        // This call itself must not panic - that's the assertion.
        let produced = run_rules(&ctx, rules);
        assert!(
            produced
                .iter()
                .any(|c| c.kind == CorrelationKind::ScriptureSermon),
            "the real rule must still run after the panicking one"
        );
    }

    // --- Scenario H: disabled music dataset (no Music finding at all) ------

    #[test]
    fn scenario_h_no_music_finding_means_no_music_correlation() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let sermon = sermon_finding(service_id, "Transition: TEACHING -> PRAYER", &[s0.id]);
        // No music finding at all - simulates a disabled dataset producing
        // nothing for the Music engine to find in the first place.
        let context = build_context(service_id, vec![s0], vec![sermon], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(correlations
            .iter()
            .all(|c| !c.domains.contains(&IntelligenceDomain::Music)));
    }

    // --- Scenario J: rejected source finding --------------------------------

    #[test]
    fn scenario_j_a_rejected_finding_is_never_treated_as_accepted_evidence() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s1 = segment(1);
        let sermon = sermon_finding(service_id, "Supporting Scripture: ROM 8:28", &[s0.id]);
        let mut bible = bible_finding(service_id, "ROM 8:28", &[s1.id]);
        bible.status = FindingStatus::Rejected;
        let context = build_context(service_id, vec![s0, s1], vec![sermon, bible], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(
            correlations.is_empty(),
            "a rejected finding must never contribute to a correlation: {correlations:?}"
        );
    }

    // --- deduplication -------------------------------------------------------

    #[test]
    fn identical_correlations_are_deduplicated_within_one_analysis() {
        let mut all = vec![
            IntelligenceCorrelation::new(
                Uuid::nil(),
                vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                vec![IntelligenceDomain::Bible, IntelligenceDomain::Sermon],
                CorrelationKind::ScriptureSermon,
                AssertionLevel::Inferred,
                confidence(0.9, "x"),
                "a",
                RULE_SCRIPTURE_SERMON,
                RULE_VERSION,
            ),
            IntelligenceCorrelation::new(
                Uuid::nil(),
                vec![Uuid::from_u128(2), Uuid::from_u128(1)],
                vec![IntelligenceDomain::Bible, IntelligenceDomain::Sermon],
                CorrelationKind::ScriptureSermon,
                AssertionLevel::Inferred,
                confidence(0.4, "y"),
                "b",
                RULE_SCRIPTURE_SERMON,
                RULE_VERSION,
            ),
        ];
        dedup(&mut all);
        assert_eq!(
            all.len(),
            1,
            "same kind + same finding-id set must collapse to one"
        );
    }

    // --- ordering --------------------------------------------------------------

    #[test]
    fn sort_is_deterministic_and_never_depends_on_hash_map_order() {
        let mut all = vec![
            IntelligenceCorrelation::new(
                Uuid::nil(),
                vec![Uuid::from_u128(1)],
                vec![IntelligenceDomain::Sermon],
                CorrelationKind::TemporalProximity,
                AssertionLevel::Inferred,
                confidence(0.3, "low"),
                "low",
                RULE_TEMPORAL_ASSOCIATION,
                RULE_VERSION,
            ),
            IntelligenceCorrelation::new(
                Uuid::nil(),
                vec![Uuid::from_u128(2)],
                vec![IntelligenceDomain::Sermon],
                CorrelationKind::ScriptureSermon,
                AssertionLevel::Inferred,
                confidence(0.9, "high"),
                "high",
                RULE_SCRIPTURE_SERMON,
                RULE_VERSION,
            ),
        ];
        sort_deterministically(&mut all);
        assert_eq!(all[0].summary, "high", "higher confidence sorts first");
        assert_eq!(all[1].summary, "low");
    }

    // --- canonical full-service scenario (spec section 32) -------------------

    #[test]
    fn canonical_full_service_cross_domain_scenario() {
        let service_id = Uuid::new_v4();
        let seg_theme = segment(0);
        let seg_romans_context = segment(1);
        let seg_point1 = segment(2);
        let seg_verse28 = segment(3);
        let seg_worship = segment(4);
        let seg_song = segment(5);

        let theme = sermon_finding(service_id, "Theme: faith", &[seg_theme.id]);
        let romans_context = bible_finding(
            service_id,
            "Active Scripture Context: ROM 8",
            &[seg_romans_context.id],
        );
        let point1 = sermon_finding(service_id, "Supporting Scripture: ROM 8", &[seg_point1.id]);
        let verse28 = bible_finding(service_id, "ROM 8:28", &[seg_verse28.id]);
        let point1_verse_link = sermon_finding(
            service_id,
            "Supporting Scripture: ROM 8:28",
            &[seg_verse28.id],
        );
        let transition = sermon_finding(
            service_id,
            "Transition: TEACHING -> PRAYER",
            &[seg_worship.id],
        );
        let song = music_finding(service_id, "Test Fixture Hymn One", &[seg_song.id]);

        let context = build_context(
            service_id,
            vec![
                seg_theme,
                seg_romans_context,
                seg_point1,
                seg_verse28,
                seg_worship,
                seg_song,
            ],
            vec![
                theme.clone(),
                romans_context.clone(),
                point1.clone(),
                verse28.clone(),
                point1_verse_link.clone(),
                transition.clone(),
                song.clone(),
            ],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);

        // ScriptureSermon: point1 <-> romans_context (chapter match, Near).
        assert!(correlations.iter().any(|c| {
            c.kind == CorrelationKind::ScriptureSermon
                && c.source_finding_ids.contains(&point1.id)
                && c.source_finding_ids.contains(&romans_context.id)
        }));
        // ScriptureSermon: point1_verse_link <-> verse28 (exact match).
        let exact = correlations
            .iter()
            .find(|c| {
                c.kind == CorrelationKind::ScriptureSermon
                    && c.source_finding_ids.contains(&point1_verse_link.id)
                    && c.source_finding_ids.contains(&verse28.id)
            })
            .expect("expected an exact-reference ScriptureSermon correlation");
        assert_eq!(exact.confidence.score, 0.95);

        // SermonMusic: transition <-> song.
        assert!(correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::SermonMusic
                && c.source_finding_ids.contains(&transition.id)
                && c.source_finding_ids.contains(&song.id)));

        // No presentation-shaped anything: correlations carry no field
        // capable of representing a PresentationItem, and this whole crate
        // has no dependency on cip_core_presentation.
        assert!(correlations
            .iter()
            .all(|c| c.status == FindingStatus::Detected));

        // Deterministic ordering: confidence never decreases down the list.
        for pair in correlations.windows(2) {
            assert!(pair[0].confidence.score >= pair[1].confidence.score);
        }
    }

    /// Phase 2.8's own full-service walkthrough: Service enters Worship,
    /// a song is recognized alongside a worship-transition sermon signal
    /// (Service + Music + Sermon converge on the same moment), the sermon
    /// moves into its main point with a supporting Scripture reference to
    /// ROM 8:28 (a real Bible finding, matching the just-completed BSB
    /// production dataset milestone's reference format), and Content
    /// Intelligence has already queued a candidate from that main point.
    /// Every correlation asserted here traces to real, implemented
    /// evidence - this is not a claim that these four kinds always
    /// co-occur, only that they do when the evidence genuinely supports
    /// each one independently (spec: "ONLY emit relationships whose
    /// actual evidence meets the implemented rules").
    #[test]
    fn phase_2_8_canonical_full_service_walkthrough() {
        let service_id = Uuid::new_v4();
        let seg_worship = segment(0);
        let seg_teaching = segment(1);

        // Service enters Worship, a song is recognized, and a
        // worship-transition sermon signal all land on the same moment -
        // three distinct domains converging on one literal segment.
        let service_worship = service_finding(service_id, "Phase: Worship", &[seg_worship.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[seg_worship.id]);
        let transition = sermon_finding(
            service_id,
            "Transition: WORSHIP -> TEACHING",
            &[seg_worship.id],
        );

        // The sermon's main point, its explicit Scripture cross-link, and
        // the real Bible finding it supports.
        let main_point = sermon_finding(
            service_id,
            "Main Point: Trusting God during difficult seasons",
            &[seg_teaching.id],
        );
        let scripture_link = sermon_finding(
            service_id,
            "Supporting Scripture: ROM 8:28",
            &[seg_teaching.id],
        );
        let verse28 = bible_finding(service_id, "ROM 8:28", &[seg_teaching.id]);

        // Content Intelligence has already queued a candidate from the
        // main point - Cross-Domain never re-derives it, only reads it.
        let candidate = content_candidate_for(
            service_id,
            &main_point,
            "Teaching: Trusting God during difficult seasons",
        );

        let context = build_context_with_candidates(
            service_id,
            vec![seg_worship, seg_teaching],
            vec![
                service_worship.clone(),
                music.clone(),
                transition.clone(),
                main_point.clone(),
                scripture_link.clone(),
                verse28.clone(),
            ],
            Vec::new(),
            vec![candidate.clone()],
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);

        // ScriptureSermon: the explicit cross-link exactly matches the
        // real Bible finding.
        let scripture_sermon = correlations
            .iter()
            .find(|c| {
                c.kind == CorrelationKind::ScriptureSermon
                    && c.source_finding_ids.contains(&scripture_link.id)
                    && c.source_finding_ids.contains(&verse28.id)
            })
            .expect("expected ScriptureSermon: exact reference match");
        assert_eq!(scripture_sermon.confidence.score, 0.95);

        // SermonMusic: the transition signal shares the worship segment
        // with the recognized song.
        assert!(correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::SermonMusic
                && c.source_finding_ids.contains(&transition.id)
                && c.source_finding_ids.contains(&music.id)));

        // MultiDomainConvergence: Service + Music + Sermon all reference
        // the same worship-segment moment.
        let convergence = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::MultiDomainConvergence)
            .expect("expected a MultiDomainConvergence correlation");
        assert_eq!(convergence.domains.len(), 3);
        for id in [service_worship.id, music.id, transition.id] {
            assert!(convergence.source_finding_ids.contains(&id));
        }

        // SermonContent: the candidate (derived from the main point)
        // relates to the real Bible finding sharing the main point's
        // teaching-segment moment - never restated against its own parent.
        let sermon_content = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::SermonContent)
            .expect("expected a SermonContent correlation");
        assert!(sermon_content.source_finding_ids.contains(&candidate.id));
        assert!(sermon_content.source_finding_ids.contains(&verse28.id));
        assert!(!sermon_content.source_finding_ids.contains(&main_point.id));

        // No presentation side effect anywhere in this analysis, and every
        // correlation is honestly `Inferred` - never `Generated`.
        assert!(correlations
            .iter()
            .all(|c| c.status == FindingStatus::Detected
                && c.assertion_level == AssertionLevel::Inferred));
    }

    // --- theme-scripture / theme-music / service-transition rules ----------

    #[test]
    fn theme_scripture_fires_when_theme_and_scripture_are_near_but_not_when_far() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let s2 = segment(2);
        let s_far = segment(40);
        let theme = sermon_finding(service_id, "Theme: faith", &[s0.id]);
        let near_bible = bible_finding(service_id, "ROM 8:28", &[s2.id]);
        let far_bible = bible_finding(service_id, "JHN 3:16", &[s_far.id]);
        let context = build_context(
            service_id,
            vec![s0, s2, s_far],
            vec![theme.clone(), near_bible.clone(), far_bible.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::ThemeScripture
                && c.source_finding_ids.contains(&theme.id)
                && c.source_finding_ids.contains(&near_bible.id)));
        assert!(!correlations
            .iter()
            .any(|c| c.source_finding_ids.contains(&far_bible.id)));
    }

    #[test]
    fn theme_music_is_temporal_only_never_a_higher_confidence_than_sermon_music() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let theme = sermon_finding(service_id, "Theme: grace", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let context = build_context(
            service_id,
            vec![s0],
            vec![theme.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::ThemeMusic)
            .expect("expected a ThemeMusic correlation");
        assert!(hit.confidence.score < 0.85, "temporal-only theme/music evidence must never reach SermonMusic's shared-segment confidence");
    }

    #[test]
    fn service_transition_fires_near_a_matching_service_event_and_not_a_distant_one() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let conclusion = sermon_finding(
            service_id,
            "Possible Conclusion: let us walk by faith",
            &[s0.id],
        );
        let near_event = ServiceEventSummary {
            name: "SERVICE_ENDED".to_string(),
            occurred_at: conclusion.created_at + chrono::Duration::seconds(30),
        };
        let far_event = ServiceEventSummary {
            name: "SERVICE_PAUSED".to_string(),
            occurred_at: conclusion.created_at + chrono::Duration::seconds(600),
        };
        let context = build_context(
            service_id,
            vec![s0],
            vec![conclusion.clone()],
            vec![near_event, far_event],
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hits: Vec<_> = correlations
            .iter()
            .filter(|c| c.kind == CorrelationKind::ServiceTransition)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "only the near event should correlate: {hits:?}"
        );
    }

    // --- Phase 2.8: content candidates, multi-domain convergence, Service ----

    fn service_finding(service_id: Uuid, summary: &str, segments: &[Uuid]) -> IntelligenceFinding {
        finding(
            service_id,
            IntelligenceDomain::Service,
            FindingKind::ServiceState,
            AssertionLevel::Observed,
            summary,
            segments,
        )
    }

    fn content_candidate_for(
        service_id: Uuid,
        parent: &IntelligenceFinding,
        label: &str,
    ) -> ContentCandidate {
        use crate::content_candidate::ContentCandidateType;

        ContentCandidate::new(
            service_id,
            None,
            vec![parent.id],
            ContentCandidateType::Theme,
            label,
            label,
            AssertionLevel::Suggested,
            CR::new(0.8, CS::Heuristic, None),
            0.5,
            "sermon-content",
            "1.0",
        )
    }

    fn build_context_with_candidates(
        service_id: Uuid,
        segments: Vec<TranscriptSegment>,
        findings: Vec<IntelligenceFinding>,
        events: Vec<ServiceEventSummary>,
        candidates: Vec<ContentCandidate>,
    ) -> IntelligenceContext {
        build_context(service_id, segments, findings, events).with_content_candidates(candidates)
    }

    #[test]
    fn sermon_content_fires_at_immediate_proximity_and_never_correlates_with_its_own_parent() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let sermon = sermon_finding(service_id, "Theme: perseverance", &[s0.id]);
        let bible = bible_finding(service_id, "JAS 1:12", &[s0.id]);
        let candidate = content_candidate_for(service_id, &sermon, "Theme: perseverance");
        let context = build_context_with_candidates(
            service_id,
            vec![s0],
            vec![sermon.clone(), bible.clone()],
            Vec::new(),
            vec![candidate.clone()],
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::SermonContent)
            .expect("expected a SermonContent correlation");
        assert_eq!(hit.confidence.score, 0.65);
        assert!(hit.source_finding_ids.contains(&candidate.id));
        assert!(hit.source_finding_ids.contains(&bible.id));
        assert!(
            !hit.source_finding_ids.contains(&sermon.id),
            "a SermonContent correlation must never restate the candidate's own parent link as a discovery: {hit:?}"
        );
        assert_eq!(hit.assertion_level, AssertionLevel::Inferred);
    }

    #[test]
    fn sermon_content_fires_at_lower_confidence_when_only_near() {
        let service_id = Uuid::new_v4();
        let segments: Vec<TranscriptSegment> = (0..3).map(segment).collect();
        let sermon = sermon_finding(service_id, "Theme: perseverance", &[segments[0].id]);
        let bible = bible_finding(service_id, "JAS 1:12", &[segments[2].id]);
        let candidate = content_candidate_for(service_id, &sermon, "Theme: perseverance");
        let context = build_context_with_candidates(
            service_id,
            segments,
            vec![sermon, bible.clone()],
            Vec::new(),
            vec![candidate.clone()],
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::SermonContent)
            .expect("expected a SermonContent correlation at Near proximity");
        assert_eq!(hit.confidence.score, 0.45);
    }

    #[test]
    fn sermon_content_produces_nothing_when_the_parent_finding_is_not_in_context() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "JAS 1:12", &[s0.id]);
        // The candidate's `source_finding_ids` names a Sermon finding id
        // that never appears in `recent_findings` (e.g. aged out of the
        // bounded window) - the rule must not fabricate a parent.
        let orphan_id = Uuid::new_v4();
        let candidate = ContentCandidate::new(
            service_id,
            None,
            vec![orphan_id],
            crate::content_candidate::ContentCandidateType::Theme,
            "Theme: perseverance",
            "Theme: perseverance",
            AssertionLevel::Suggested,
            CR::new(0.8, CS::Heuristic, None),
            0.5,
            "sermon-content",
            "1.0",
        );
        let context = build_context_with_candidates(
            service_id,
            vec![s0],
            vec![bible],
            Vec::new(),
            vec![candidate],
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(
            !correlations
                .iter()
                .any(|c| c.kind == CorrelationKind::SermonContent),
            "no SermonContent correlation without a resolvable parent finding"
        );
    }

    #[test]
    fn sermon_content_never_fires_when_no_candidates_were_attached_to_the_context() {
        // A plain `build_context` (no `.with_content_candidates` call at
        // all) must behave exactly as it did before Phase 2.8 - proves the
        // additive-extension discipline holds for this rule specifically.
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let sermon = sermon_finding(service_id, "Theme: perseverance", &[s0.id]);
        let bible = bible_finding(service_id, "JAS 1:12", &[s0.id]);
        let context = build_context(service_id, vec![s0], vec![sermon, bible], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(!correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::SermonContent));
    }

    #[test]
    fn multi_domain_convergence_fires_when_three_domains_share_one_segment() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "ROM 8:28", &[s0.id]);
        let sermon = sermon_finding(
            service_id,
            "Main Point: God works all things for good",
            &[s0.id],
        );
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let context = build_context(
            service_id,
            vec![s0],
            vec![bible.clone(), sermon.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::MultiDomainConvergence)
            .expect("expected a MultiDomainConvergence correlation");
        assert_eq!(hit.confidence.score, 0.85);
        assert_eq!(hit.domains.len(), 3);
        for id in [bible.id, sermon.id, music.id] {
            assert!(hit.source_finding_ids.contains(&id));
        }
        assert_eq!(hit.assertion_level, AssertionLevel::Inferred);
    }

    #[test]
    fn multi_domain_convergence_scores_higher_with_a_fourth_domain() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "ROM 8:28", &[s0.id]);
        let sermon = sermon_finding(
            service_id,
            "Main Point: God works all things for good",
            &[s0.id],
        );
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let service = service_finding(service_id, "Phase: Sermon", &[s0.id]);
        let context = build_context(
            service_id,
            vec![s0],
            vec![
                bible.clone(),
                sermon.clone(),
                music.clone(),
                service.clone(),
            ],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| c.kind == CorrelationKind::MultiDomainConvergence)
            .expect("expected a MultiDomainConvergence correlation");
        assert_eq!(hit.confidence.score, 0.9);
        assert_eq!(hit.domains.len(), 4);
    }

    #[test]
    fn multi_domain_convergence_never_fires_for_only_two_domains() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "ROM 8:28", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let context = build_context(service_id, vec![s0], vec![bible, music], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(!correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::MultiDomainConvergence));
    }

    #[test]
    fn multi_domain_convergence_never_fires_for_near_but_not_shared_segments() {
        // Three domains, but each in its own segment (Near, not Immediate) -
        // convergence requires the literal same segment, never mere
        // proximity (that stays the weaker `TemporalProximity` fallback's
        // job).
        let service_id = Uuid::new_v4();
        let segments: Vec<TranscriptSegment> = (0..3).map(segment).collect();
        let bible = bible_finding(service_id, "ROM 8:28", &[segments[0].id]);
        let sermon = sermon_finding(
            service_id,
            "Main Point: God works all things for good",
            &[segments[1].id],
        );
        let music = music_finding(service_id, "Test Fixture Hymn One", &[segments[2].id]);
        let context = build_context(service_id, segments, vec![bible, sermon, music], Vec::new());

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        assert!(!correlations
            .iter()
            .any(|c| c.kind == CorrelationKind::MultiDomainConvergence));
    }

    #[test]
    fn service_domain_now_participates_in_the_temporal_fallback() {
        // Before Phase 2.8, `Service` was entirely excluded from
        // `rule_temporal_association`'s domain set - a Service finding
        // could never correlate with anything, not even at the weakest
        // tier. This proves the fix without inventing a dedicated
        // `ServiceMusic`/`ServiceScripture` kind (see the rule's own docs
        // for why that would be unjustified).
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let service = service_finding(service_id, "Phase: Worship", &[s0.id]);
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let context = build_context(
            service_id,
            vec![s0],
            vec![service.clone(), music.clone()],
            Vec::new(),
        );

        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        let hit = correlations
            .iter()
            .find(|c| {
                c.kind == CorrelationKind::TemporalProximity
                    && c.source_finding_ids.contains(&service.id)
                    && c.source_finding_ids.contains(&music.id)
            })
            .expect("Service and Music at Immediate proximity should now produce a TemporalProximity correlation");
        assert_eq!(hit.confidence.score, 0.35);
    }

    #[test]
    fn phase_2_8_analysis_is_deterministic_across_repeated_calls() {
        let service_id = Uuid::new_v4();
        let s0 = segment(0);
        let bible = bible_finding(service_id, "ROM 8:28", &[s0.id]);
        let sermon = sermon_finding(
            service_id,
            "Main Point: God works all things for good",
            &[s0.id],
        );
        let music = music_finding(service_id, "Test Fixture Hymn One", &[s0.id]);
        let candidate = content_candidate_for(service_id, &sermon, "Theme: perseverance");
        let context = build_context_with_candidates(
            service_id,
            vec![s0],
            vec![bible, sermon, music],
            Vec::new(),
            vec![candidate],
        );

        let engine = CrossDomainCorrelationEngine::new();
        let strip = |mut cs: Vec<IntelligenceCorrelation>| {
            for c in &mut cs {
                c.id = Uuid::nil();
            }
            cs
        };
        let first = strip(engine.analyze(&context));
        for _ in 0..10 {
            let repeat = strip(engine.analyze(&context));
            assert_eq!(first.len(), repeat.len());
            for (a, b) in first.iter().zip(repeat.iter()) {
                assert_eq!(a.kind, b.kind);
                assert_eq!(a.source_finding_ids, b.source_finding_ids);
                assert_eq!(a.confidence.score, b.confidence.score);
            }
        }
    }

    // --- CorrelationQueue operator workflow -----------------------------------

    #[test]
    fn correlation_queue_add_dedups_and_dismiss_removes_from_pending() {
        let service_id = Uuid::new_v4();
        let build = || {
            IntelligenceCorrelation::new(
                service_id,
                vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                vec![IntelligenceDomain::Bible, IntelligenceDomain::Sermon],
                CorrelationKind::ScriptureSermon,
                AssertionLevel::Inferred,
                confidence(0.9, "x"),
                "same relationship",
                RULE_SCRIPTURE_SERMON,
                RULE_VERSION,
            )
        };
        let mut queue = CorrelationQueue::new();
        assert_eq!(queue.add(build()), CorrelationQueueAddOutcome::Added);
        assert_eq!(
            queue.add(build()),
            CorrelationQueueAddOutcome::DuplicateIgnored
        );
        assert_eq!(queue.pending().len(), 1);

        let id = queue.pending()[0].id;
        queue.dismiss(id).unwrap();
        assert!(queue.pending().is_empty());
        assert_eq!(queue.get(id).unwrap().status, FindingStatus::Rejected);
        assert_eq!(
            queue.all().len(),
            1,
            "a dismissed correlation is still retrievable via all()"
        );
    }

    #[test]
    fn correlation_queue_unknown_id_reports_not_found() {
        let mut queue = CorrelationQueue::new();
        assert!(matches!(
            queue.dismiss(Uuid::new_v4()),
            Err(IntelligenceError::CorrelationNotFound(_))
        ));
    }

    // --- boundary / performance sanity (release-profile numbers measured
    // separately; this just proves correctness at scale in debug too) -----

    #[test]
    fn one_thousand_bounded_findings_never_panics_and_stays_correct() {
        let service_id = Uuid::new_v4();
        let mut segments = Vec::new();
        let mut findings = Vec::new();
        for i in 0..1000u64 {
            let seg = segment(i);
            let f = match i % 3 {
                0 => bible_finding(service_id, &format!("ROM {}:1", (i % 20) + 1), &[seg.id]),
                1 => sermon_finding(service_id, &format!("Theme: concept{}", i % 7), &[seg.id]),
                _ => music_finding(service_id, "Test Fixture Hymn One", &[seg.id]),
            };
            findings.push(f);
            segments.push(seg);
        }
        // IntelligenceContext bounds recent_findings/segments to their
        // configured defaults regardless of how many are passed in.
        let context = build_context(service_id, segments, findings, Vec::new());
        let correlations = CrossDomainCorrelationEngine::new().analyze(&context);
        // Must not panic, and must stay within a sane bound relative to
        // the (already-truncated) candidate pool - never literal O(1000^2).
        assert!(correlations.len() < 2000);
    }
}
