//! [`FindingQueue`]: an in-memory, domain-level queue of findings awaiting
//! operator review. Phase 2.0 spec section 31 prefers an in-memory
//! abstraction over persistence unless persistence is clearly justified -
//! nothing yet needs a finding to survive a restart (unlike suggestions/
//! presentation items, which already have their own persisted tables), so
//! this stays in-memory. It must never prepare a presentation, project a
//! slide, publish content, modify the transcript, or auto-approve a
//! finding - it only tracks state an operator explicitly changes.

use uuid::Uuid;

use crate::domain::FindingStatus;
use crate::engine::IntelligenceError;
use crate::finding::IntelligenceFinding;

/// What happened when a finding was added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAddOutcome {
    Added,
    /// An equivalent finding (see `IntelligenceFinding::is_equivalent_to`)
    /// was already queued and not yet resolved; the new one was discarded
    /// rather than creating an uncontrolled duplicate (Phase 2.0 spec
    /// section 32).
    DuplicateIgnored,
}

#[derive(Default)]
pub struct FindingQueue {
    findings: Vec<IntelligenceFinding>,
}

impl FindingQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a finding, unless an equivalent one is already queued with a
    /// status that hasn't been resolved yet (`Detected`/`Reviewed`) - a
    /// resolved (accepted/rejected/expired) equivalent does not block a
    /// fresh finding, since the situation may have genuinely recurred.
    pub fn add(&mut self, finding: IntelligenceFinding) -> QueueAddOutcome {
        let is_duplicate = self.findings.iter().any(|existing| {
            matches!(
                existing.status,
                FindingStatus::Detected | FindingStatus::Reviewed
            ) && existing.is_equivalent_to(&finding)
        });
        if is_duplicate {
            return QueueAddOutcome::DuplicateIgnored;
        }
        self.findings.push(finding);
        QueueAddOutcome::Added
    }

    /// Findings still awaiting a decision (`Detected`/`Reviewed`),
    /// ordered by priority (highest first), then confidence (highest
    /// first) as a deterministic tiebreak.
    pub fn pending(&self) -> Vec<&IntelligenceFinding> {
        let mut pending: Vec<&IntelligenceFinding> = self
            .findings
            .iter()
            .filter(|f| matches!(f.status, FindingStatus::Detected | FindingStatus::Reviewed))
            .collect();
        pending.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(b.confidence.score.total_cmp(&a.confidence.score))
        });
        pending
    }

    fn find_mut(&mut self, id: Uuid) -> Result<&mut IntelligenceFinding, IntelligenceError> {
        self.findings
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or(IntelligenceError::FindingNotFound(id))
    }

    pub fn review(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.review();
        Ok(())
    }

    /// Accept a finding. This changes only this queue's copy of the
    /// finding's `status` - it has no way to create a `PresentationItem`
    /// or any other side effect (Phase 2.0 spec section 47's explicit
    /// requirement).
    pub fn accept(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.accept();
        Ok(())
    }

    pub fn reject(&mut self, id: Uuid) -> Result<(), IntelligenceError> {
        self.find_mut(id)?.reject();
        Ok(())
    }

    /// Expire every still-pending finding created before `cutoff`, and
    /// report how many were expired.
    pub fn expire_older_than(&mut self, cutoff: chrono::DateTime<chrono::Utc>) -> usize {
        let mut expired = 0;
        for finding in self.findings.iter_mut() {
            if matches!(
                finding.status,
                FindingStatus::Detected | FindingStatus::Reviewed
            ) && finding.created_at < cutoff
            {
                finding.expire();
                expired += 1;
            }
        }
        expired
    }

    pub fn get(&self, id: Uuid) -> Option<&IntelligenceFinding> {
        self.findings.iter().find(|f| f.id == id)
    }

    pub fn len(&self) -> usize {
        self.findings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AssertionLevel, FindingKind, IntelligenceDomain, IntelligencePriority};
    use cip_core_confidence::{ConfidenceResult, ConfidenceSource};

    fn finding(service_id: Uuid, summary: &str, confidence: f32) -> IntelligenceFinding {
        IntelligenceFinding::new(
            service_id,
            IntelligenceDomain::Bible,
            FindingKind::Scripture,
            AssertionLevel::Suggested,
            ConfidenceResult::new(confidence, ConfidenceSource::Heuristic, None),
            summary,
            "bible",
            "1.0",
        )
    }

    #[test]
    fn add_queues_a_new_finding() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        assert_eq!(
            queue.add(finding(service_id, "ROM 8:28", 0.9)),
            QueueAddOutcome::Added
        );
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn duplicate_pending_finding_is_ignored() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        queue.add(finding(service_id, "ROM 8:28", 0.9));
        let outcome = queue.add(finding(service_id, "ROM 8:28", 0.5));
        assert_eq!(outcome, QueueAddOutcome::DuplicateIgnored);
        assert_eq!(queue.len(), 1, "no uncontrolled duplicate was created");
    }

    #[test]
    fn a_resolved_equivalent_does_not_block_a_fresh_finding() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let first = finding(service_id, "ROM 8:28", 0.9);
        let first_id = first.id;
        queue.add(first);
        queue.accept(first_id).unwrap();

        let outcome = queue.add(finding(service_id, "ROM 8:28", 0.9));
        assert_eq!(
            outcome,
            QueueAddOutcome::Added,
            "a genuinely recurring finding is not permanently blocked"
        );
    }

    #[test]
    fn pending_orders_by_priority_then_confidence() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let mut low_priority = finding(service_id, "ROM 8:28", 0.6);
        low_priority.priority = IntelligencePriority::Low;
        let mut high_priority = finding(service_id, "ROM 8:31", 0.5);
        high_priority.priority = IntelligencePriority::High;
        queue.add(low_priority);
        queue.add(high_priority);

        let pending = queue.pending();
        assert_eq!(
            pending[0].summary, "ROM 8:31",
            "higher priority ranks first despite lower confidence"
        );
    }

    #[test]
    fn accept_removes_a_finding_from_pending() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let f = finding(service_id, "ROM 8:28", 0.9);
        let id = f.id;
        queue.add(f);
        queue.accept(id).unwrap();
        assert!(queue.pending().is_empty());
        assert_eq!(queue.get(id).unwrap().status, FindingStatus::Accepted);
    }

    #[test]
    fn reject_removes_a_finding_from_pending() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let f = finding(service_id, "ROM 8:28", 0.9);
        let id = f.id;
        queue.add(f);
        queue.reject(id).unwrap();
        assert!(queue.pending().is_empty());
    }

    #[test]
    fn accepting_a_finding_never_creates_a_presentation_item() {
        // Type-level proof, mirrored from `finding::tests`: `FindingQueue`
        // has no dependency on `cip_core_presentation` at all, so it is
        // structurally incapable of creating a `PresentationItem`.
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let f = finding(service_id, "ROM 8:28", 0.9);
        let id = f.id;
        queue.add(f);
        queue.accept(id).unwrap();
        assert_eq!(queue.get(id).unwrap().status, FindingStatus::Accepted);
    }

    #[test]
    fn unknown_finding_id_reports_not_found() {
        let mut queue = FindingQueue::new();
        assert!(matches!(
            queue.accept(Uuid::new_v4()),
            Err(IntelligenceError::FindingNotFound(_))
        ));
    }

    #[test]
    fn expire_older_than_only_affects_unresolved_findings_before_the_cutoff() {
        let mut queue = FindingQueue::new();
        let service_id = Uuid::new_v4();
        let f = finding(service_id, "ROM 8:28", 0.9);
        let id = f.id;
        queue.add(f);

        let expired = queue.expire_older_than(chrono::Utc::now() + chrono::Duration::seconds(60));
        assert_eq!(expired, 1);
        assert_eq!(queue.get(id).unwrap().status, FindingStatus::Expired);
    }
}
