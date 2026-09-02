//! Church Knowledge Base (Phase 13) - read-only, cross-sermon aggregation
//! of Sermon Intelligence findings an operator has explicitly accepted.
//! Mirrors `harvest.rs`'s own discipline exactly: this module contains no
//! detection logic of its own, invents no title/label when one is absent,
//! and re-parses `IntelligenceFinding.summary` only through the single
//! `element_label_for_summary` function below, which uses the exact same
//! `summary.starts_with(...)` text-prefix convention `service.rs`,
//! `sermon_foundation.rs`, `sermon.rs`, and `pipeline.rs` already rely on
//! elsewhere in this codebase - see `docs/phase-13-audit.md` for the full
//! verification of that premise.
//!
//! Unlike `harvest.rs` (which reads the live, in-memory `FindingQueue` and
//! is therefore scoped to whatever is still resident in that Mutex), this
//! module reads only the durable `saved_sermon_findings` table - so it
//! genuinely spans services and survives a restart, at the cost of only
//! ever including findings an operator explicitly accepted (see
//! `commands::accept_sermon_finding`, the only place a row is written).

use crate::persistence;
use chrono::{DateTime, Utc};
use cip_core_intelligence::IntelligenceFinding;
use cip_core_sermon::foundation::Sermon;
use rusqlite::Connection;
use serde::Serialize;
use uuid::Uuid;

/// Bounds how much history a single knowledge-base build reads back -
/// generous enough to cover years of weekly services (52/year) without
/// being genuinely unbounded, mirroring `harvest.rs`'s own precedent.
const KNOWLEDGE_BASE_SERMON_LIMIT: u32 = 5_000;
const KNOWLEDGE_BASE_FINDING_LIMIT: u32 = 20_000;

/// A minimal, display-ready reference to one sermon - never the full
/// `Sermon` record, since the knowledge base only needs enough to label a
/// list entry, not to re-render every field.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonRef {
    pub sermon_id: Uuid,
    pub service_id: Uuid,
    pub title: Option<String>,
    pub speaker_name: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

impl SermonRef {
    fn from_sermon(sermon: &Sermon) -> Self {
        Self {
            sermon_id: sermon.id,
            service_id: sermon.service_id,
            title: sermon.title.clone(),
            speaker_name: sermon.speaker.as_ref().map(|s| s.name.clone()),
            started_at: sermon.started_at,
        }
    }
}

/// One distinct Theme label and every sermon it was attached to - "what
/// have we preached about, and how often." `occurrence_count` counts
/// every accepted Theme finding with this label; `sermons` deduplicates
/// by sermon (a theme mentioned twice in one sermon still counts as one
/// sermon in that list).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFrequencyEntry {
    pub label: String,
    pub occurrence_count: usize,
    pub sermon_count: usize,
    pub sermons: Vec<SermonRef>,
}

/// One speaker (as recorded on the durable `sermons` table itself, not
/// derived from findings) and the sermons they preached, most recent
/// first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerHistoryEntry {
    pub speaker_name: String,
    pub sermon_count: usize,
    pub sermons: Vec<SermonRef>,
}

/// The complete Church Knowledge Base bundle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonKnowledgeBase {
    /// Most-mentioned theme first, then alphabetical for a deterministic
    /// tiebreak.
    pub theme_frequency: Vec<ThemeFrequencyEntry>,
    /// Most sermons first, then alphabetical by speaker name.
    pub sermons_by_speaker: Vec<SpeakerHistoryEntry>,
    /// Every accepted sermon finding, newest first, up to
    /// `KNOWLEDGE_BASE_FINDING_LIMIT` - a simple browsable feed.
    pub recent_findings: Vec<IntelligenceFinding>,
    pub generated_at: DateTime<Utc>,
}

/// Derives a stable, human-readable element label from a finding's
/// `summary` using the text-prefix convention `sermon_adapter.rs`'s
/// `finding_for_*` functions already establish. Never panics on an
/// unrecognized summary - falls back to `"Other"` so a future taxonomy
/// addition in `core/sermon` degrades gracefully here instead of being
/// silently dropped or crashing this module.
pub fn element_label_for_summary(summary: &str) -> &'static str {
    const PREFIXES: &[(&str, &str)] = &[
        ("Theme: ", "Theme"),
        ("Takeaway: ", "Takeaway"),
        ("Food for Thought: ", "Food for Thought"),
        ("Main Point: ", "Main Point"),
        ("Sub Point: ", "Sub Point"),
        ("Scripture Reference: ", "Scripture Reference"),
        ("Scripture Quotation: ", "Scripture Quotation"),
        ("Supporting Scripture: ", "Supporting Scripture"),
        ("Definition: ", "Definition"),
        ("Key Statement: ", "Key Statement"),
        ("Declaration: ", "Declaration"),
        ("Question: ", "Question"),
        ("Illustration: ", "Illustration"),
        ("Story: ", "Story"),
        ("Example: ", "Example"),
        ("Application: ", "Application"),
        ("Prayer Point: ", "Prayer Point"),
        ("Summary: ", "Summary"),
        ("Reflection: ", "Reflection"),
        ("Transition: ", "Transition"),
        ("Conclusion: ", "Conclusion"),
    ];
    PREFIXES
        .iter()
        .find(|(prefix, _)| summary.starts_with(prefix))
        .map(|(_, label)| *label)
        .unwrap_or("Other")
}

