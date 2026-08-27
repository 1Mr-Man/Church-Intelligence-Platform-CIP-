//! Phase 3.3 pilot-qualification evidence model.
//!
//! This module exists for exactly one reason (spec section 37): to make
//! it structurally impossible for a future phase to accidentally convert
//! automated-test or Xvfb success into a hardware-verified claim.
//! `docs/phase-3-3-pilot-qualification.md` is the actual qualification
//! report this session produced (a document, not code) - this module is
//! the small, testable guardrail that report's central claim rests on:
//! "hardware qualification can only ever be satisfied by real-hardware
//! evidence, however many automated/Xvfb records exist."
//!
//! Deliberately minimal - no persistence, no Tauri command, no new
//! architecture. It is not wired into the running app at all; it exists
//! to be unit-tested, and to be the one place a human (or a future
//! Claude session) can point at to see the rule enforced in code rather
//! than only asserted in prose.
//!
//! `#[allow(dead_code)]` below is deliberate, not an oversight: every
//! item here is exercised by this module's own test suite (the actual
//! deliverable this phase's spec asked for), but nothing in the running
//! application calls it yet - there is no existing "pilot evidence
//! package importer" command to wire it into, and inventing one only to
//! silence this lint would be exactly the speculative functionality this
//! phase's spec prohibits. A future phase that builds real tooling
//! around `pilot-evidence/` (spec section 26) is the natural place to
//! start calling this.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Which of the three evidence classes this project's own governing spec
/// (Phase 3.1 onward) has always distinguished, given a name here for
/// the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceEnvironment {
    /// `cargo test` / `vitest` - proves code correctness only.
    Automated,
    /// A real binary launched under a virtual display (Xvfb) - proves
    /// desktop/runtime correctness only.
    Xvfb,
    /// An actual church laptop, microphone, model, and
    /// monitor/projector - the only environment that can prove physical
    /// pilot readiness.
    RealHardware,
}

/// The five states a single qualification check can honestly be in.
/// Deliberately does not include a generic "verified" - every claim must
/// say which of these it is (spec section 30's evidence-confidence
/// labels, given a machine-checkable form here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationStatus {
    NotTested,
    Pass,
    Fail,
    BlockedHardware,
    NotApplicable,
}

/// One piece of evidence for one capability, from one environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub environment: EvidenceEnvironment,
    pub status: QualificationStatus,
    /// Free-text pointer to where this evidence lives (a test name, a
    /// log file, a pilot-evidence-package file) - never the evidence
    /// itself; this module only carries the classification, not the
    /// payload.
    pub note: String,
}

impl EvidenceRecord {
    pub fn new(
        environment: EvidenceEnvironment,
        status: QualificationStatus,
        note: impl Into<String>,
    ) -> Self {
        Self {
            environment,
            status,
            note: note.into(),
        }
    }
}

/// The central guardrail (spec section 37): derives a capability's
/// **hardware** qualification status from a set of evidence records
/// spanning any mix of environments.
///
/// The rule, exactly: a `RealHardware` record's own status is the
/// answer, full stop. Any number of `Automated`/`Xvfb` `Pass` records -
/// even every one of them - can never produce anything other than
/// `BlockedHardware` here if no `RealHardware` record exists. A
/// `RealHardware` `Fail` always wins over everything else, since a
/// capability that has genuinely failed on real hardware cannot be
/// rescued by unrelated automated/Xvfb passes.
pub fn hardware_qualification_status(records: &[EvidenceRecord]) -> QualificationStatus {
    let real_hardware_records: Vec<&EvidenceRecord> = records
        .iter()
        .filter(|r| r.environment == EvidenceEnvironment::RealHardware)
        .collect();

    if real_hardware_records
        .iter()
        .any(|r| r.status == QualificationStatus::Fail)
    {
        return QualificationStatus::Fail;
    }

    match real_hardware_records.first() {
        Some(r) => r.status,
        None => QualificationStatus::BlockedHardware,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical acceptance test the spec itself asks for (section
    /// 37): automated success alone must never satisfy hardware
    /// qualification, however many automated records exist and however
    /// unanimously they pass.
    #[test]
    fn automated_pass_never_satisfies_hardware_qualification() {
        let records = vec![
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "cargo test: microphone enumeration",
            ),
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "cargo test: stream-error handling",
            ),
        ];
        assert_eq!(
            hardware_qualification_status(&records),
            QualificationStatus::BlockedHardware,
            "automated PASS must never become hardware PASS"
        );
    }

    /// The same rule, restated for Xvfb - the spec's other explicitly
    /// named false-positive risk.
    #[test]
    fn xvfb_pass_never_satisfies_hardware_qualification() {
        let records = vec![EvidenceRecord::new(
            EvidenceEnvironment::Xvfb,
            QualificationStatus::Pass,
            "Xvfb: presentation window opened and rendered",
        )];
        assert_eq!(
            hardware_qualification_status(&records),
            QualificationStatus::BlockedHardware,
            "Xvfb PASS must never become hardware PASS"
        );
    }

    /// Mixing every non-hardware environment together still can't do it.
    #[test]
    fn every_non_hardware_environment_combined_still_blocks() {
        let records = vec![
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "a",
            ),
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "b",
            ),
            EvidenceRecord::new(EvidenceEnvironment::Xvfb, QualificationStatus::Pass, "c"),
            EvidenceRecord::new(EvidenceEnvironment::Xvfb, QualificationStatus::Pass, "d"),
        ];
        assert_eq!(
            hardware_qualification_status(&records),
            QualificationStatus::BlockedHardware
        );
    }

    /// A real hardware record is the only thing that can produce Pass.
    #[test]
    fn a_real_hardware_pass_record_produces_pass() {
        let records = vec![
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "a",
            ),
            EvidenceRecord::new(
                EvidenceEnvironment::RealHardware,
                QualificationStatus::Pass,
                "real microphone captured a test phrase on the pilot laptop",
            ),
        ];
        assert_eq!(
            hardware_qualification_status(&records),
            QualificationStatus::Pass
        );
    }

    /// A real hardware failure is never masked by unrelated passing
    /// automated/Xvfb evidence.
    #[test]
    fn a_real_hardware_failure_is_never_masked_by_other_passing_evidence() {
        let records = vec![
            EvidenceRecord::new(
                EvidenceEnvironment::Automated,
                QualificationStatus::Pass,
                "a",
            ),
            EvidenceRecord::new(EvidenceEnvironment::Xvfb, QualificationStatus::Pass, "b"),
            EvidenceRecord::new(
                EvidenceEnvironment::RealHardware,
                QualificationStatus::Fail,
                "microphone connected but produced no audio chunks",
            ),
        ];
        assert_eq!(
            hardware_qualification_status(&records),
            QualificationStatus::Fail
        );
    }

    /// No evidence at all is exactly the same as "hardware unavailable" -
    /// never silently `NotTested` or, worse, `Pass`.
    #[test]
    fn no_evidence_at_all_is_blocked_hardware_not_not_tested() {
        assert_eq!(
            hardware_qualification_status(&[]),
            QualificationStatus::BlockedHardware
        );
    }
}
