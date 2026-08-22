//! [`ScriptedAcousticMusicRecognizer`] - a deterministic test/demo
//! adapter at the `AcousticMusicRecognizer` boundary, mirroring
//! `cip_ai_speech::ScriptedSpeechEngine` exactly: implements the real
//! trait but ignores the audio it's given, instead stepping through a
//! pre-programmed script of outcomes. Exists to test the
//! `AudioEngine -> acoustic worker -> MusicIntelligenceEngine` wiring,
//! evidence fusion, ambiguity handling, and song-transition detection
//! end to end without a microphone or a real acoustic model. It is not,
//! and must never become, a substitute for validating a real acoustic
//! backend - see `LocalAcousticMusicRecognizer` and
//! `docs/acoustic-music.md`.

use std::collections::VecDeque;

use cip_core_music::{
    AcousticMusicRecognizer, AcousticRecognitionCandidate, AcousticRecognitionError,
    AcousticRecognitionMethod, AcousticRecognitionStatus, AudioSegment,
};

/// One scripted outcome for a single `recognize()` call - covers every
/// scenario Phase 2.2's manual test mode requires (a single candidate,
/// an ambiguous pair, no result, a hard failure, and a transient
/// "unavailable" outcome). A song transition (A -> B) is simply two
/// consecutive `Candidates` steps naming different songs - no dedicated
/// step type is needed for it.
#[derive(Debug, Clone)]
pub enum ScriptedAcousticStep {
    /// Zero or more candidates for this call - an empty vec and
    /// [`ScriptedAcousticStep::NoResult`] are equivalent; both exist so a
    /// script reads naturally either way. `segment_id` on each candidate
    /// is overwritten with the real segment passed to `recognize()` -
    /// callers do not need to know it ahead of time when building a
    /// script.
    Candidates(Vec<AcousticRecognitionCandidate>),
    NoResult,
    Error(String),
    Unavailable(String),
}

pub struct ScriptedAcousticMusicRecognizer {
    script: VecDeque<ScriptedAcousticStep>,
}

impl ScriptedAcousticMusicRecognizer {
    pub fn new(steps: impl IntoIterator<Item = ScriptedAcousticStep>) -> Self {
        Self {
            script: steps.into_iter().collect(),
        }
    }
}

impl AcousticMusicRecognizer for ScriptedAcousticMusicRecognizer {
    fn status(&self) -> AcousticRecognitionStatus {
        AcousticRecognitionStatus::Available
    }

    fn method(&self) -> AcousticRecognitionMethod {
        AcousticRecognitionMethod::Test
    }

    fn status_reason(&self) -> Option<String> {
        Some("scripted test adapter, not a real acoustic model".to_string())
    }

    /// `content_ids` is honored defensively even for scripted
    /// candidates: a script that (by mistake) names a dataset not in
    /// `content_ids` has that candidate silently dropped here, exactly
    /// as a real recognizer's own dataset scoping would, so a dataset-
    /// isolation test cannot be defeated by a misconfigured fixture.
    fn recognize(
        &mut self,
        segment: &AudioSegment,
        content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError> {
        match self.script.pop_front() {
            None | Some(ScriptedAcousticStep::NoResult) => Ok(Vec::new()),
            Some(ScriptedAcousticStep::Candidates(mut candidates)) => {
                for candidate in &mut candidates {
                    candidate.segment_id = segment.id;
                }
                candidates.retain(|c| content_ids.contains(&c.content_id));
                Ok(candidates)
            }
            Some(ScriptedAcousticStep::Error(reason)) => {
                Err(AcousticRecognitionError::RecognitionFailed(reason))
            }
            Some(ScriptedAcousticStep::Unavailable(reason)) => {
                Err(AcousticRecognitionError::Unavailable(reason))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use uuid::Uuid;

    fn segment() -> AudioSegment {
        AudioSegment::new(vec![1; 16_000], 16_000, 0)
    }

    fn candidate(song_id: &str, content_id: &str, score: f32) -> AcousticRecognitionCandidate {
        AcousticRecognitionCandidate {
            song_id: song_id.to_string(),
            content_id: content_id.to_string(),
            confidence: ConfidenceResult::new(score, ConfidenceSource::Model, None),
            method: AcousticRecognitionMethod::Test,
            segment_id: Uuid::nil(),
            duration_ms: 8_000,
            evidence: vec!["scripted acoustic match".to_string()],
        }
    }

    #[test]
    fn is_always_available_and_reports_itself_as_a_test_adapter() {
        let recognizer = ScriptedAcousticMusicRecognizer::new(vec![]);
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Available);
        assert_eq!(recognizer.method(), AcousticRecognitionMethod::Test);
        assert!(recognizer.status_reason().unwrap().contains("scripted"));
    }

    #[test]
    fn emits_one_candidate_set_per_step_in_order() {
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![candidate("a", "music:dev", 0.9)]),
            ScriptedAcousticStep::Candidates(vec![candidate("b", "music:dev", 0.85)]),
        ]);
        let content_ids = vec!["music:dev".to_string()];

        let first = recognizer.recognize(&segment(), &content_ids).unwrap();
        assert_eq!(first[0].song_id, "a");
        let second = recognizer.recognize(&segment(), &content_ids).unwrap();
        assert_eq!(second[0].song_id, "b");
    }