/// Extracts the theme text itself from a `"Theme: {label}"` summary -
/// `None` for any other element label, since only Theme findings are
/// meaningful to group for `theme_frequency`.
fn theme_text(summary: &str) -> Option<&str> {
    summary.strip_prefix("Theme: ")
}

/// Pure aggregation over already-fetched rows - fully unit-testable
/// without a database. `sermons` and `findings` are expected to already
/// be bounded (see the `KNOWLEDGE_BASE_*_LIMIT` constants); this function
/// never queries anything itself.
pub fn build_knowledge_base(
    sermons: &[Sermon],
    findings: &[IntelligenceFinding],
) -> SermonKnowledgeBase {
    let sermon_by_id: std::collections::HashMap<Uuid, &Sermon> =
        sermons.iter().map(|s| (s.id, s)).collect();

    // --- theme_frequency ---------------------------------------------
    let mut theme_occurrences: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut theme_sermons: std::collections::HashMap<String, Vec<Uuid>> =
        std::collections::HashMap::new();
    for finding in findings {
        let Some(label) = theme_text(&finding.summary) else {
            continue;
        };
        *theme_occurrences.entry(label.to_string()).or_insert(0) += 1;
        if let Some(sermon_id) = finding.sermon_id {
            let list = theme_sermons.entry(label.to_string()).or_default();
            if !list.contains(&sermon_id) {
                list.push(sermon_id);
            }
        }
    }
    let mut theme_frequency: Vec<ThemeFrequencyEntry> = theme_occurrences
        .into_iter()
        .map(|(label, occurrence_count)| {
            let sermon_ids = theme_sermons.get(&label).cloned().unwrap_or_default();
            let mut sermons: Vec<SermonRef> = sermon_ids
                .iter()
                .filter_map(|id| sermon_by_id.get(id))
                .map(|s| SermonRef::from_sermon(s))
                .collect();
            sermons.sort_by_key(|s| std::cmp::Reverse(s.started_at));
            ThemeFrequencyEntry {
                sermon_count: sermons.len(),
                label,
                occurrence_count,
                sermons,
            }
        })
        .collect();
    theme_frequency.sort_by(|a, b| {
        b.occurrence_count
            .cmp(&a.occurrence_count)
            .then_with(|| a.label.cmp(&b.label))
    });

    // --- sermons_by_speaker --------------------------------------------
    let mut speaker_sermons: std::collections::HashMap<String, Vec<&Sermon>> =
        std::collections::HashMap::new();
    for sermon in sermons {
        if let Some(speaker) = sermon.speaker.as_ref() {
            speaker_sermons
                .entry(speaker.name.clone())
                .or_default()
                .push(sermon);
        }
    }
    let mut sermons_by_speaker: Vec<SpeakerHistoryEntry> = speaker_sermons
        .into_iter()
        .map(|(speaker_name, mut speaker_sermons)| {
            speaker_sermons.sort_by_key(|s| std::cmp::Reverse(s.started_at));
            SpeakerHistoryEntry {
                sermon_count: speaker_sermons.len(),
                sermons: speaker_sermons
                    .iter()
                    .map(|s| SermonRef::from_sermon(s))
                    .collect(),
                speaker_name,
            }
        })
        .collect();
    sermons_by_speaker.sort_by(|a, b| {
        b.sermon_count
            .cmp(&a.sermon_count)
            .then_with(|| a.speaker_name.cmp(&b.speaker_name))
    });

    // --- recent_findings -------------------------------------------------
    let mut recent_findings: Vec<IntelligenceFinding> = findings.to_vec();
    recent_findings.sort_by_key(|f| std::cmp::Reverse(f.created_at));

    SermonKnowledgeBase {
        theme_frequency,
        sermons_by_speaker,
        recent_findings,
        generated_at: Utc::now(),
    }
}

