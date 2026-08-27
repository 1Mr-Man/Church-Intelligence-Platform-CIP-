//! [`IntelligenceEngineRegistry`]: an in-process registry of engines, one
//! per domain. No dynamic plugin loading, no network-loaded engines, no
//! executor - engines are registered at startup and resolved/called
//! in-process (Phase 2.0 spec section 23).

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::context::IntelligenceContext;
use crate::domain::IntelligenceDomain;
use crate::engine::{
    EngineCapability, EngineIdentity, IntelligenceEngine, IntelligenceError, IntelligenceInput,
    IntelligenceResult,
};

/// The outcome of calling one engine during [`IntelligenceEngineRegistry::analyze_all`] -
/// always present for every registered engine, whether it succeeded,
/// returned an error, was skipped as unavailable, or panicked. Nothing is
/// ever silently dropped.
#[derive(Debug)]
pub struct EngineOutcome {
    pub identity: EngineIdentity,
    pub result: Result<IntelligenceResult, IntelligenceError>,
}

/// One engine per [`IntelligenceDomain`], in-process only. Registration is
/// explicit and rejects a duplicate domain rather than silently replacing
/// an existing engine (matching this codebase's established "never
/// silently overwrite" discipline - see `docs/bible-datasets.md`'s
/// importer section for the same rule applied to content).
#[derive(Default)]
pub struct IntelligenceEngineRegistry {
    engines: HashMap<IntelligenceDomain, Box<dyn IntelligenceEngine>>,
}

