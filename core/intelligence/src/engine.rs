//! The [`IntelligenceEngine`] contract itself, and the input/output/error
//! types around it. Kept synchronous and executor-free by design (Phase
//! 2.0 spec section 20) - an engine is a plain function of
//! `(IntelligenceInput, IntelligenceContext) -> Result<IntelligenceResult,
//! IntelligenceError>`, nothing more.

use crate::context::IntelligenceContext;
use crate::domain::IntelligenceDomain;
use crate::finding::IntelligenceFinding;
use cip_core_ai::TranscriptSegment;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// What the runtime this engine is analyzing under can (and cannot) do.
/// Deliberately minimal for Phase 2.0: CIP is always offline-only, so the
/// one fact worth carrying today is that guarantee itself - not a
/// speculative capability-negotiation system for engines that don't exist
/// yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub offline_only: bool,
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self { offline_only: true }
    }
}

/// What an [`IntelligenceEngine`] receives for one `analyze` call: the
/// current transcript segment plus the runtime it's operating under.
/// Everything *contextual* (recent transcript, active Scripture context,
/// recent findings, service/content state) is on [`IntelligenceContext`]
/// instead - kept as a separate parameter because it's shared,
/// engine-independent state, whereas this is the one new thing this call
/// is actually about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceInput {
    pub service_id: Uuid,
    pub transcript_segment: TranscriptSegment,
    pub runtime: RuntimeCapabilities,
}

impl IntelligenceInput {
    pub fn new(service_id: Uuid, transcript_segment: TranscriptSegment) -> Self {
        Self {
            service_id,
            transcript_segment,
            runtime: RuntimeCapabilities::default(),
        }
    }
}

/// What one `analyze` call produced. Always zero, one, or many findings -
/// never a mutation of anything outside itself. `processing_ms` is
/// optional measurement metadata (Phase 2.0 spec section 57), not a
/// correctness signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceResult {
    pub findings: Vec<IntelligenceFinding>,
    pub processing_ms: Option<u64>,
}

impl IntelligenceResult {
    pub fn new(findings: Vec<IntelligenceFinding>) -> Self {
        Self {
            findings,
            processing_ms: None,
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn with_processing_ms(mut self, ms: u64) -> Self {
        self.processing_ms = Some(ms);
        self
    }
}

/// Stable identity for one engine instance - who produced a finding, and
/// with what version. Deliberately not a model registry: just enough to
/// answer "which engine, which version" (Phase 2.0 spec section 21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineIdentity {
    pub domain: IntelligenceDomain,
    pub engine_id: String,
    pub engine_version: String,
}

/// Whether an engine is even worth calling right now. Distinguishes "not
/// installed" (`Unavailable`) from "installed but currently broken"
/// (`Error`) from "installed but turned off" (`Disabled`) - three
/// different facts an operator or diagnostic UI needs to tell apart
/// (Phase 2.0 spec section 22/35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCapability {
    Available,
    Unavailable,
    Disabled,
    Error,
}

/// The shared intelligence error boundary. Identifies the affected engine
/// so a multi-engine failure is attributable, never silently swallowed
/// into "no findings" (Phase 2.0 spec section 34).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum IntelligenceError {
    #[error("engine {engine_id} is not available to analyze ({reason})")]
    EngineUnavailable { engine_id: String, reason: String },
    #[error("engine {engine_id} failed: {reason}")]
    EngineFailed { engine_id: String, reason: String },
    #[error("no engine registered for domain {0:?}")]
    NoEngineForDomain(IntelligenceDomain),
    #[error("an engine is already registered for domain {0:?}")]
    DomainAlreadyRegistered(IntelligenceDomain),
    #[error("finding {0} not found")]
    FindingNotFound(Uuid),
    #[error("correlation {0} not found")]
    CorrelationNotFound(Uuid),
}

/// The shared contract every intelligence engine implements. Synchronous
/// and deterministic by design: given the same `input` and `context`, an
/// engine must be able to produce the same `IntelligenceResult` (modulo
/// engine-generated ids/timestamps - see the determinism test). Engines
/// must not call one another directly (Phase 2.0 spec sections 2/24/55);
/// the only channel between them is what the orchestrator puts into
/// `context.recent_findings`.
pub trait IntelligenceEngine: Send + Sync {
    fn identity(&self) -> EngineIdentity;
    fn capability(&self) -> EngineCapability;
    fn analyze(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Result<IntelligenceResult, IntelligenceError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_capabilities_default_to_offline_only() {
        assert!(RuntimeCapabilities::default().offline_only);
    }

    #[test]
    fn intelligence_result_empty_has_no_findings() {
        assert!(IntelligenceResult::empty().findings.is_empty());
    }

    #[test]
    fn engine_capability_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&EngineCapability::Unavailable).unwrap(),
            "\"unavailable\""
        );
    }

    #[test]
    fn intelligence_error_identifies_the_affected_engine() {
        let err = IntelligenceError::EngineFailed {
            engine_id: "music".to_string(),
            reason: "not implemented".to_string(),
        };
        assert!(err.to_string().contains("music"));
    }
}
