//! [`Sermon`]: the message-within-a-service entity Phase 2.5 introduces -
//! see this crate's `foundation` module docs for why this is a separate
//! question from `cip_core_service::ServiceStatus`/`ServiceSession`.
//!
//! A `ServiceSession` answers "which church service is currently
//! happening" - one service may contain worship, announcements, offering,
//! prayer, a sermon, an altar call, and a closing, none of which a
//! `ServiceSession` distinguishes on its own. `Sermon` answers "which
//! message within that service is being delivered" - a strictly smaller,
//! optional-in-time-and-number span inside a service's lifetime.
//!
//! ## Plain data contract, validated by a separate pure function
//!
//! Mirrors `cip_core_service::ServiceSession` exactly: `Sermon`'s own
//! mutating methods (`activate`/`pause`/`resume`/`end`/`cancel`) do not
//! themselves reject an invalid call - transition *validation* is
//! [`is_valid_transition`], a separate, directly-testable pure function,
//! called by the orchestration layer
//! (`apps/desktop/src-tauri/src/sermon_foundation.rs`) before ever calling
//! one of these methods, the same division of responsibility
//! `commands::ensure_service_status` already uses for `ServiceSession`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::foundation::speaker::Speaker;

/// The message lifecycle - deliberately a separate state machine from
/// `cip_core_service::ServiceStatus` (never reused/aliased for this
/// purpose, per this phase's own non-negotiable rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SermonStatus {
    /// Scheduled/named but not yet delivering - not reachable from any
    /// Tauri command in this phase (nothing yet needs "schedule a sermon
    /// ahead of time"), but modeled and tested here so a future phase can
    /// add that workflow without redesigning this enum. See
    /// `docs/sermon-foundation.md`'s "NOT AVAILABLE" section.
    Planned,
    Active,
    Paused,
    Ended,
    Cancelled,
}

impl SermonStatus {
    pub const fn label(self) -> &'static str {
        match self {
            SermonStatus::Planned => "PLANNED",
            SermonStatus::Active => "ACTIVE",
            SermonStatus::Paused => "PAUSED",
            SermonStatus::Ended => "ENDED",
            SermonStatus::Cancelled => "CANCELLED",
        }
    }
}

/// Whether `to` is a legal next state from `from` (spec's "SERMON STATE
/// MACHINE" section) - a same-state call (e.g. `Active` -> `Active`) is
/// deliberately **not** valid here; the orchestration layer's own guard
/// treats "already in that state" as its own distinct error, exactly the
/// way `commands::ensure_service_status` already does for
/// `pause_service`/`resume_service`. `Ended`/`Cancelled` are terminal - no
/// transition out of either is ever valid, including back to `Active`
/// ("never silently mutate state" / "Ended -> Active" is explicitly
/// disallowed by the spec).
pub fn is_valid_transition(from: SermonStatus, to: SermonStatus) -> bool {
    use SermonStatus::*;
    matches!(
        (from, to),
        (Planned, Active)
            | (Planned, Cancelled)
            | (Active, Paused)
            | (Active, Ended)
            | (Paused, Active)
            | (Paused, Ended)
    )
}

