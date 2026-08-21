use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Started,
    Paused,
    Ended,
}

/// A single live-service session: the top-level unit of work an operator
/// starts before a service and ends after it. Every transcript segment,
/// scripture detection, suggestion, and presentation item produced during
/// the service links back to a `ServiceSession` id so the whole service can
/// be reviewed or replayed afterward.
///
/// This is a plain data contract - lifecycle transitions (start/pause/end)
/// are enforced by whatever orchestrates sessions (a future
/// `ServiceSessionManager`), not by this struct itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSession {
    pub id: Uuid,
    pub title: String,
    pub status: ServiceStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl ServiceSession {
    pub fn start(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            status: ServiceStatus::Started,
            started_at: Utc::now(),
            ended_at: None,
        }
    }

    pub fn pause(&mut self) {
        self.status = ServiceStatus::Paused;
    }

    pub fn resume(&mut self) {
        self.status = ServiceStatus::Started;
    }

    pub fn end(&mut self) {
        self.status = ServiceStatus::Ended;
        self.ended_at = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_transitions_set_expected_status() {
        let mut session = ServiceSession::start("Sunday Morning");
        assert_eq!(session.status, ServiceStatus::Started);

        session.pause();
        assert_eq!(session.status, ServiceStatus::Paused);

        session.resume();
        assert_eq!(session.status, ServiceStatus::Started);

        session.end();
        assert_eq!(session.status, ServiceStatus::Ended);
        assert!(session.ended_at.is_some());
    }
}
