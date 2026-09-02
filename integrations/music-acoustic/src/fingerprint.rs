//! A real, offline audio-fingerprinting algorithm: spectral landmark
//! (constellation) hashing, the same family of technique described in
//! Wang, "An Industrial-Strength Audio Search Algorithm" (2003) - the
//! algorithm behind Shazam. This is the genuine backend
//! `LocalAcousticMusicRecognizer`'s module docs describe as the deferred
//! "future phase plugs in a real backend" seam.
//!
//! ## Why this design
//!
//! A landmark hash is built from *pairs* of nearby spectral peaks (an
//! anchor and a target), not from a peak's absolute time - so the same
//! hash appears whether the matching audio starts at second 0 of a
//! reference recording or is excerpted from the middle of a live
//! service's microphone feed. Alignment is then recovered by voting on
//! `reference_time - query_time` per matching hash: a genuine match
//! produces many hashes voting for the *same* offset (the query is one
//! contiguous clip of the reference, so every landmark inside it is
//! offset by the same amount); random hash collisions between unrelated
//! songs scatter across many different offsets and never accumulate a
//! majority. This offset-histogram step is what makes fingerprinting
//! resistant to the kind of coincidental single-hash collision a raw
//! "hash present/absent" lookup would be fooled by.
//!
//! ## What this module does NOT do
//!
//! It has no notion of a song, dataset, or file - it operates purely on
//! `i16` PCM samples in and `Hash`/`Landmark`/`FingerprintIndex` types
//! out. `local.rs` is where enrollment (reading real reference audio,
//! associating fingerprints with a song/content id) and the
//! `AcousticMusicRecognizer` trait live.

use std::collections::HashMap;

use rustfft::{num_complex::Complex32, FftPlanner};

/// One STFT analysis window's worth of samples, and the hop between
/// windows - both fixed, documented constants rather than tunables, since
/// changing either invalidates every previously-enrolled fingerprint
/// (the hash space is derived from bin/frame indices at these exact
/// sizes). A future phase that wants configurability would need to also
/// version the fingerprint format.
pub const WINDOW_SIZE: usize = 1024;
pub const HOP_SIZE: usize = 512;

/// The usable spectrum (only the first half of `WINDOW_SIZE` bins carries
/// information for real-valued input - the rest mirrors it, per the
/// standard real-FFT symmetry).
const USABLE_BINS: usize = WINDOW_SIZE / 2;

/// Logarithmically-spaced frequency bands peaks are picked from - low
/// bands are narrow (speech/vocal fundamentals crowd here and would
/// dominate a linear split), high bands are wide. Picking at most one
/// peak per band per frame keeps the constellation sparse (a handful of
/// points per frame, not hundreds), which is what keeps a landmark hash
/// selective rather than swamped with noise-floor bins.
const BAND_EDGES: [usize; 7] = [1, 10, 20, 40, 80, 160, USABLE_BINS];

/// How many frames ahead of an anchor peak a target peak may be paired
/// with, and how many target peaks each anchor fans out to - both from
/// the same Wang (2003) design: a small, bounded target zone keeps the
/// number of hashes linear in the number of peaks rather than quadratic,
/// while still needing many matching hashes (not one) to establish a
/// genuine match.
const TARGET_ZONE_MIN_DELTA: usize = 1;
const TARGET_ZONE_MAX_DELTA: usize = 32;
const FAN_OUT: usize = 3;

/// One spectral peak: which analysis frame it came from, which frequency
/// bin within that frame, and its magnitude (kept only for tie-breaking
/// within a band - never compared across frames or used at match time).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Peak {
    frame: usize,
    bin: usize,
    magnitude: f32,
}

/// A landmark hash: two frequency bins and the frame delta between the
/// anchor and target peak that produced them, packed into one `u32` so
/// it can be used directly as a `HashMap` key without a wrapper struct.
/// Packing: `bin1` (10 bits) | `bin2` (10 bits) | `delta` (6 bits) - all
/// three inputs fit comfortably (`USABLE_BINS` is 512, `TARGET_ZONE_MAX_DELTA`
/// is 32), so this is lossless for every value this module ever produces.
pub type LandmarkHash = u32;

fn pack_hash(bin1: usize, bin2: usize, delta: usize) -> LandmarkHash {
    debug_assert!(bin1 < 1024 && bin2 < 1024 && delta < 64);
    ((bin1 as u32) << 16) | ((bin2 as u32) << 6) | (delta as u32)
}

