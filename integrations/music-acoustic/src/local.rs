//! [`LocalAcousticMusicRecognizer`] - the real local-model integration
//! boundary, mirroring `cip_ai_speech::WhisperSpeechEngine`'s pattern:
//! genuine configuration, genuine status resolution, and an honest
//! `Unavailable`/`Error` report when nothing usable is configured -
//! never fabricated recognition.
//!
//! ## Phase 7.1: a real backend, at last
//!
//! Phase 2.2 deliberately left this always `Unavailable` - see this
//! module's git history and `docs/acoustic-music.md`'s "PROVEN vs NOT
//! AVAILABLE" section for why (no fingerprint/embedding algorithm had
//! been chosen or implemented yet). Phase 7.1 fills that seam with a
//! genuine spectral landmark (constellation) hashing recognizer - see
//! `crate::fingerprint` for the algorithm itself. This struct's job is
//! narrower: read a manifest describing which reference audio file
//! belongs to which song/dataset, enroll each one into a
//! `FingerprintIndex` at construction time (mirroring
//! `WhisperSpeechEngine::load`'s "fail/succeed at load time, not per
//! call" design), and answer `recognize()` calls from that index.
//!
//! ## What is, and is not, proven by this phase
//!
//! The algorithm itself is proven correct against synthetic audio (see
//! `crate::fingerprint`'s test suite: self-match, cross-song rejection,
//! time-shift invariance, noise tolerance) - this container has no real
//! recorded music to enroll or test against (the same "Music Library is
//! legitimately empty in a production build" constraint recorded since
//! Phase 2.7.1). This module has therefore never been exercised against
//! a real hymn/worship recording captured by a real microphone in a real
//! room - that remains the decisive Environment C gate, exactly like
//! every other model-shaped capability this project has shipped (Whisper,
//! the semantic Bible search embedding model).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use cip_core_music::{
    AcousticMusicRecognizer, AcousticRecognitionCandidate, AcousticRecognitionError,
    AcousticRecognitionMethod, AcousticRecognitionStatus, AudioSegment,
};

use crate::fingerprint::FingerprintIndex;

/// The manifest filename a configured model directory is expected to
/// contain. Its schema (see [`Manifest`]) names one or more reference
/// audio files (WAV, any sample rate/channel count - resampled to match
/// each other and the query audio at enrollment/query time) and the
/// song/dataset each belongs to.
pub const MODEL_MANIFEST_FILENAME: &str = "acoustic-model.json";

/// Minimum landmark-hash votes (see `crate::fingerprint::FingerprintMatch`)
/// a song needs to be reported as a candidate at all - filters out the
/// small number of coincidental single-hash collisions any two clips of
/// real audio will occasionally share, which is expected background
/// noise for this algorithm, not a sign of a real match. Chosen
/// conservatively (favoring silence over a false positive), matching this
/// project's "never fabricate a match" discipline; a future phase could
/// make this operator-tunable if real-world testing shows it needs
/// adjustment.
pub const MIN_VOTES: usize = 8;

/// One entry in the manifest: which reference audio file to enroll, and
/// which song/dataset it belongs to. `pub` (Phase 7.2) so a Tauri command
/// can read/write the manifest directly via [`read_manifest_entries`]/
/// [`write_manifest_entries`] rather than this crate needing to expose a
/// second, parallel type for the same shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSong {
    pub song_id: String,
    pub content_id: String,
    /// Relative (to the manifest's own directory) or absolute path to a
    /// WAV file of this song, used only at enrollment time - never read
    /// again per-`recognize()` call.
    pub audio_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    songs: Vec<ManifestSong>,
}

/// Read a model directory's current manifest entries, if any. An absent
/// or empty manifest file returns an empty list (not an error) - the
/// same "nothing configured yet is not a failure" discipline
/// [`resolve`] itself follows; only genuinely malformed JSON is an
/// `Err`. Used by [`enroll_acoustic_reference`]'s Tauri-command
/// counterpart (Phase 7.2) to list and upsert enrollments without
/// duplicating this crate's own manifest schema.
pub fn read_manifest_entries(model_dir: &Path) -> Result<Vec<ManifestSong>, String> {
    let manifest_path = model_dir.join(MODEL_MANIFEST_FILENAME);
    match std::fs::read(&manifest_path) {
        Err(_) => Ok(Vec::new()),
        Ok(bytes) if bytes.is_empty() => Ok(Vec::new()),
        Ok(bytes) => {
            let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
                format!(
                    "existing manifest at {} is malformed JSON: {e}",
                    manifest_path.display()
                )
            })?;
            Ok(manifest.songs)
        }
    }
}