    #[test]
    fn stamps_the_real_segment_id_onto_scripted_candidates() {
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![candidate("a", "music:dev", 0.9)]),
        ]);
        let real_segment = segment();
        let content_ids = vec!["music:dev".to_string()];
        let result = recognizer.recognize(&real_segment, &content_ids).unwrap();
        assert_eq!(result[0].segment_id, real_segment.id);
    }

    #[test]
    fn ambiguity_scenario_returns_two_close_candidates() {
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::Candidates(vec![
                candidate("a", "music:dev", 0.91),
                candidate("b", "music:dev", 0.89),
            ])]);
        let content_ids = vec!["music:dev".to_string()];
        let result = recognizer.recognize(&segment(), &content_ids).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn no_result_scenario_returns_an_empty_vec_not_an_error() {
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::NoResult]);
        let result = recognizer
            .recognize(&segment(), &["music:dev".to_string()])
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn error_scenario_returns_recognition_failed() {
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::Error(
                "simulated model crash".to_string(),
            )]);
        assert!(matches!(
            recognizer.recognize(&segment(), &["music:dev".to_string()]),
            Err(AcousticRecognitionError::RecognitionFailed(_))
        ));
    }

    #[test]
    fn unavailable_scenario_returns_unavailable_error() {
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::Unavailable(
                "device disappeared".to_string(),
            )]);
        assert!(matches!(
            recognizer.recognize(&segment(), &["music:dev".to_string()]),
            Err(AcousticRecognitionError::Unavailable(_))
        ));
    }

    #[test]
    fn transition_scenario_is_two_consecutive_steps_naming_different_songs() {
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![
            ScriptedAcousticStep::Candidates(vec![candidate("song-a", "music:dev", 0.9)]),
            ScriptedAcousticStep::Candidates(vec![candidate("song-b", "music:dev", 0.9)]),
        ]);
        let content_ids = vec!["music:dev".to_string()];
        let first = recognizer.recognize(&segment(), &content_ids).unwrap();
        let second = recognizer.recognize(&segment(), &content_ids).unwrap();
        assert_eq!(first[0].song_id, "song-a");
        assert_eq!(second[0].song_id, "song-b");
    }

    #[test]
    fn exhausted_script_returns_no_result_forever() {
        let mut recognizer = ScriptedAcousticMusicRecognizer::new(vec![]);
        let content_ids = vec!["music:dev".to_string()];
        assert!(recognizer
            .recognize(&segment(), &content_ids)
            .unwrap()
            .is_empty());
        assert!(recognizer
            .recognize(&segment(), &content_ids)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_candidate_naming_a_dataset_outside_content_ids_is_dropped() {
        let mut recognizer =
            ScriptedAcousticMusicRecognizer::new(vec![ScriptedAcousticStep::Candidates(vec![
                candidate("a", "music:enabled", 0.9),
                candidate("b", "music:not-requested", 0.9),
            ])]);
        let content_ids = vec!["music:enabled".to_string()];
        let result = recognizer.recognize(&segment(), &content_ids).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].song_id, "a");
    }
}
