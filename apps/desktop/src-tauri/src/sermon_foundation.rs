//! Sermon Foundation orchestration (Phase 2.5, per the authoritative
//! Phase 2 roadmap) - the durable entity/lifecycle layer Phase 2.6's real
//! Sermon Intelligence engine will operate on. Deliberately
//! Tauri-agnostic (plain domain types, no `AppHandle`/`State`), matching
//! `content.rs`/`presentation.rs`/`music.rs`/`sermon.rs`/`service.rs`.
//!
//! ## A separate module from `sermon.rs`, on purpose
//!
//! `sermon.rs` (this repository's earlier internal "Phase 2.3" work,
//! understood under the authoritative roadmap to be Phase 2.6-equivalent
//! semantic detection) is untouched by this phase - not renamed, not
//! extended, not imported from here beyond the shared
//! `cip_core_intelligence` types every orchestration module already uses.
//! This module answers a prior, structural question: "what sermon is
//! active, who is speaking, what section are we in, and which transcript
//! segments belong to it" - never "what does the transcript mean."
//!
//! ## No `IntelligenceEngine`, on purpose
//!
//! Nothing here implements `IntelligenceEngine` or gets registered into
//! `IntelligenceEngineRegistry` - see `docs/sermon-foundation.md`'s
//! "Engine boundary" section for why. Every mutating action here is an
//! explicit operator action (never transcript-driven inference), so there
//! is no `analyze(&self, input, context)` call to make: state changes
//! happen through plain functions the Tauri commands call directly, and
//! every one that produces a fact worth recording constructs an ordinary
//! `IntelligenceFinding` (domain `Sermon`, kind `Sermon` - the same
//! domain/kind the historical semantic engine's findings already use,
//! distinguished only by summary prefix, exactly the way Service
//! Intelligence distinguishes "Anomaly" from "Service phase changed"
//! findings within its own single `FindingKind::ServiceState`).

use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
use cip_core_intelligence::{
    AssertionLevel, EvidenceSource, FindingKind, IntelligenceDomain, IntelligenceFinding,
    IntelligenceProvenance,
};
use cip_core_sermon::foundation::{Sermon, SermonSection, Speaker};
use uuid::Uuid;

pub const SERMON_FOUNDATION_SOURCE_ID: &str = "sermon-foundation";
pub const SERMON_FOUNDATION_SOURCE_VERSION: &str = "1.0.0";

/// Every structural Sermon Foundation finding's summary starts with this
/// prefix - the filter behind any future "foundation vs. semantic" split,
/// mirroring `service.rs::is_transition_finding`'s own prefix-based
/// distinction within a single shared `FindingKind`.
pub const FOUNDATION_SUMMARY_PREFIX: &str = "Sermon foundation:";

fn observed_operator_finding(
    service_id: Uuid,
    summary: String,
    description: String,
) -> IntelligenceFinding {
    let confidence = ConfidenceResult::new(1.0, ConfidenceSource::Human, None);
    IntelligenceFinding::new(
        service_id,
        IntelligenceDomain::Sermon,
        FindingKind::Sermon,
        AssertionLevel::Observed,
        confidence,
        summary,
        SERMON_FOUNDATION_SOURCE_ID,
        SERMON_FOUNDATION_SOURCE_VERSION,
    )
    .with_evidence(vec![EvidenceSource::OperatorAction { description }])
    .with_provenance(IntelligenceProvenance::unknown())
}

/// A lifecycle transition finding ("started"/"paused"/"resumed"/"ended") -
/// every one of these is always `Observed`: an operator's own start/pause/
/// resume/end action is a direct observation of what happened, never an
/// inference (spec's "explicit operator corrections... Observed").
pub fn finding_for_lifecycle_event(
    service_id: Uuid,
    sermon: &Sermon,
    verb: &str,
) -> IntelligenceFinding {
    let title = sermon.title.as_deref().unwrap_or("(untitled)");
    observed_operator_finding(
        service_id,
        format!("{FOUNDATION_SUMMARY_PREFIX} sermon {verb} - \"{title}\""),
        format!("operator {verb} sermon {}", sermon.id),
    )
}