impl IntelligenceEngineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        engine: Box<dyn IntelligenceEngine>,
    ) -> Result<(), IntelligenceError> {
        let domain = engine.identity().domain;
        if self.engines.contains_key(&domain) {
            return Err(IntelligenceError::DomainAlreadyRegistered(domain));
        }
        self.engines.insert(domain, engine);
        Ok(())
    }

    pub fn resolve(&self, domain: IntelligenceDomain) -> Option<&dyn IntelligenceEngine> {
        self.engines.get(&domain).map(|e| e.as_ref())
    }

    pub fn list(&self) -> Vec<EngineIdentity> {
        self.engines.values().map(|e| e.identity()).collect()
    }

    /// The capability of every registered engine, keyed by domain. A
    /// domain with no registered engine simply has no entry - callers
    /// distinguish "not registered at all" from "registered but
    /// `Unavailable`/`Disabled`/`Error`" by checking for the domain's
    /// presence, not by a placeholder value (Phase 2.0 spec section 22).
    pub fn capabilities(&self) -> HashMap<IntelligenceDomain, EngineCapability> {
        self.engines
            .iter()
            .map(|(domain, engine)| (*domain, engine.capability()))
            .collect()
    }

    /// Run every registered, `Available` engine against the same `input`/
    /// `context`, isolating each engine's failure - including a panic -
    /// from every other engine and from the caller (Phase 2.0 spec
    /// sections 33/50). One `EngineOutcome` per registered engine, always,
    /// so a caller can see exactly what happened to each one.
    pub fn analyze_all(
        &self,
        input: &IntelligenceInput,
        context: &IntelligenceContext,
    ) -> Vec<EngineOutcome> {
        self.engines
            .values()
            .map(|engine| {
                let identity = engine.identity();
                let capability = engine.capability();
                if capability != EngineCapability::Available {
                    return EngineOutcome {
                        identity: identity.clone(),
                        result: Err(IntelligenceError::EngineUnavailable {
                            engine_id: identity.engine_id.clone(),
                            reason: format!("{capability:?}"),
                        }),
                    };
                }

                let outcome = catch_unwind(AssertUnwindSafe(|| engine.analyze(input, context)));
                let result = outcome.unwrap_or_else(|_| {
                    Err(IntelligenceError::EngineFailed {
                        engine_id: identity.engine_id.clone(),
                        reason: "engine panicked during analyze()".to_string(),
                    })
                });
                EngineOutcome { identity, result }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AssertionLevel, FindingKind};
    use crate::finding::IntelligenceFinding;
    use cip_core_ai::TranscriptSegment;
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};
    use uuid::Uuid;

    fn segment() -> TranscriptSegment {
        TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: "test".to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 0,
            end_ms: 500,
            language: Some("en".to_string()),
            speaker_id: None,
        }
    }

    fn input() -> IntelligenceInput {
        IntelligenceInput::new(Uuid::new_v4(), segment())
    }

    fn context(service_id: uuid::Uuid) -> IntelligenceContext {
        IntelligenceContext::build(
            service_id,
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            crate::context::ContextBounds::default(),
        )
    }

    struct WorkingEngine {
        domain: IntelligenceDomain,
        capability: EngineCapability,
    }

    impl IntelligenceEngine for WorkingEngine {
        fn identity(&self) -> EngineIdentity {
            EngineIdentity {
                domain: self.domain,
                engine_id: format!("{:?}", self.domain).to_lowercase(),
                engine_version: "1.0".to_string(),
            }
        }
        fn capability(&self) -> EngineCapability {
            self.capability
        }
        fn analyze(
            &self,
            input: &IntelligenceInput,
            _context: &IntelligenceContext,
        ) -> Result<IntelligenceResult, IntelligenceError> {
            let finding = IntelligenceFinding::new(
                input.service_id,
                self.domain,
                FindingKind::Scripture,
                AssertionLevel::Suggested,
                ConfidenceResult::new(0.9, ConfidenceSource::Heuristic, None),
                "test finding",
                self.identity().engine_id,
                "1.0",
            );
            Ok(IntelligenceResult::new(vec![finding]))
        }
    }

    struct FailingEngine;

    impl IntelligenceEngine for FailingEngine {
        fn identity(&self) -> EngineIdentity {
            EngineIdentity {
                domain: IntelligenceDomain::Music,
                engine_id: "music".to_string(),
                engine_version: "0.1".to_string(),
            }
        }
        fn capability(&self) -> EngineCapability {
            EngineCapability::Available
        }
        fn analyze(
            &self,
            _input: &IntelligenceInput,
            _context: &IntelligenceContext,
        ) -> Result<IntelligenceResult, IntelligenceError> {
            Err(IntelligenceError::EngineFailed {
                engine_id: "music".to_string(),
                reason: "simulated failure".to_string(),
            })
        }
    }

    struct PanickingEngine;

    impl IntelligenceEngine for PanickingEngine {
        fn identity(&self) -> EngineIdentity {
            EngineIdentity {
                domain: IntelligenceDomain::Sermon,
                engine_id: "sermon".to_string(),
                engine_version: "0.1".to_string(),
            }
        }
        fn capability(&self) -> EngineCapability {
            EngineCapability::Available
        }
        fn analyze(
            &self,
            _input: &IntelligenceInput,
            _context: &IntelligenceContext,
        ) -> Result<IntelligenceResult, IntelligenceError> {
            panic!("simulated engine panic");
        }
    }

    struct PanickingServiceEngine;

    impl IntelligenceEngine for PanickingServiceEngine {
        fn identity(&self) -> EngineIdentity {
            EngineIdentity {
                domain: IntelligenceDomain::Service,
                engine_id: "service".to_string(),
                engine_version: "0.1".to_string(),
            }
        }
        fn capability(&self) -> EngineCapability {
            EngineCapability::Available
        }
        fn analyze(
            &self,
            _input: &IntelligenceInput,
            _context: &IntelligenceContext,
        ) -> Result<IntelligenceResult, IntelligenceError> {
            panic!("simulated engine panic");
        }
    }

    fn working(domain: IntelligenceDomain) -> Box<dyn IntelligenceEngine> {
        Box::new(WorkingEngine {
            domain,
            capability: EngineCapability::Available,
        })
    }

    #[test]
    fn registers_and_resolves_by_domain() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        assert!(registry.resolve(IntelligenceDomain::Bible).is_some());
    }

    #[test]
    fn resolving_an_unknown_domain_returns_none() {
        let registry = IntelligenceEngineRegistry::new();
        assert!(registry.resolve(IntelligenceDomain::Music).is_none());
    }

    #[test]
    fn duplicate_registration_for_the_same_domain_is_rejected() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        let err = registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap_err();
        assert!(matches!(
            err,
            IntelligenceError::DomainAlreadyRegistered(IntelligenceDomain::Bible)
        ));
    }

    #[test]
    fn list_reports_every_registered_engine() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        registry
            .register(working(IntelligenceDomain::Service))
            .unwrap();
        assert_eq!(registry.list().len(), 2);
    }

    #[test]
    fn capabilities_only_lists_registered_domains() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        let capabilities = registry.capabilities();
        assert!(capabilities.contains_key(&IntelligenceDomain::Bible));
        assert!(
            !capabilities.contains_key(&IntelligenceDomain::Music),
            "an unregistered domain must not appear at all, not as a placeholder"
        );
    }

    #[test]
    fn unavailable_engine_is_not_called_and_reports_unavailable() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(Box::new(WorkingEngine {
                domain: IntelligenceDomain::Music,
                capability: EngineCapability::Unavailable,
            }))
            .unwrap();
        let service_id = Uuid::new_v4();
        let outcomes = registry.analyze_all(&input(), &context(service_id));
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(
            outcomes[0].result,
            Err(IntelligenceError::EngineUnavailable { .. })
        ));
    }

    #[test]
    fn disabled_engine_is_not_called() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(Box::new(WorkingEngine {
                domain: IntelligenceDomain::Sermon,
                capability: EngineCapability::Disabled,
            }))
            .unwrap();
        let service_id = Uuid::new_v4();
        let outcomes = registry.analyze_all(&input(), &context(service_id));
        assert!(outcomes[0].result.is_err());
    }

    #[test]
    fn a_failing_engine_does_not_affect_a_succeeding_one() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        registry.register(Box::new(FailingEngine)).unwrap();
        let service_id = Uuid::new_v4();
        let outcomes = registry.analyze_all(&input(), &context(service_id));

        let bible = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Bible)
            .unwrap();
        let music = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Music)
            .unwrap();
        assert!(bible.result.is_ok(), "Bible result must remain available");
        assert!(music.result.is_err(), "Music failure must be observable");
    }

    #[test]
    fn a_panicking_engine_is_isolated_and_never_propagates() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        registry.register(Box::new(PanickingEngine)).unwrap();
        let service_id = Uuid::new_v4();

        // This call itself must not panic - that's the assertion.
        let outcomes = registry.analyze_all(&input(), &context(service_id));

        let bible = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Bible)
            .unwrap();
        let sermon = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Sermon)
            .unwrap();
        assert!(bible.result.is_ok());
        assert!(matches!(
            sermon.result,
            Err(IntelligenceError::EngineFailed { .. })
        ));
    }

    /// Phase 3.1 failure-injection gap #13: the same panic-isolation
    /// guarantee proven above for Music (`a_failing_engine_...`) and
    /// Sermon (`a_panicking_engine_...`) also holds when the *Service*
    /// domain is the one that fails - the registry's isolation mechanism
    /// is domain-agnostic, but until now no test exercised a failing
    /// engine specifically registered under `IntelligenceDomain::Service`.
    #[test]
    fn a_panicking_service_engine_is_isolated_and_never_propagates() {
        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(working(IntelligenceDomain::Bible))
            .unwrap();
        registry.register(Box::new(PanickingServiceEngine)).unwrap();
        let service_id = Uuid::new_v4();

        // This call itself must not panic - that's the assertion.
        let outcomes = registry.analyze_all(&input(), &context(service_id));

        let bible = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Bible)
            .unwrap();
        let service = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Service)
            .unwrap();
        assert!(bible.result.is_ok(), "Bible result must remain available");
        assert!(matches!(
            service.result,
            Err(IntelligenceError::EngineFailed { .. })
        ));
    }

    // --- Phase 2.3: the real SermonIntelligenceEngine, registered
    // alongside a real Bible engine and a deliberately failing Music
    // engine - proves failure isolation holds for the actual production
    // Sermon engine, not just the `PanickingEngine`/`FailingEngine` test
    // doubles above.

    #[test]
    fn the_real_sermon_engine_registers_and_analyzes_alongside_bible_and_a_failing_music_engine() {
        use crate::bible_adapter::BibleIntelligenceEngine;
        use crate::fixtures::FakeBibleProvider;
        use crate::sermon_adapter::SermonIntelligenceEngine;
        use cip_core_ai::TranscriptSegment;

        let mut registry = IntelligenceEngineRegistry::new();
        registry
            .register(Box::new(BibleIntelligenceEngine::new(
                Box::new(FakeBibleProvider::kjv_fixture()),
                "KJV",
            )))
            .unwrap();
        registry.register(Box::new(FailingEngine)).unwrap();
        registry
            .register(Box::new(SermonIntelligenceEngine::new()))
            .unwrap();

        let service_id = Uuid::new_v4();
        let segment = TranscriptSegment {
            id: Uuid::new_v4(),
            sequence: 0,
            text: "My first point is that faith comes by hearing.".to_string(),
            is_final: true,
            confidence: ConfidenceResult::new(0.9, ConfidenceSource::Model, None),
            start_ms: 0,
            end_ms: 900,
            language: Some("en".to_string()),
            speaker_id: None,
        };
        let outcomes = registry.analyze_all(
            &IntelligenceInput::new(service_id, segment),
            &context(service_id),
        );

        assert_eq!(outcomes.len(), 3);
        let bible = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Bible)
            .unwrap();
        let music = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Music)
            .unwrap();
        let sermon = outcomes
            .iter()
            .find(|o| o.identity.domain == IntelligenceDomain::Sermon)
            .unwrap();

        assert!(
            bible.result.is_ok(),
            "Bible must be unaffected by Music's failure"
        );
        assert!(
            music.result.is_err(),
            "Music's simulated failure must be observable"
        );
        let sermon_result = sermon
            .result
            .as_ref()
            .expect("Sermon must be unaffected by Music's failure");
        assert!(
            sermon_result
                .findings
                .iter()
                .any(|f| f.summary.starts_with("Main Point:")),
            "the real Sermon engine must have actually analyzed the segment, not just avoided crashing"
        );
    }
}
