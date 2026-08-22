//! [`Speaker`]: explicit, operator-supplied speaker identity for a
//! [`crate::foundation::Sermon`] - never biometric speaker recognition or
//! diarization (spec section "SPEAKER MODEL"), and never confused with a
//! CIP user/operator account. A speaker only exists because an operator
//! explicitly said so; nothing here guesses who is talking from audio or
//! transcript content.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A sermon has at most one [`Speaker`] in this phase - see
/// `docs/sermon-foundation.md`'s "NOT AVAILABLE" section for why
/// multiple-speaker support (a panel discussion, a guest introduced
/// partway through) is deliberately deferred rather than half-built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerRole {
    Primary,
    Guest,
}

impl SpeakerRole {
    pub const fn label(self) -> &'static str {
        match self {
            SpeakerRole::Primary => "PRIMARY",
            SpeakerRole::Guest => "GUEST",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    pub id: Uuid,
    /// Exactly what the operator typed - never inferred, never defaulted
    /// to a placeholder like "Unknown Speaker".
    pub name: String,
    pub role: SpeakerRole,
}

impl Speaker {
    pub fn new(name: impl Into<String>, role: SpeakerRole) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            role,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_speaker_carries_exactly_the_supplied_name_and_role() {
        let speaker = Speaker::new("Pastor Jane Doe", SpeakerRole::Primary);
        assert_eq!(speaker.name, "Pastor Jane Doe");
        assert_eq!(speaker.role, SpeakerRole::Primary);
    }

    #[test]
    fn each_speaker_gets_a_distinct_id() {
        let a = Speaker::new("A", SpeakerRole::Primary);
        let b = Speaker::new("A", SpeakerRole::Primary);
        assert_ne!(
            a.id, b.id,
            "two speakers with the same name are still distinct identities"
        );
    }

    #[test]
    fn role_labels_are_screaming_snake_case_and_distinct() {
        assert_eq!(SpeakerRole::Primary.label(), "PRIMARY");
        assert_eq!(SpeakerRole::Guest.label(), "GUEST");
        assert_ne!(SpeakerRole::Primary.label(), SpeakerRole::Guest.label());
    }

    #[test]
    fn speaker_serializes_with_camel_case_fields() {
        let speaker = Speaker::new("Pastor Jane Doe", SpeakerRole::Guest);
        let json = serde_json::to_value(&speaker).unwrap();
        assert_eq!(json["name"], "Pastor Jane Doe");
        assert_eq!(json["role"], "guest");
    }
}