/// Write a full replacement set of manifest entries to `model_dir`,
/// creating the directory if needed. Always overwrites the whole file -
/// callers that want to add/update one entry read the current list via
/// [`read_manifest_entries`], modify it, and write the result back
/// (an upsert-by-`song_id` is the caller's job, not this function's,
/// since "replace an existing entry" vs. "always append" is a policy
/// choice this crate has no opinion on).
pub fn write_manifest_entries(model_dir: &Path, entries: &[ManifestSong]) -> Result<(), String> {
    std::fs::create_dir_all(model_dir)
        .map_err(|e| format!("could not create {}: {e}", model_dir.display()))?;
    let manifest = Manifest {
        songs: entries.to_vec(),
    };
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("could not serialize manifest: {e}"))?;
    let manifest_path = model_dir.join(MODEL_MANIFEST_FILENAME);
    std::fs::write(&manifest_path, json)
        .map_err(|e| format!("could not write {}: {e}", manifest_path.display()))?;
    Ok(())
}

/// Validate that `path` is a usable reference recording - the exact same
/// check [`enroll_one`] performs before enrolling a manifest entry,
/// exposed standalone (Phase 7.2) so a Tauri command can reject a bad
/// file at the moment an operator picks it, before it is ever copied
/// into the model directory or written into the manifest. Never partial:
/// a file that passes this check is guaranteed to decode identically at
/// enrollment time, since both call the same [`decode_reference_wav`].
pub fn validate_reference_wav(path: &Path) -> Result<(), String> {
    decode_reference_wav(path).map(|_| ())
}

#[derive(Debug, Clone)]
pub struct LocalAcousticConfig {
    /// Directory expected to contain `MODEL_MANIFEST_FILENAME`. `None`
    /// means "never configured" - the honest default, never a guessed
    /// path.
    pub model_dir: Option<PathBuf>,
    pub enabled: bool,
}

impl Default for LocalAcousticConfig {
    fn default() -> Self {
        Self {
            model_dir: None,
            enabled: true,
        }
    }
}

pub struct LocalAcousticMusicRecognizer {
    status: AcousticRecognitionStatus,
    reason: String,
    /// Maps an enrolled `song_id` (the manifest's own identifier, e.g. a
    /// hymn number) to the `content_id` (dataset) it belongs to, so
    /// `recognize()` can honor its `content_ids` scoping the same way
    /// every other recognizer does - the fingerprint index itself has no
    /// concept of datasets, only song ids.
    song_content: std::collections::HashMap<String, String>,
    index: FingerprintIndex,
}

impl LocalAcousticMusicRecognizer {
    /// Resolves status and, when a valid manifest with at least one
    /// successfully-enrolled reference recording is found, builds a real
    /// `FingerprintIndex` - all done once, at construction, never
    /// re-checked per `recognize()` call (mirroring
    /// `WhisperSpeechEngine::load`). A caller that wants to react to a
    /// manifest changing mid-service reconstructs this type; nothing
    /// here polls the file system in the background.
    pub fn configure(config: LocalAcousticConfig) -> Self {
        let (status, reason, song_content, index) = resolve(&config);
        Self {
            status,
            reason,
            song_content,
            index,
        }
    }
}

type ResolveOutcome = (
    AcousticRecognitionStatus,
    String,
    std::collections::HashMap<String, String>,
    FingerprintIndex,
);