/// One landmark extracted from a clip: its hash, and the frame index of
/// its *anchor* peak (the time this hash is anchored to, used for offset
/// voting at match time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landmark {
    pub hash: LandmarkHash,
    pub anchor_frame: usize,
}

/// Compute the Hann-windowed magnitude spectrum for every `WINDOW_SIZE`
/// frame across `samples`, hopping by `HOP_SIZE`. Pure and deterministic:
/// the same samples always produce the same frames, in the same order.
fn compute_spectrogram(samples: &[i16]) -> Vec<Vec<f32>> {
    if samples.len() < WINDOW_SIZE {
        return Vec::new();
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW_SIZE);

    let window: Vec<f32> = (0..WINDOW_SIZE)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (WINDOW_SIZE - 1) as f32).cos())
        })
        .collect();

    let mut frames = Vec::new();
    let mut start = 0;
    while start + WINDOW_SIZE <= samples.len() {
        let mut buffer: Vec<Complex32> = samples[start..start + WINDOW_SIZE]
            .iter()
            .zip(window.iter())
            .map(|(&s, &w)| Complex32::new(f32::from(s) * w, 0.0))
            .collect();
        fft.process(&mut buffer);
        let magnitudes: Vec<f32> = buffer[..USABLE_BINS].iter().map(|c| c.norm()).collect();
        frames.push(magnitudes);
        start += HOP_SIZE;
    }
    frames
}

/// Pick at most one peak (the strongest bin) per frequency band, per
/// frame, from a spectrogram - the "constellation" of a clip. Skips a
/// band entirely if every bin in it is at or below `magnitude` zero (true
/// silence in that band), rather than reporting a meaningless peak.
fn pick_peaks(spectrogram: &[Vec<f32>]) -> Vec<Peak> {
    let mut peaks = Vec::new();
    for (frame_idx, magnitudes) in spectrogram.iter().enumerate() {
        for window in BAND_EDGES.windows(2) {
            let (lo, hi) = (window[0], window[1]);
            if lo >= hi || hi > magnitudes.len() {
                continue;
            }
            let mut best_bin = lo;
            let mut best_mag = magnitudes[lo];
            for (bin, &mag) in magnitudes.iter().enumerate().take(hi).skip(lo + 1) {
                if mag > best_mag {
                    best_mag = mag;
                    best_bin = bin;
                }
            }
            if best_mag > 0.0 {
                peaks.push(Peak {
                    frame: frame_idx,
                    bin: best_bin,
                    magnitude: best_mag,
                });
            }
        }
    }
    peaks
}

/// Pair each peak (as an anchor) with up to `FAN_OUT` nearby later peaks
/// (as targets) within the target zone, producing one landmark hash per
/// pair. Peaks are assumed sorted by frame (true by construction - see
/// `pick_peaks`'s outer loop order).
fn build_landmarks(peaks: &[Peak]) -> Vec<Landmark> {
    let mut landmarks = Vec::new();
    for (i, anchor) in peaks.iter().enumerate() {
        let mut fanned = 0;
        for target in peaks.iter().skip(i + 1) {
            let delta = target.frame.saturating_sub(anchor.frame);
            if delta < TARGET_ZONE_MIN_DELTA {
                continue;
            }
            if delta > TARGET_ZONE_MAX_DELTA {
                break;
            }
            landmarks.push(Landmark {
                hash: pack_hash(anchor.bin, target.bin, delta),
                anchor_frame: anchor.frame,
            });
            fanned += 1;
            if fanned >= FAN_OUT {
                break;
            }
        }
    }
    landmarks
}

/// Extract every landmark from a clip of raw mono PCM16 audio - the one
/// entry point both enrollment (`FingerprintIndex::enroll`) and query
/// (`FingerprintIndex::query`) use, so a reference recording and a live
/// excerpt of it are guaranteed to be hashed identically.
pub fn fingerprint(samples: &[i16]) -> Vec<Landmark> {
    let spectrogram = compute_spectrogram(samples);
    let peaks = pick_peaks(&spectrogram);
    build_landmarks(&peaks)
}

/// An in-memory index from landmark hash to every `(song_id, anchor_frame)`
/// it was seen at during enrollment - deliberately just a `HashMap`, not a
/// database: this is a per-process, rebuild-on-startup structure (mirroring
/// how `core/bible::semantic`'s in-memory verse embeddings work), not
/// something queried across process restarts.
#[derive(Debug, Default)]
pub struct FingerprintIndex {
    table: HashMap<LandmarkHash, Vec<(String, usize)>>,
}