/// A single message/sermon delivered within one [`cip_core_service::ServiceSession`].
/// `title`/`speaker` are `None` until an operator explicitly supplies them -
/// never guessed, never defaulted to a placeholder (spec's "unknown
/// metadata remains unknown" invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sermon {
    pub id: Uuid,
    pub service_id: Uuid,
    pub title: Option<String>,
    pub speaker: Option<Speaker>,
    pub status: SermonStatus,
    /// Set the moment the sermon becomes `Active` for the first time -
    /// `None` while still `Planned`.
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Sermon {
    /// A scheduled-but-not-yet-delivering sermon - see [`SermonStatus::Planned`]'s
    /// own docs for why this exists but has no current Tauri command.
    pub fn planned(service_id: Uuid, title: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            service_id,
            title,
            speaker: None,
            status: SermonStatus::Planned,
            started_at: None,
            ended_at: None,
            created_at: Utc::now(),
        }
    }

    /// The real operator workflow's entry point - "start sermon" begins
    /// delivering immediately, exactly mirroring
    /// `cip_core_service::ServiceSession::start`'s own "no separate
    /// planning step" convention.
    pub fn start(service_id: Uuid, title: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            service_id,
            title,
            speaker: None,
            status: SermonStatus::Active,
            started_at: Some(now),
            ended_at: None,
            created_at: now,
        }
    }

    /// `Planned` -> `Active`. Sets `started_at` if not already set (a
    /// sermon activated from `Planned` has never been active before, so
    /// this is always the first activation for it).
    pub fn activate(&mut self) {
        self.status = SermonStatus::Active;
        if self.started_at.is_none() {
            self.started_at = Some(Utc::now());
        }
    }

    pub fn pause(&mut self) {
        self.status = SermonStatus::Paused;
    }

    pub fn resume(&mut self) {
        self.status = SermonStatus::Active;
    }

    pub fn end(&mut self) {
        self.status = SermonStatus::Ended;
        self.ended_at = Some(Utc::now());
    }

    pub fn cancel(&mut self) {
        self.status = SermonStatus::Cancelled;
    }

    /// Explicit operator metadata correction - `None`/empty is never
    /// invented; this is the only way `title` changes after creation.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    pub fn assign_speaker(&mut self, speaker: Speaker) {
        self.speaker = Some(speaker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundation::speaker::SpeakerRole;

    #[test]
    fn start_creates_an_active_sermon_with_started_at_set() {
        let service_id = Uuid::new_v4();
        let sermon = Sermon::start(service_id, Some("Faith That Moves".to_string()));
        assert_eq!(sermon.service_id, service_id);
        assert_eq!(sermon.status, SermonStatus::Active);
        assert!(sermon.started_at.is_some());
        assert!(sermon.ended_at.is_none());
        assert_eq!(sermon.title.as_deref(), Some("Faith That Moves"));
    }

    #[test]
    fn a_sermon_is_never_the_same_identity_as_its_service() {
        let service_id = Uuid::new_v4();
        let sermon = Sermon::start(service_id, None);
        assert_ne!(
            sermon.id, sermon.service_id,
            "Sermon != ServiceSession (invariant 1/2)"
        );
    }

    #[test]
    fn planned_sermon_has_no_started_at_until_activated() {
        let mut sermon = Sermon::planned(Uuid::new_v4(), None);
        assert_eq!(sermon.status, SermonStatus::Planned);
        assert!(sermon.started_at.is_none());

        sermon.activate();
        assert_eq!(sermon.status, SermonStatus::Active);
        assert!(sermon.started_at.is_some());
    }

    #[test]
    fn pause_then_resume_round_trips_through_active() {
        let mut sermon = Sermon::start(Uuid::new_v4(), None);
        sermon.pause();
        assert_eq!(sermon.status, SermonStatus::Paused);
        sermon.resume();
        assert_eq!(sermon.status, SermonStatus::Active);
    }

    #[test]
    fn end_sets_ended_at_and_never_unsets_it() {
        let mut sermon = Sermon::start(Uuid::new_v4(), None);
        sermon.end();
        assert_eq!(sermon.status, SermonStatus::Ended);
        assert!(sermon.ended_at.is_some());
    }

    #[test]
    fn cancel_moves_a_planned_sermon_to_cancelled() {
        let mut sermon = Sermon::planned(Uuid::new_v4(), None);
        sermon.cancel();
        assert_eq!(sermon.status, SermonStatus::Cancelled);
    }

    #[test]
    fn set_title_and_assign_speaker_only_change_their_own_field() {
        let mut sermon = Sermon::start(Uuid::new_v4(), None);
        let status_before = sermon.status;
        sermon.set_title("Grace Abounding");
        sermon.assign_speaker(Speaker::new("Pastor Jane Doe", SpeakerRole::Primary));
        assert_eq!(sermon.title.as_deref(), Some("Grace Abounding"));
        assert_eq!(sermon.speaker.as_ref().unwrap().name, "Pastor Jane Doe");
        assert_eq!(
            sermon.status, status_before,
            "metadata correction never touches lifecycle status"
        );
    }

    #[test]
    fn unset_title_and_speaker_remain_none_until_explicitly_supplied() {
        let sermon = Sermon::start(Uuid::new_v4(), None);
        assert!(sermon.title.is_none());
        assert!(
            sermon.speaker.is_none(),
            "unknown metadata remains unknown (invariant 12)"
        );
    }

    // --- state machine (is_valid_transition) --------------------------------

    #[test]
    fn every_documented_valid_transition_is_accepted() {
        use SermonStatus::*;
        let valid = [
            (Planned, Active),
            (Planned, Cancelled),
            (Active, Paused),
            (Active, Ended),
            (Paused, Active),
            (Paused, Ended),
        ];
        for (from, to) in valid {
            assert!(
                is_valid_transition(from, to),
                "{from:?} -> {to:?} should be valid"
            );
        }
    }

    #[test]
    fn ended_and_cancelled_are_terminal() {
        use SermonStatus::*;
        for terminal in [Ended, Cancelled] {
            for to in [Planned, Active, Paused, Ended, Cancelled] {
                assert!(
                    !is_valid_transition(terminal, to),
                    "{terminal:?} must never transition to {to:?} - terminal states are final"
                );
            }
        }
    }

    #[test]
    fn ended_to_active_is_never_valid_even_though_both_are_real_states() {
        assert!(!is_valid_transition(
            SermonStatus::Ended,
            SermonStatus::Active
        ));
    }

    #[test]
    fn a_same_state_call_is_not_a_valid_transition() {
        for status in [
            SermonStatus::Planned,
            SermonStatus::Active,
            SermonStatus::Paused,
            SermonStatus::Ended,
            SermonStatus::Cancelled,
        ] {
            assert!(!is_valid_transition(status, status));
        }
    }

    #[test]
    fn planned_cannot_skip_directly_to_paused_or_ended() {
        assert!(!is_valid_transition(
            SermonStatus::Planned,
            SermonStatus::Paused
        ));
        assert!(!is_valid_transition(
            SermonStatus::Planned,
            SermonStatus::Ended
        ));
    }

    #[test]
    fn status_labels_are_screaming_snake_case_and_distinct() {
        let all = [
            SermonStatus::Planned,
            SermonStatus::Active,
            SermonStatus::Paused,
            SermonStatus::Ended,
            SermonStatus::Cancelled,
        ];
        let mut labels: Vec<&str> = all.iter().map(|s| s.label()).collect();
        let before = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), before);
        for label in labels {
            assert_eq!(label, label.to_uppercase());
        }
    }

    #[test]
    fn sermon_serializes_with_camel_case_fields() {
        let sermon = Sermon::start(Uuid::new_v4(), Some("Grace".to_string()));
        let json = serde_json::to_value(&sermon).unwrap();
        assert!(json.get("serviceId").is_some());
        assert!(json.get("startedAt").is_some());
        assert!(json.get("endedAt").is_some());
    }
}