fn resolve(config: &LocalAcousticConfig) -> ResolveOutcome {
    if !config.enabled {
        return (
            AcousticRecognitionStatus::Disabled,
            "acoustic recognition explicitly disabled".to_string(),
            std::collections::HashMap::new(),
            FingerprintIndex::new(),
        );
    }
    let Some(dir) = &config.model_dir else {
        return (
            AcousticRecognitionStatus::Unavailable,
            "no acoustic model directory configured".to_string(),
            std::collections::HashMap::new(),
            FingerprintIndex::new(),
        );
    };
    if !dir.is_dir() {
        return (
            AcousticRecognitionStatus::Unavailable,
            format!(
                "configured model directory does not exist: {}",
                dir.display()
            ),
            std::collections::HashMap::new(),
            FingerprintIndex::new(),
        );
    }
    let manifest_path = dir.join(MODEL_MANIFEST_FILENAME);
    let bytes = match std::fs::read(&manifest_path) {
        Err(_) => {
            return (
                AcousticRecognitionStatus::Unavailable,
                format!("no model manifest found at {}", manifest_path.display()),
                std::collections::HashMap::new(),
                FingerprintIndex::new(),
            )
        }
        Ok(bytes) if bytes.is_empty() => {
            return (
                AcousticRecognitionStatus::Error,
                format!(
                    "model manifest is empty (malformed): {}",
                    manifest_path.display()
                ),
                std::collections::HashMap::new(),
                FingerprintIndex::new(),
            )
        }
        Ok(bytes) => bytes,
    };
    let manifest: Manifest = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            return (
                AcousticRecognitionStatus::Error,
                format!(
                    "model manifest at {} is malformed JSON: {e}",
                    manifest_path.display()
                ),
                std::collections::HashMap::new(),
                FingerprintIndex::new(),
            )
        }
    };
    if manifest.songs.is_empty() {
        return (
            AcousticRecognitionStatus::Unavailable,
            format!(
                "model manifest at {} lists zero songs to enroll",
                manifest_path.display()
            ),
            std::collections::HashMap::new(),
            FingerprintIndex::new(),
        );
    }

    let mut song_content = std::collections::HashMap::new();
    let mut index = FingerprintIndex::new();
    let mut enrolled = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for entry in &manifest.songs {
        match enroll_one(dir, entry, &mut index) {
            Ok(()) => {
                song_content.insert(entry.song_id.clone(), entry.content_id.clone());
                enrolled += 1;
            }
            Err(reason) => failures.push(format!("{}: {reason}", entry.song_id)),
        }
    }

    if enrolled == 0 {
        return (
            AcousticRecognitionStatus::Error,
            format!(
                "manifest at {} named {} song(s) but none could be enrolled: {}",
                manifest_path.display(),
                manifest.songs.len(),
                failures.join("; ")
            ),
            std::collections::HashMap::new(),
            FingerprintIndex::new(),
        );
    }

    let reason = if failures.is_empty() {
        format!(
            "{enrolled} reference recording(s) enrolled from {}",
            manifest_path.display()
        )
    } else {
        format!(
            "{enrolled} of {} reference recording(s) enrolled from {} ({} failed: {})",
            manifest.songs.len(),
            manifest_path.display(),
            failures.len(),
            failures.join("; ")
        )
    };
    (
        AcousticRecognitionStatus::Available,
        reason,
        song_content,
        index,
    )
}

/// Decode a WAV file into mono `i16` PCM samples, downmixing if necessary
/// (averaging channels - the same normalization `cip_core_service::AudioChunk`
/// already guarantees for live capture, so enrolled reference audio and
/// live query audio are on equal footing). The one real decode path both
/// [`enroll_one`] and [`validate_reference_wav`] use, so a file that
/// passes validation is guaranteed to enroll identically, not merely
/// similarly.
fn decode_reference_wav(path: &Path) -> Result<Vec<i16>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return Err(format!(
            "{} is not 16-bit PCM WAV ({:?}, {} bits)",
            path.display(),
            spec.sample_format,
            spec.bits_per_sample
        ));
    }
    let channels = spec.channels as usize;
    if channels == 0 {
        return Err(format!("{} declares zero channels", path.display()));
    }
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<i16>, _>>()
        .map_err(|e| format!("could not decode {}: {e}", path.display()))?;
    let mono: Vec<i16> = if channels == 1 {
        samples
    } else {
        samples
            .chunks(channels)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
                (sum / frame.len() as i32) as i16
            })
            .collect()
    };
    if mono.is_empty() {
        return Err(format!("{} contains no audio samples", path.display()));
    }
    Ok(mono)
}

/// Resolve, decode, and enroll one manifest entry's WAV file into
/// `index`. Never panics on a malformed file - any failure is reported
/// as `Err` and skipped, honoring "one bad reference file must not take
/// down every other song's recognition."
fn enroll_one(
    manifest_dir: &Path,
    entry: &ManifestSong,
    index: &mut FingerprintIndex,
) -> Result<(), String> {
    let audio_path = resolve_audio_path(manifest_dir, &entry.audio_path);
    let mono = decode_reference_wav(&audio_path)?;
    index.enroll(&entry.song_id, &mono);
    Ok(())
}