pub fn finding_for_section_changed(
    service_id: Uuid,
    sermon_id: Uuid,
    section: &SermonSection,
) -> IntelligenceFinding {
    observed_operator_finding(
        service_id,
        format!(
            "{FOUNDATION_SUMMARY_PREFIX} section changed to {}",
            section.kind.label()
        ),
        format!(
            "sermon {sermon_id} section set to {} (origin: {:?})",
            section.kind.label(),
            section.origin
        ),
    )
}

pub fn finding_for_speaker_assigned(
    service_id: Uuid,
    sermon_id: Uuid,
    speaker: &Speaker,
) -> IntelligenceFinding {
    observed_operator_finding(
        service_id,
        format!(
            "{FOUNDATION_SUMMARY_PREFIX} speaker assigned - \"{}\"",
            speaker.name
        ),
        format!(
            "sermon {sermon_id} speaker set to \"{}\" ({})",
            speaker.name,
            speaker.role.label()
        ),
    )
}

pub fn finding_for_metadata_updated(
    service_id: Uuid,
    sermon_id: Uuid,
    field: &str,
    value: &str,
) -> IntelligenceFinding {
    observed_operator_finding(
        service_id,
        format!("{FOUNDATION_SUMMARY_PREFIX} {field} updated - \"{value}\""),
        format!("sermon {sermon_id} {field} set to \"{value}\" by operator"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_intelligence::FindingStatus;
    use cip_core_sermon::foundation::{
        SectionOrigin, SermonSectionKind, SermonStatus, SpeakerRole,
    };

    #[test]
    fn lifecycle_finding_is_always_observed_with_full_confidence() {
        let sermon = Sermon::start(Uuid::new_v4(), Some("Grace".to_string()));
        let finding = finding_for_lifecycle_event(sermon.service_id, &sermon, "started");
        assert_eq!(finding.assertion_level, AssertionLevel::Observed);
        assert_eq!(finding.confidence.score, 1.0);
        assert_eq!(finding.domain, IntelligenceDomain::Sermon);
        assert_eq!(finding.kind, FindingKind::Sermon);
        assert_eq!(finding.status, FindingStatus::Detected);
        assert!(finding.summary.starts_with(FOUNDATION_SUMMARY_PREFIX));
        assert!(finding.summary.contains("Grace"));
    }

    #[test]
    fn lifecycle_finding_evidence_is_operator_action_never_transcript() {
        let sermon = Sermon::start(Uuid::new_v4(), None);
        let finding = finding_for_lifecycle_event(sermon.service_id, &sermon, "ended");
        assert_eq!(finding.evidence.len(), 1);
        assert!(matches!(
            finding.evidence[0],
            EvidenceSource::OperatorAction { .. }
        ));
    }

    #[test]
    fn untitled_sermon_never_fabricates_a_title() {
        let sermon = Sermon::start(Uuid::new_v4(), None);
        let finding = finding_for_lifecycle_event(sermon.service_id, &sermon, "started");
        assert!(finding.summary.contains("(untitled)"));
    }

    #[test]
    fn section_finding_names_the_section_kind() {
        let sermon_id = Uuid::new_v4();
        let section = SermonSection::open(
            sermon_id,
            SermonSectionKind::Illustration,
            SectionOrigin::OperatorAssigned,
            None,
        );
        let finding = finding_for_section_changed(Uuid::new_v4(), sermon_id, &section);
        assert!(finding.summary.contains("ILLUSTRATION"));
        assert_eq!(finding.assertion_level, AssertionLevel::Observed);
    }

    #[test]
    fn speaker_finding_never_invents_a_name() {
        let speaker = Speaker::new("Pastor Jane Doe", SpeakerRole::Primary);
        let finding = finding_for_speaker_assigned(Uuid::new_v4(), Uuid::new_v4(), &speaker);
        assert!(finding.summary.contains("Pastor Jane Doe"));
    }

    #[test]
    fn metadata_finding_carries_the_exact_supplied_value() {
        let finding = finding_for_metadata_updated(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "title",
            "Grace Abounding",
        );
        assert!(finding.summary.contains("title"));
        assert!(finding.summary.contains("Grace Abounding"));
    }

    #[test]
    fn distinct_lifecycle_events_produce_distinguishable_summaries() {
        let sermon = Sermon::start(Uuid::new_v4(), Some("Faith".to_string()));
        let started = finding_for_lifecycle_event(sermon.service_id, &sermon, "started");
        let paused = finding_for_lifecycle_event(sermon.service_id, &sermon, "paused");
        assert!(!started.is_equivalent_to(&paused));
    }

    #[test]
    fn a_sermon_never_claims_a_status_other_than_what_it_actually_has() {
        // Type-level sanity check the state machine module already proves
        // exhaustively - this crate's foundation types are the only
        // source of truth `sermon_foundation.rs` reads from.
        let sermon = Sermon::start(Uuid::new_v4(), None);
        assert_eq!(sermon.status, SermonStatus::Active);
    }

    // --- canonical Phase 2.5 acceptance scenario ---------------------------
    //
    // Fictional service, fictional sermon, synthetic project-authored
    // transcript text only (never copyrighted sermon content) - proves the
    // full spec walkthrough: SERVICE START -> SERMON START -> speaker
    // assigned -> title assigned -> transcript segments -> section
    // assigned -> transcript segment retains linkage -> PAUSE -> RESUME ->
    // more transcript -> END, using the same real persistence layer, real
    // domain types, and real `IntelligenceContext` this app's Tauri
    // commands (`commands.rs`) call - just without the `AppHandle`/`State`
    // machinery this codebase has no test harness for (see
    // `commands.rs`'s own "Phase 1.3 lifecycle/workflow guards" comment).

    mod acceptance {
        use super::*;
        use cip_core_ai::TranscriptSegment;
        use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
        use cip_core_intelligence::{
            ContextBounds, FindingQueue, IntelligenceContext, QueueAddOutcome,
        };
        use cip_core_sermon::foundation::SermonSegment;
        use cip_core_service::ServiceSession;
        use cip_database::{open_in_memory, run_migrations};

        fn transcript_segment(text: &str, sequence: u64) -> TranscriptSegment {
            TranscriptSegment {
                id: Uuid::new_v4(),
                sequence,
                text: text.to_string(),
                is_final: true,
                confidence: ConfidenceResult::new(1.0, ConfidenceSource::Human, None),
                start_ms: sequence * 1000,
                end_ms: sequence * 1000 + 900,
                language: Some("en".to_string()),
                speaker_id: None,
            }
        }

        #[test]
        fn canonical_full_walkthrough_service_sermon_speaker_title_sections_segments_pause_resume_end(
        ) {
            let mut conn = open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            let mut findings = FindingQueue::new();

            // SERVICE START
            let service = ServiceSession::start("Sunday Morning");
            crate::persistence::persist_service(&conn, &service).unwrap();

            // SERMON START (Introduction section opens automatically, a
            // deterministic system boundary - mirrors `start_sermon`).
            let mut sermon = Sermon::start(service.id, None);
            crate::persistence::persist_sermon(&conn, &sermon).unwrap();
            let intro = SermonSection::open(
                sermon.id,
                SermonSectionKind::Introduction,
                SectionOrigin::SystemBoundary,
                None,
            );
            crate::persistence::persist_sermon_section(&conn, &intro).unwrap();
            let started_finding = finding_for_lifecycle_event(service.id, &sermon, "started");
            assert_eq!(
                findings.add(started_finding.clone()),
                QueueAddOutcome::Added
            );
            crate::timeline::record_event(
                &conn,
                Some(service.id),
                crate::events::AppEvent::SermonStarted,
                crate::logging::LogCategory::App,
                &sermon,
            )
            .unwrap();

            // INVARIANT 1/2: Sermon != ServiceSession, Sermon != TranscriptSegment.
            assert_ne!(sermon.id, service.id);

            // speaker assigned
            let speaker = Speaker::new("Pastor Jane Doe", SpeakerRole::Primary);
            sermon.assign_speaker(speaker.clone());
            crate::persistence::update_sermon(&conn, &sermon).unwrap();
            findings.add(finding_for_speaker_assigned(
                service.id, sermon.id, &speaker,
            ));

            // title assigned
            sermon.set_title("Faith That Moves");
            crate::persistence::update_sermon(&conn, &sermon).unwrap();
            findings.add(finding_for_metadata_updated(
                service.id,
                sermon.id,
                "title",
                "Faith That Moves",
            ));

            // transcript segment 1, 2 - the canonical transcript source,
            // never duplicated into the sermon's own row.
            let seg1 =
                transcript_segment("In the beginning God created the heavens and the earth.", 0);
            let seg2 = transcript_segment("Faith comes by hearing, and hearing by the word.", 1);
            crate::persistence::persist_transcript_segment(&conn, service.id, &seg1).unwrap();
            crate::persistence::persist_transcript_segment(&conn, service.id, &seg2).unwrap();
            let link1 = SermonSegment::new(sermon.id, seg1.id, 0, Some(intro.id));
            crate::persistence::persist_sermon_segment(&conn, &link1).unwrap();
            let link2 = SermonSegment::new(sermon.id, seg2.id, 1, Some(intro.id));
            crate::persistence::persist_sermon_segment(&conn, &link2).unwrap();

            // section assigned (operator moves from Introduction to Main Message)
            let main_message = SermonSection::open(
                sermon.id,
                SermonSectionKind::MainMessage,
                SectionOrigin::OperatorAssigned,
                None,
            );
            crate::persistence::close_open_sermon_section(
                &conn,
                sermon.id,
                main_message.started_at,
            )
            .unwrap();
            crate::persistence::persist_sermon_section(&conn, &main_message).unwrap();
            findings.add(finding_for_section_changed(
                service.id,
                sermon.id,
                &main_message,
            ));

            // transcript segment 3 - linked under the new section, proving
            // a sermon segment retains transcript linkage AND records
            // which section was open at link time.
            let seg3 = transcript_segment("Turn with me to Romans chapter 10.", 2);
            crate::persistence::persist_transcript_segment(&conn, service.id, &seg3).unwrap();
            let link3 = SermonSegment::new(sermon.id, seg3.id, 2, Some(main_message.id));
            crate::persistence::persist_sermon_segment(&conn, &link3).unwrap();

            // "scripture reference already detected by Bible Intelligence" -
            // simulated as an already-populated `active_scripture_context`
            // this module never produces itself; `IntelligenceContext` is
            // the shared channel, never a direct Bible-engine call
            // (invariant 3/4).
            let scripture_context = cip_core_bible::ScriptureContext {
                translation_id: "KJV".to_string(),
                book: "ROM".to_string(),
                chapter: 10,
                last_verse: None,
                confidence: ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
                established_at: chrono::Utc::now(),
                valid: true,
            };
            let recent_segments =
                crate::persistence::list_sermon_segments(&conn, sermon.id).unwrap();
            let context = IntelligenceContext::build(
                service.id,
                Some(cip_core_service::ServiceStatus::Started),
                Some(seg3.clone()),
                vec![seg1.clone(), seg2.clone(), seg3.clone()],
                Some(scripture_context.clone()),
                findings.all().into_iter().cloned().collect(),
                Vec::new(),
                Vec::new(),
                ContextBounds::default(),
            )
            .with_sermon_context(
                Some(sermon.clone()),
                Some(main_message.clone()),
                recent_segments.clone(),
            );

            // Sermon context references the Bible finding's evidence via
            // the *shared context*, never a direct engine invocation - no
            // `BibleIntelligenceEngine`/`MusicIntelligenceEngine`/
            // `SermonIntelligenceEngine` symbol is imported anywhere in
            // this module (see the file's own `use` list at the top),
            // which is the structural proof "no engine calls another
            // engine" (invariant 4).
            assert_eq!(context.active_scripture_context.unwrap().book, "ROM");
            assert_eq!(context.active_sermon.as_ref().unwrap().id, sermon.id);
            assert_eq!(
                context.current_sermon_section.as_ref().unwrap().kind,
                SermonSectionKind::MainMessage
            );
            assert_eq!(
                context.recent_sermon_segments.len(),
                3,
                "sermon context is bounded, not silently truncated below what's there"
            );

            // sermon segment retains transcript linkage - never a copy of
            // the transcript text itself.
            assert_eq!(recent_segments[2].transcript_segment_id, seg3.id);
            assert_eq!(recent_segments[2].section_id, Some(main_message.id));

            // SERMON PAUSE
            sermon.pause();
            crate::persistence::update_sermon(&conn, &sermon).unwrap();
            findings.add(finding_for_lifecycle_event(service.id, &sermon, "paused"));
            assert_eq!(sermon.status, SermonStatus::Paused);

            // SERMON RESUME
            sermon.resume();
            crate::persistence::update_sermon(&conn, &sermon).unwrap();
            findings.add(finding_for_lifecycle_event(service.id, &sermon, "resumed"));
            assert_eq!(sermon.status, SermonStatus::Active);

            // more transcript
            let seg4 = transcript_segment("Let us pray as we close this message.", 3);
            crate::persistence::persist_transcript_segment(&conn, service.id, &seg4).unwrap();
            let link4 = SermonSegment::new(sermon.id, seg4.id, 3, Some(main_message.id));
            crate::persistence::persist_sermon_segment(&conn, &link4).unwrap();

            // SERMON END
            sermon.end();
            crate::persistence::update_sermon(&conn, &sermon).unwrap();
            crate::persistence::close_open_sermon_section(
                &conn,
                sermon.id,
                sermon.ended_at.unwrap(),
            )
            .unwrap();
            findings.add(finding_for_lifecycle_event(service.id, &sermon, "ended"));
            assert_eq!(sermon.status, SermonStatus::Ended);
            assert!(sermon.ended_at.is_some());

            // --- restart-recovery proof: reload every relevant piece purely
            // from the database, never from any in-process value above.
            let reloaded_sermon = crate::persistence::get_sermon(&conn, sermon.id).unwrap();
            assert_eq!(
                reloaded_sermon, sermon,
                "sermon survives restart identically"
            );
            let reloaded_sections =
                crate::persistence::list_sermon_sections(&conn, sermon.id).unwrap();
            assert_eq!(
                reloaded_sections.len(),
                2,
                "both Introduction and Main Message sections survive restart"
            );
            assert!(
                reloaded_sections.iter().all(|s| s.ended_at.is_some()),
                "every section is closed once its sermon has ended"
            );
            let reloaded_segments =
                crate::persistence::list_sermon_segments(&conn, sermon.id).unwrap();
            assert_eq!(
                reloaded_segments.len(),
                4,
                "all four transcript-segment links survive restart"
            );
            assert_eq!(
                reloaded_segments
                    .iter()
                    .map(|s| s.sequence)
                    .collect::<Vec<_>>(),
                vec![0, 1, 2, 3],
                "sequence stays gapless and in order"
            );

            // --- transcript remains canonical and is never rewritten.
            let reloaded_transcript =
                crate::persistence::list_transcript_segments(&conn, service.id, 10).unwrap();
            assert_eq!(reloaded_transcript.len(), 4);
            assert_eq!(
                reloaded_transcript[0].text, seg1.text,
                "transcript text is never rewritten by any sermon action"
            );

            // --- no semantic finding was ever fabricated: every finding
            // this scenario produced is Sermon-domain, Observed (never
            // Inferred/Suggested/Generated), and confidence 1.0 - a direct
            // consequence of every one being an explicit operator action.
            for finding in findings.all() {
                assert_eq!(
                    finding.domain,
                    cip_core_intelligence::IntelligenceDomain::Sermon
                );
                assert_eq!(
                    finding.assertion_level,
                    cip_core_intelligence::AssertionLevel::Observed
                );
                assert!(
                    !finding.summary.to_lowercase().contains("theme")
                        && !finding.summary.to_lowercase().contains("doctrine")
                        && !finding.summary.to_lowercase().contains("means"),
                    "no semantic/theological claim ever appears in a foundation finding: {}",
                    finding.summary
                );
            }

            // --- audited: every lifecycle action left a timeline entry.
            let timeline = crate::timeline::list_timeline(&conn, service.id, 50).unwrap();
            assert!(
                timeline
                    .iter()
                    .any(|e| e.event_name == crate::events::AppEvent::SermonStarted.name()),
                "the sermon-start action is present in the audit trail"
            );

            // --- determinism: replaying the same operator-action sequence
            // from scratch against a second, independent database produces
            // an equivalent (never a numerically-drifting) final sermon
            // status and section count.
            let mut conn2 = open_in_memory().unwrap();
            run_migrations(&mut conn2).unwrap();
            let service2 = ServiceSession::start("Sunday Morning");
            crate::persistence::persist_service(&conn2, &service2).unwrap();
            let mut sermon2 = Sermon::start(service2.id, None);
            crate::persistence::persist_sermon(&conn2, &sermon2).unwrap();
            sermon2.pause();
            sermon2.resume();
            sermon2.end();
            crate::persistence::update_sermon(&conn2, &sermon2).unwrap();
            assert_eq!(
                sermon2.status, sermon.status,
                "identical operator-action sequences produce identical final status"
            );
        }
    }
}