/// The result of matching a query clip against everything enrolled so
/// far - one entry per song whose votes cleared `min_votes`, sorted by
/// vote count descending (`FingerprintIndex::query`'s contract).
#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintMatch {
    pub song_id: String,
    /// How many landmark hashes agreed on the same alignment offset -
    /// the "votes" behind this match, not a 0..1 confidence. Callers
    /// (see `local.rs`) turn this into a `ConfidenceResult` themselves,
    /// since what counts as "enough" votes is legitimately a tunable
    /// policy decision, not an algorithmic fact this module should bake
    /// in.
    pub votes: usize,
    pub total_query_landmarks: usize,
}

impl FingerprintIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add every landmark from a reference clip's samples to the index,
    /// tagged with `song_id`. Enrolling the same `song_id` more than once
    /// (e.g. two reference clips for one song) is additive, not
    /// replacing - both contribute votes at query time.
    pub fn enroll(&mut self, song_id: &str, samples: &[i16]) {
        for landmark in fingerprint(samples) {
            self.table
                .entry(landmark.hash)
                .or_default()
                .push((song_id.to_string(), landmark.anchor_frame));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Match a query clip's samples against everything enrolled, via
    /// hash lookup + offset-histogram voting (see this module's docs).
    /// Returns matches with at least `min_votes`, sorted by vote count
    /// descending, ties broken by `song_id` for determinism.
    pub fn query(&self, samples: &[i16], min_votes: usize) -> Vec<FingerprintMatch> {
        let query_landmarks = fingerprint(samples);
        let total_query_landmarks = query_landmarks.len();
        if total_query_landmarks == 0 {
            return Vec::new();
        }

        // (song_id, reference_anchor_frame - query_anchor_frame) -> votes.
        // Using i64 for the offset since a query clip's anchor frames can
        // exceed a short reference recording's, making the difference
        // negative.
        let mut offset_votes: HashMap<(String, i64), usize> = HashMap::new();
        for landmark in &query_landmarks {
            let Some(hits) = self.table.get(&landmark.hash) else {
                continue;
            };
            for (song_id, reference_frame) in hits {
                let offset = *reference_frame as i64 - landmark.anchor_frame as i64;
                *offset_votes.entry((song_id.clone(), offset)).or_insert(0) += 1;
            }
        }

        let mut best_per_song: HashMap<String, usize> = HashMap::new();
        for ((song_id, _offset), votes) in offset_votes {
            let entry = best_per_song.entry(song_id).or_insert(0);
            if votes > *entry {
                *entry = votes;
            }
        }

        let mut matches: Vec<FingerprintMatch> = best_per_song
            .into_iter()
            .filter(|(_, votes)| *votes >= min_votes)
            .map(|(song_id, votes)| FingerprintMatch {
                song_id,
                votes,
                total_query_landmarks,
            })
            .collect();
        matches.sort_by(|a, b| {
            b.votes
                .cmp(&a.votes)
                .then_with(|| a.song_id.cmp(&b.song_id))
        });
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// A synthetic multi-tone "song" - a fixed sum of sine waves at
    /// several frequencies, long enough to span many analysis windows.
    /// Deterministic and self-contained (no audio file dependency), the
    /// same way this project's other pure-logic tests build synthetic
    /// fixtures rather than depending on real recordings.
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

    fn white_noise(n: usize, amplitude: i16, seed: u64) -> Vec<i16> {
        // A tiny deterministic LCG - good enough for "add some noise" in
        // a unit test, and reproducible across runs/platforms, unlike
        // pulling in a `rand` dependency for one test helper.
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let normalized =
                    ((state >> 33) as i64 % (2 * i64::from(amplitude) + 1)) - i64::from(amplitude);
                normalized as i16
            })
            .collect()
    }

    const SR: u32 = 16_000;

    #[test]
    fn empty_samples_produce_no_landmarks() {
        assert!(fingerprint(&[]).is_empty());
    }

    #[test]
    fn samples_shorter_than_one_window_produce_no_landmarks() {
        let short = vec![1_000_i16; WINDOW_SIZE - 1];
        assert!(fingerprint(&short).is_empty());
    }

    #[test]
    fn a_long_enough_tone_produces_landmarks() {
        let song = synth_tone(&[440.0, 880.0, 1320.0], SR, 4_000, 12_000.0);
        assert!(!fingerprint(&song).is_empty());
    }

    #[test]
    fn a_clip_matches_itself_with_a_strong_vote_count() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[440.0, 880.0, 1320.0], SR, 5_000, 12_000.0);
        index.enroll("song-a", &song);

        let matches = index.query(&song, 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].song_id, "song-a");
        // A full self-match should have a large fraction of all query
        // landmarks agreeing on the same (zero) offset - not just a
        // handful of coincidental hash collisions.
        assert!(matches[0].votes > matches[0].total_query_landmarks / 4);
    }

    #[test]
    fn two_acoustically_different_songs_do_not_cross_match() {
        let mut index = FingerprintIndex::new();
        let song_a = synth_tone(&[220.0, 440.0, 660.0], SR, 5_000, 12_000.0);
        let song_b = synth_tone(&[3_000.0, 4_500.0, 6_000.0], SR, 5_000, 12_000.0);
        index.enroll("song-a", &song_a);
        index.enroll("song-b", &song_b);

        // Query with song B; song A should either not match at all, or
        // match with far fewer votes than song B does against itself.
        let matches = index.query(&song_b, 1);
        let a_votes = matches
            .iter()
            .find(|m| m.song_id == "song-a")
            .map(|m| m.votes)
            .unwrap_or(0);
        let b_votes = matches
            .iter()
            .find(|m| m.song_id == "song-b")
            .map(|m| m.votes)
            .unwrap_or(0);
        assert!(b_votes > 0, "song B must match itself");
        assert!(
            b_votes > a_votes * 4,
            "unrelated song must not out-vote (or come close to) the real match: a={a_votes} b={b_votes}"
        );
    }

    #[test]
    fn a_cropped_excerpt_from_the_middle_still_matches_via_time_shift_invariance() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[350.0, 700.0, 1_050.0], SR, 8_000, 12_000.0);
        index.enroll("song-a", &song);

        // Take a 3-second excerpt starting 2 seconds in - landmark
        // hashing must recognize this via a consistent nonzero offset,
        // not require the excerpt to start at sample 0.
        let start = (SR as usize) * 2;
        let end = start + (SR as usize) * 3;
        let excerpt = song[start..end].to_vec();

        let matches = index.query(&excerpt, 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].song_id, "song-a");
        assert!(matches[0].votes > matches[0].total_query_landmarks / 4);
    }

    #[test]
    fn moderate_added_noise_still_matches() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[300.0, 600.0, 900.0, 1_200.0], SR, 5_000, 14_000.0);
        index.enroll("song-a", &song);

        let noise = white_noise(song.len(), 1_500, 42);
        let noisy: Vec<i16> = song
            .iter()
            .zip(noise.iter())
            .map(|(&s, &n)| s.saturating_add(n))
            .collect();

        let matches = index.query(&noisy, 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].song_id, "song-a");
    }

    #[test]
    fn silence_produces_no_matches_against_a_populated_index() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[440.0], SR, 4_000, 12_000.0);
        index.enroll("song-a", &song);

        let silence = vec![0_i16; SR as usize * 4];
        let matches = index.query(&silence, 1);
        assert!(matches.is_empty());
    }

    #[test]
    fn querying_an_empty_index_returns_no_matches() {
        let index = FingerprintIndex::new();
        let song = synth_tone(&[440.0], SR, 2_000, 12_000.0);
        assert!(index.query(&song, 1).is_empty());
    }

    #[test]
    fn min_votes_threshold_filters_out_weak_matches() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[440.0, 880.0], SR, 5_000, 12_000.0);
        index.enroll("song-a", &song);

        let unrealistically_high_threshold = usize::MAX;
        assert!(index
            .query(&song, unrealistically_high_threshold)
            .is_empty());
    }

    #[test]
    fn enrolling_the_same_song_twice_is_additive_not_replacing() {
        let mut index = FingerprintIndex::new();
        let song = synth_tone(&[500.0, 1_000.0], SR, 3_000, 12_000.0);
        index.enroll("song-a", &song);
        let landmarks_after_one = index.table.values().map(Vec::len).sum::<usize>();
        index.enroll("song-a", &song);
        let landmarks_after_two = index.table.values().map(Vec::len).sum::<usize>();
        assert_eq!(landmarks_after_two, landmarks_after_one * 2);
    }

    #[test]
    fn hash_packing_is_lossless_for_realistic_inputs() {
        let hash = pack_hash(511, 511, 32);
        assert_eq!(hash, pack_hash(511, 511, 32));
        let different = pack_hash(511, 510, 32);
        assert_ne!(hash, different);
    }
}