/// Reads the bounded, already-persisted sermon and saved-finding history
/// and assembles the knowledge base - the only function in this module
/// that touches the database.
pub fn get_knowledge_base(
    conn: &Connection,
) -> Result<SermonKnowledgeBase, persistence::PersistError> {
    let sermons = persistence::list_sermons(conn, KNOWLEDGE_BASE_SERMON_LIMIT)?;
    let findings = persistence::list_all_saved_sermon_findings(conn, KNOWLEDGE_BASE_FINDING_LIMIT)?;
    Ok(build_knowledge_base(&sermons, &findings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use cip_core_intelligence::{AssertionLevel, FindingKind, IntelligenceDomain};
    use cip_core_sermon::foundation::{Speaker, SpeakerRole};
    use cip_core_service::ServiceSession;
    use cip_database::{open_in_memory, run_migrations};

    fn open_test_db() -> Connection {
        let mut conn = open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn persist_test_service(conn: &Connection) -> Uuid {
        let session = ServiceSession::start("Test Service");
        persistence::persist_service(conn, &session).unwrap();
        session.id
    }

    fn sample_sermon(
        service_id: Uuid,
        title: &str,
        speaker_name: Option<&str>,
        started_at: DateTime<Utc>,
    ) -> Sermon {
        Sermon {
            id: Uuid::new_v4(),
            service_id,
            title: Some(title.to_string()),
            speaker: speaker_name.map(|name| Speaker {
                id: Uuid::new_v4(),
                name: name.to_string(),
                role: SpeakerRole::Primary,
            }),
            status: cip_core_sermon::foundation::SermonStatus::Ended,
            started_at: Some(started_at),
            ended_at: None,
            created_at: started_at,
        }
    }

    fn sample_finding(
        service_id: Uuid,
        sermon_id: Option<Uuid>,
        summary: &str,
        created_at: DateTime<Utc>,
    ) -> IntelligenceFinding {
        let mut finding = IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Sermon,
            FindingKind::Sermon,
            AssertionLevel::Observed,
            ConfidenceResult::new(0.85, ConfidenceSource::Heuristic, None),
            summary.to_string(),
            "test-engine",
            "1.0",
        );
        finding.sermon_id = sermon_id;
        finding.created_at = created_at;
        finding
    }

    #[test]
    fn element_label_for_summary_recognizes_every_known_prefix() {
        assert_eq!(element_label_for_summary("Theme: Waiting on God"), "Theme");
        assert_eq!(
            element_label_for_summary("Takeaway: Trust the process"),
            "Takeaway"
        );
        assert_eq!(
            element_label_for_summary("Food for Thought: What is your Ebenezer?"),
            "Food for Thought"
        );
        assert_eq!(
            element_label_for_summary("Key Statement: Faith is now"),
            "Key Statement"
        );
        assert_eq!(
            element_label_for_summary("Supporting Scripture: ROM 8:28"),
            "Supporting Scripture"
        );
    }

    #[test]
    fn element_label_for_summary_falls_back_to_other_without_panicking() {
        assert_eq!(element_label_for_summary("something unrecognized"), "Other");
        assert_eq!(element_label_for_summary(""), "Other");
    }

    #[test]
    fn theme_frequency_counts_occurrences_and_deduplicates_sermons() {
        let service_id = Uuid::new_v4();
        let sermon_a = sample_sermon(service_id, "Sermon A", None, Utc::now());
        let sermon_b = sample_sermon(service_id, "Sermon B", None, Utc::now() - Duration::days(7));
        let findings = vec![
            sample_finding(
                service_id,
                Some(sermon_a.id),
                "Theme: Waiting on God",
                Utc::now(),
            ),
            // Same theme mentioned twice in the same sermon - occurrence
            // count increases, sermon_count does not double-count it.
            sample_finding(
                service_id,
                Some(sermon_a.id),
                "Theme: Waiting on God",
                Utc::now(),
            ),
            sample_finding(
                service_id,
                Some(sermon_b.id),
                "Theme: Waiting on God",
                Utc::now(),
            ),
            sample_finding(
                service_id,
                Some(sermon_b.id),
                "Theme: Faith in Action",
                Utc::now(),
            ),
        ];

        let kb = build_knowledge_base(&[sermon_a.clone(), sermon_b.clone()], &findings);

        assert_eq!(kb.theme_frequency.len(), 2);
        let waiting = kb
            .theme_frequency
            .iter()
            .find(|t| t.label == "Waiting on God")
            .unwrap();
        assert_eq!(waiting.occurrence_count, 3);
        assert_eq!(
            waiting.sermon_count, 2,
            "must dedup by sermon, not by occurrence"
        );
        assert_eq!(
            kb.theme_frequency[0].label, "Waiting on God",
            "most-mentioned theme must sort first"
        );
    }

    #[test]
    fn theme_with_no_sermon_id_still_counts_as_an_occurrence_but_has_no_sermon() {
        let service_id = Uuid::new_v4();
        let findings = vec![sample_finding(
            service_id,
            None,
            "Theme: Orphaned Finding",
            Utc::now(),
        )];

        let kb = build_knowledge_base(&[], &findings);

        assert_eq!(kb.theme_frequency.len(), 1);
        assert_eq!(kb.theme_frequency[0].occurrence_count, 1);
        assert_eq!(
            kb.theme_frequency[0].sermon_count, 0,
            "a finding with no active sermon context cannot be attributed to one"
        );
    }

    #[test]
    fn non_theme_findings_never_appear_in_theme_frequency() {
        let service_id = Uuid::new_v4();
        let sermon = sample_sermon(service_id, "Sermon A", None, Utc::now());
        let findings = vec![sample_finding(
            service_id,
            Some(sermon.id),
            "Takeaway: Trust the process",
            Utc::now(),
        )];

        let kb = build_knowledge_base(&[sermon], &findings);

        assert!(
            kb.theme_frequency.is_empty(),
            "a Takeaway must never be counted as a Theme"
        );
        assert_eq!(
            kb.recent_findings.len(),
            1,
            "but it still appears in recent_findings"
        );
    }

    #[test]
    fn sermons_by_speaker_groups_and_sorts_by_sermon_count_then_name() {
        let service_id = Uuid::new_v4();
        let s1 = sample_sermon(service_id, "Sermon 1", Some("Pastor Ada"), Utc::now());
        let s2 = sample_sermon(
            service_id,
            "Sermon 2",
            Some("Pastor Ada"),
            Utc::now() - Duration::days(7),
        );
        let s3 = sample_sermon(service_id, "Sermon 3", Some("Guest Speaker"), Utc::now());
        let s4_no_speaker = sample_sermon(service_id, "Sermon 4", None, Utc::now());

        let kb = build_knowledge_base(&[s1, s2, s3, s4_no_speaker], &[]);

        assert_eq!(
            kb.sermons_by_speaker.len(),
            2,
            "a sermon with no recorded speaker must be excluded, never fabricated"
        );
        assert_eq!(kb.sermons_by_speaker[0].speaker_name, "Pastor Ada");
        assert_eq!(kb.sermons_by_speaker[0].sermon_count, 2);
        assert_eq!(kb.sermons_by_speaker[1].speaker_name, "Guest Speaker");
    }

    #[test]
    fn recent_findings_sorted_newest_first() {
        let service_id = Uuid::new_v4();
        let older = sample_finding(
            service_id,
            None,
            "Takeaway: older",
            Utc::now() - Duration::days(1),
        );
        let newer = sample_finding(service_id, None, "Takeaway: newer", Utc::now());

        let kb = build_knowledge_base(&[], &[older, newer]);

        assert_eq!(kb.recent_findings[0].summary, "Takeaway: newer");
        assert_eq!(kb.recent_findings[1].summary, "Takeaway: older");
    }

    #[test]
    fn get_knowledge_base_reads_back_persisted_sermons_and_saved_findings() {
        let conn = open_test_db();
        let service_id = persist_test_service(&conn);
        let sermon = sample_sermon(
            service_id,
            "The Waiting Season",
            Some("Pastor Ada"),
            Utc::now(),
        );
        persistence::persist_sermon(&conn, &sermon).unwrap();

        let finding = sample_finding(
            service_id,
            Some(sermon.id),
            "Theme: Waiting on God",
            Utc::now(),
        );
        persistence::persist_saved_sermon_finding(
            &conn,
            &finding,
            element_label_for_summary(&finding.summary),
        )
        .unwrap();

        let kb = get_knowledge_base(&conn).unwrap();

        assert_eq!(kb.theme_frequency.len(), 1);
        assert_eq!(kb.theme_frequency[0].label, "Waiting on God");
        assert_eq!(kb.sermons_by_speaker.len(), 1);
        assert_eq!(kb.recent_findings.len(), 1);
    }

    #[test]
    fn empty_database_produces_an_empty_but_valid_knowledge_base() {
        let conn = open_test_db();
        let kb = get_knowledge_base(&conn).unwrap();
        assert!(kb.theme_frequency.is_empty());
        assert!(kb.sermons_by_speaker.is_empty());
        assert!(kb.recent_findings.is_empty());
    }
}
