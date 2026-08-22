//! [`NullAcousticMusicRecognizer`] - the safe default, mirroring
//! `cip_ai_speech::NullSpeechEngine` exactly: reports itself unavailable,
//! rejects every recognition attempt. What the application uses whenever
//! no real recognizer is configured, so "no acoustic model" is never
//! fatal - manual/lyric-based Music Intelligence keeps working
//! regardless.

use cip_core_music::{
    AcousticMusicRecognizer, AcousticRecognitionCandidate, AcousticRecognitionError,
    AcousticRecognitionMethod, AcousticRecognitionStatus, AudioSegment,
};

#[derive(Default)]
pub struct NullAcousticMusicRecognizer;

impl AcousticMusicRecognizer for NullAcousticMusicRecognizer {
    fn status(&self) -> AcousticRecognitionStatus {
        AcousticRecognitionStatus::Unavailable
    }

    fn method(&self) -> AcousticRecognitionMethod {
        AcousticRecognitionMethod::None
    }

    fn status_reason(&self) -> Option<String> {
        Some("no acoustic recognizer configured".to_string())
    }

    fn recognize(
        &mut self,
        _segment: &AudioSegment,
        _content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError> {
        Err(AcousticRecognitionError::Unavailable(
            "no acoustic recognizer configured".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment() -> AudioSegment {
        AudioSegment::new(vec![0; 16_000], 16_000, 0)
    }

    #[test]
    fn reports_unavailable_and_rejects_recognition() {
        let mut recognizer = NullAcousticMusicRecognizer;
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
        assert_eq!(recognizer.method(), AcousticRecognitionMethod::None);
        assert!(matches!(
            recognizer.recognize(&segment(), &["music:dev".to_string()]),
            Err(AcousticRecognitionError::Unavailable(_))
        ));
    }

    #[test]
    fn satisfies_the_trait_object_contract() {
        let recognizer: Box<dyn AcousticMusicRecognizer> = Box::new(NullAcousticMusicRecognizer);
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
    }
}