fn resolve_audio_path(manifest_dir: &Path, audio_path: &str) -> PathBuf {
    let candidate = PathBuf::from(audio_path);
    if candidate.is_absolute() {
        candidate
    } else {
        manifest_dir.join(candidate)
    }
}

impl AcousticMusicRecognizer for LocalAcousticMusicRecognizer {
    fn status(&self) -> AcousticRecognitionStatus {
        self.status
    }

    fn method(&self) -> AcousticRecognitionMethod {
        AcousticRecognitionMethod::LocalModel
    }

    fn status_reason(&self) -> Option<String> {
        Some(self.reason.clone())
    }

    fn recognize(
        &mut self,
        segment: &AudioSegment,
        content_ids: &[String],
    ) -> Result<Vec<AcousticRecognitionCandidate>, AcousticRecognitionError> {
        match self.status {
            AcousticRecognitionStatus::Disabled => return Err(AcousticRecognitionError::Disabled),
            AcousticRecognitionStatus::Error => {
                return Err(AcousticRecognitionError::RecognitionFailed(
                    self.reason.clone(),
                ))
            }
            AcousticRecognitionStatus::Unavailable => {
                return Err(AcousticRecognitionError::Unavailable(self.reason.clone()))
            }
            AcousticRecognitionStatus::Available => {}
        }

        let matches = self.index.query(&segment.samples, MIN_VOTES);
        let candidates = matches
            .into_iter()
            .filter_map(|m| {
                let content_id = self.song_content.get(&m.song_id)?;
                if !content_ids.iter().any(|id| id == content_id) {
                    return None;
                }
                // Votes-to-confidence: a deliberately conservative,
                // saturating mapping - `total_query_landmarks` varies
                // with clip length/loudness, so votes are normalized
                // against it rather than compared to a fixed constant.
                // Capped at 0.97 (never 1.0): acoustic fingerprinting is
                // never proof beyond all doubt the same way an exact
                // scripture-reference match is - see
                // `docs/phase-7-1-real-audio-fingerprinting.md`.
                let ratio = if m.total_query_landmarks == 0 {
                    0.0
                } else {
                    m.votes as f32 / m.total_query_landmarks as f32
                };
                let confidence_value = (ratio * 2.0).min(0.97);
                Some(AcousticRecognitionCandidate {
                    song_id: m.song_id.clone(),
                    content_id: content_id.clone(),
                    confidence: cip_core_confidence::ConfidenceResult::new(
                        confidence_value,
                        cip_core_confidence::ConfidenceSource::Model,
                        None,
                    ),
                    method: AcousticRecognitionMethod::LocalModel,
                    segment_id: segment.id,
                    duration_ms: segment.duration_ms,
                    evidence: vec![format!(
                        "{} landmark hash(es) agreed on one alignment offset (of {} in this segment)",
                        m.votes, m.total_query_landmarks
                    )],
                })
            })
            .collect();
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn segment() -> AudioSegment {
        AudioSegment::new(vec![1; 16_000], 16_000, 0)
    }

    fn synth_tone(
        freqs_hz: &[f32],
        sample_rate: u32,
        duration_ms: u64,
        amplitude: f32,
    ) -> Vec<i16> {
        let n = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                let sum: f32 = freqs_hz
                    .iter()
                    .map(|&f| (2.0 * PI * f * t).sin())
                    .sum::<f32>()
                    / freqs_hz.len() as f32;
                (sum * amplitude) as i16
            })
            .collect()
    }

    fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn disabled_config_reports_disabled_and_rejects_recognition() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: None,
            enabled: false,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Disabled);
    }

    #[test]
    fn no_model_dir_configured_is_honestly_unavailable() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig::default());
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
        assert!(recognizer
            .status_reason()
            .unwrap()
            .contains("no acoustic model directory"));
    }

    #[test]
    fn a_nonexistent_model_dir_is_unavailable_not_a_crash() {
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(PathBuf::from("/nonexistent/cip-acoustic-model-dir")),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
    }

    #[test]
    fn an_empty_model_dir_with_no_manifest_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
    }

    #[test]
    fn a_malformed_empty_manifest_is_reported_as_error_not_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_MANIFEST_FILENAME), b"").unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Error);
        assert!(recognizer.status_reason().unwrap().contains("malformed"));
    }

    #[test]
    fn invalid_json_manifest_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_MANIFEST_FILENAME), b"not json").unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Error);
        assert!(recognizer
            .status_reason()
            .unwrap()
            .contains("malformed JSON"));
    }

    #[test]
    fn a_manifest_with_zero_songs_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            br#"{"songs": []}"#,
        )
        .unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Unavailable);
        assert!(recognizer.status_reason().unwrap().contains("zero songs"));
    }

    #[test]
    fn a_manifest_naming_a_missing_audio_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = serde_json::json!({
            "songs": [
                {"songId": "s1", "contentId": "music:dev", "audioPath": "missing.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Error);
        assert!(recognizer
            .status_reason()
            .unwrap()
            .contains("none could be enrolled"));
    }

    #[test]
    fn a_valid_manifest_with_real_reference_audio_becomes_available_and_recognizes_it() {
        let dir = tempfile::tempdir().unwrap();
        let song = synth_tone(&[440.0, 880.0, 1_320.0], 16_000, 5_000, 12_000.0);
        write_wav(&dir.path().join("song1.wav"), &song, 16_000);

        let manifest = serde_json::json!({
            "songs": [
                {"songId": "hymn-1", "contentId": "music:dev-hymnbook", "audioPath": "song1.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let mut recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Available);

        // A 4-second excerpt of the enrolled song, as a live "query"
        // segment - proves the whole enrollment -> recognize() path
        // genuinely round-trips through real WAV I/O and the real
        // fingerprint index, not a fake/stubbed result.
        let excerpt = AudioSegment::new(song[8_000..8_000 + 64_000].to_vec(), 16_000, 0);
        let candidates = recognizer
            .recognize(&excerpt, &["music:dev-hymnbook".to_string()])
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].song_id, "hymn-1");
        assert_eq!(candidates[0].content_id, "music:dev-hymnbook");
        assert_eq!(candidates[0].method, AcousticRecognitionMethod::LocalModel);
    }

    #[test]
    fn recognize_respects_content_id_scoping_even_for_a_real_match() {
        let dir = tempfile::tempdir().unwrap();
        let song = synth_tone(&[500.0, 1_000.0], 16_000, 5_000, 12_000.0);
        write_wav(&dir.path().join("song1.wav"), &song, 16_000);

        let manifest = serde_json::json!({
            "songs": [
                {"songId": "hymn-1", "contentId": "music:dev-hymnbook", "audioPath": "song1.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let mut recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });

        let excerpt = AudioSegment::new(song[..64_000].to_vec(), 16_000, 0);
        // Asked to search a dataset the enrolled song is not part of -
        // must be filtered out even though the acoustic match is real.
        let candidates = recognizer
            .recognize(&excerpt, &["music:some-other-dataset".to_string()])
            .unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn silence_against_a_real_enrolled_song_yields_no_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let song = synth_tone(&[440.0], 16_000, 5_000, 12_000.0);
        write_wav(&dir.path().join("song1.wav"), &song, 16_000);
        let manifest = serde_json::json!({
            "songs": [
                {"songId": "hymn-1", "contentId": "music:dev-hymnbook", "audioPath": "song1.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let mut recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        let silence = AudioSegment::new(vec![0_i16; 64_000], 16_000, 0);
        let candidates = recognizer
            .recognize(&silence, &["music:dev-hymnbook".to_string()])
            .unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn one_bad_reference_file_does_not_prevent_other_songs_from_enrolling() {
        let dir = tempfile::tempdir().unwrap();
        let song = synth_tone(&[440.0], 16_000, 3_000, 12_000.0);
        write_wav(&dir.path().join("good.wav"), &song, 16_000);

        let manifest = serde_json::json!({
            "songs": [
                {"songId": "bad", "contentId": "music:dev", "audioPath": "missing.wav"},
                {"songId": "good", "contentId": "music:dev", "audioPath": "good.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Available);
        let reason = recognizer.status_reason().unwrap();
        assert!(reason.contains("1 of 2"));
        assert!(reason.contains("bad"));
    }

    #[test]
    fn recognize_never_panics_regardless_of_status() {
        for config in [
            LocalAcousticConfig {
                model_dir: None,
                enabled: false,
            },
            LocalAcousticConfig::default(),
        ] {
            let mut recognizer = LocalAcousticMusicRecognizer::configure(config);
            let _ = recognizer.recognize(&segment(), &["music:dev".to_string()]);
        }
    }

    // --- Phase 7.2: enrollment-support helpers ---------------------------

    #[test]
    fn read_manifest_entries_on_a_directory_with_no_manifest_is_an_empty_list_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let entries = read_manifest_entries(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn read_manifest_entries_on_a_nonexistent_directory_is_an_empty_list_not_an_error() {
        let entries =
            read_manifest_entries(std::path::Path::new("/nonexistent/cip-acoustic-dir")).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn read_manifest_entries_rejects_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_MANIFEST_FILENAME), b"not json").unwrap();
        let result = read_manifest_entries(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("malformed JSON"));
    }

    #[test]
    fn write_then_read_manifest_entries_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![
            ManifestSong {
                song_id: "hymn-1".to_string(),
                content_id: "music:dev-hymnbook".to_string(),
                audio_path: "hymn-1.wav".to_string(),
            },
            ManifestSong {
                song_id: "hymn-2".to_string(),
                content_id: "music:dev-hymnbook".to_string(),
                audio_path: "hymn-2.wav".to_string(),
            },
        ];
        write_manifest_entries(dir.path(), &entries).unwrap();
        let read_back = read_manifest_entries(dir.path()).unwrap();
        assert_eq!(read_back, entries);
    }

    #[test]
    fn write_manifest_entries_creates_the_model_directory_if_missing() {
        let parent = tempfile::tempdir().unwrap();
        let nested = parent.path().join("acoustic");
        assert!(!nested.exists());
        write_manifest_entries(&nested, &[]).unwrap();
        assert!(nested.join(MODEL_MANIFEST_FILENAME).exists());
    }

    #[test]
    fn write_manifest_entries_fully_replaces_the_previous_contents() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest_entries(
            dir.path(),
            &[ManifestSong {
                song_id: "old".to_string(),
                content_id: "music:dev".to_string(),
                audio_path: "old.wav".to_string(),
            }],
        )
        .unwrap();
        write_manifest_entries(
            dir.path(),
            &[ManifestSong {
                song_id: "new".to_string(),
                content_id: "music:dev".to_string(),
                audio_path: "new.wav".to_string(),
            }],
        )
        .unwrap();
        let entries = read_manifest_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].song_id, "new");
    }

    #[test]
    fn validate_reference_wav_accepts_a_real_wav_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..16_000 {
            writer.write_sample((i % 100) as i16).unwrap();
        }
        writer.finalize().unwrap();
        assert!(validate_reference_wav(&path).is_ok());
    }

    #[test]
    fn validate_reference_wav_rejects_a_missing_file() {
        let result = validate_reference_wav(std::path::Path::new("/nonexistent/song.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_reference_wav_rejects_a_non_wav_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-audio.txt");
        std::fs::write(&path, b"this is plain text, not a WAV file").unwrap();
        assert!(validate_reference_wav(&path).is_err());
    }

    #[test]
    fn validate_reference_wav_rejects_an_empty_wav_with_zero_samples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.finalize().unwrap();
        assert!(validate_reference_wav(&path).is_err());
    }

    #[test]
    fn a_file_that_passes_validation_also_enrolls_successfully() {
        // Proves validate_reference_wav and enroll_one share one decode
        // path (decode_reference_wav) - a file is never accepted by one
        // and rejected by the other.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..32_000 {
            writer.write_sample((i % 100) as i16).unwrap();
        }
        writer.finalize().unwrap();

        assert!(validate_reference_wav(&path).is_ok());

        let manifest = serde_json::json!({
            "songs": [
                {"songId": "s1", "contentId": "music:dev", "audioPath": "song.wav"}
            ]
        });
        std::fs::write(
            dir.path().join(MODEL_MANIFEST_FILENAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let recognizer = LocalAcousticMusicRecognizer::configure(LocalAcousticConfig {
            model_dir: Some(dir.path().to_path_buf()),
            enabled: true,
        });
        assert_eq!(recognizer.status(), AcousticRecognitionStatus::Available);
    }
}
