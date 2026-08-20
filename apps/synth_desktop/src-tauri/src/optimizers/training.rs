//! Canonical hosted-training event model consumed from Synth Cloud.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const TRAINING_EVENT_SCHEMA_VERSION: &str = "training.event.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrainingEventProducer {
    pub service: String,
    pub version: String,
    pub commit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrainingEvent {
    pub schema_version: String,
    pub event_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub sequence: u64,
    pub occurred_at: String,
    pub kind: String,
    pub phase: String,
    #[serde(default)]
    pub payload: Value,
    pub producer: TrainingEventProducer,
}

impl TrainingEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TRAINING_EVENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported training event schema {:?}",
                self.schema_version
            ));
        }
        if self.sequence == 0
            || [
                self.event_id.as_str(),
                self.job_id.as_str(),
                self.attempt_id.as_str(),
                self.kind.as_str(),
                self.phase.as_str(),
                self.producer.service.as_str(),
                self.producer.version.as_str(),
                self.producer.commit.as_str(),
            ]
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err("training event is missing required identity".into());
        }
        if !(self.occurred_at.ends_with('Z') || self.occurred_at.ends_with("+00:00")) {
            return Err("training event timestamp must be UTC".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrainingLifecycle {
    #[default]
    Draft,
    Validating,
    Queued,
    Provisioning,
    Running,
    EnvUnreachable,
    Checkpointing,
    Evaluating,
    Cancelling,
    Cancelled,
    Paused,
    Completed,
    Degraded,
    FailedEvidence,
    Failed,
    InfrastructureLost,
    CapReached,
}

impl TrainingLifecycle {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Completed
                | Self::Degraded
                | Self::FailedEvidence
                | Self::Failed
                | Self::InfrastructureLost
                | Self::CapReached
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TrainingProjection {
    pub lifecycle: TrainingLifecycle,
    pub phase: Option<String>,
    pub last_sequence: u64,
    pub last_event_id: Option<String>,
    pub attempt_id: Option<String>,
    pub metrics: BTreeMap<String, f64>,
    pub checkpoints: Vec<Value>,
    pub warnings: Vec<Value>,
    pub latest_rollout: Option<Value>,
    pub tunnel_health: Option<Value>,
    pub provider_usage: Option<Value>,
    pub terminal_summary: Option<Value>,
    pub attempt_history: Vec<Value>,
}

impl TrainingProjection {
    /// Idempotent, order-aware reducer. Unknown event kinds still advance the
    /// durable cursor and remain forward-compatible.
    pub fn apply(&mut self, event: &TrainingEvent) -> Result<(), String> {
        event.validate()?;
        if event.sequence == self.last_sequence {
            if self.last_event_id.as_deref() != Some(event.event_id.as_str()) {
                return Err("training event sequence collision".into());
            }
            return Ok(());
        }
        if event.sequence < self.last_sequence {
            return Ok(());
        }
        if self.last_sequence > 0 && event.sequence != self.last_sequence + 1 {
            return Err("training event sequence gap".into());
        }
        if let Some(attempt_id) = self.attempt_id.as_deref() {
            if attempt_id != event.attempt_id {
                if event.kind != "job.accepted" || !self.lifecycle.terminal() {
                    return Err("training event attempt changed outside resume boundary".into());
                }
                self.attempt_history.push(serde_json::json!({
                    "attempt_id": attempt_id,
                    "terminal_lifecycle": self.lifecycle,
                    "last_sequence": self.last_sequence,
                    "terminal_summary": self.terminal_summary,
                    "provider_usage": self.provider_usage,
                    "checkpoints": self.checkpoints,
                }));
                self.lifecycle = TrainingLifecycle::Draft;
                self.phase = None;
                self.attempt_id = Some(event.attempt_id.clone());
                self.metrics.clear();
                self.checkpoints.clear();
                self.warnings.clear();
                self.latest_rollout = None;
                self.tunnel_health = None;
                self.provider_usage = None;
                self.terminal_summary = None;
            }
        } else {
            self.attempt_id = Some(event.attempt_id.clone());
        }
        self.last_sequence = event.sequence;
        self.last_event_id = Some(event.event_id.clone());
        self.phase = Some(event.phase.clone());
        self.lifecycle = reduce_lifecycle(self.lifecycle, event);
        match event.kind.as_str() {
            "metric" => {
                if let Some(metrics) = event.payload.get("metrics").and_then(Value::as_object) {
                    for (name, value) in metrics {
                        if let Some(value) = value.as_f64() {
                            self.metrics.insert(name.clone(), value);
                        }
                    }
                }
            }
            "checkpoint.ready" | "checkpoint.failed" => {
                self.checkpoints.push(event.payload.clone())
            }
            "warning" | "error.recoverable" | "cap.enforced" => {
                self.warnings.push(event.payload.clone())
            }
            "rollout.summary" | "reward.summary" => {
                self.latest_rollout = Some(event.payload.clone())
            }
            "rollout.env_unreachable" | "rollout.env_reconnected" => {
                self.tunnel_health = Some(serde_json::json!({
                    "status": if event.kind == "rollout.env_unreachable" { "unreachable" } else { "connected" },
                    "occurred_at": event.occurred_at,
                    "sequence": event.sequence,
                    "detail": event.payload,
                }));
            }
            "usage" | "usage.observed" | "usage.provider_observed" | "usage.reconciled" => {
                self.provider_usage = Some(event.payload.clone())
            }
            kind if kind.starts_with("job.") && self.lifecycle.terminal() => {
                self.terminal_summary = Some(event.payload.clone())
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn reduce_lifecycle(current: TrainingLifecycle, event: &TrainingEvent) -> TrainingLifecycle {
    if current.terminal() {
        return current;
    }
    use TrainingLifecycle as S;
    let target = match event.kind.as_str() {
        "job.accepted" | "validation.succeeded" => S::Queued,
        "validation.started" => S::Validating,
        "provisioning.started" => S::Provisioning,
        "training.started" | "rollout.env_reconnected" | "job.resumed" => S::Running,
        "rollout.env_unreachable" => S::EnvUnreachable,
        "checkpoint.writing" => S::Checkpointing,
        "evaluation.started" => S::Evaluating,
        "cancellation.requested" => S::Cancelling,
        "job.paused" => S::Paused,
        "job.cancelled" => S::Cancelled,
        "job.completed" => S::Completed,
        "job.degraded" => S::Degraded,
        "job.failed_evidence" => S::FailedEvidence,
        "job.failed" => S::Failed,
        "job.infrastructure_lost" => S::InfrastructureLost,
        "job.cap_reached" => S::CapReached,
        _ => return current,
    };
    if target.terminal() || matches!(target, S::Running | S::EnvUnreachable) {
        return target;
    }
    let order = |state| match state {
        S::Draft => 0,
        S::Validating => 1,
        S::Queued => 2,
        S::Provisioning => 3,
        S::Running => 4,
        S::EnvUnreachable => 5,
        S::Checkpointing => 6,
        S::Evaluating => 7,
        S::Cancelling => 8,
        S::Paused => 9,
        _ => 10,
    };
    if order(target) < order(current) {
        current
    } else {
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> TrainingEvent {
        serde_json::from_str(include_str!("fixtures/training_event_v1.json")).unwrap()
    }

    #[test]
    fn shared_fixture_round_trips() {
        let event = fixture();
        event.validate().unwrap();
        let actual = serde_json::to_value(event).unwrap();
        let expected: Value =
            serde_json::from_str(include_str!("fixtures/training_event_v1.json")).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn reducer_deduplicates_and_preserves_terminal_truth() {
        let mut projection = TrainingProjection::default();
        let mut event = fixture();
        event.kind = "job.completed".into();
        event.phase = "terminal".into();
        projection.apply(&event).unwrap();
        projection.apply(&event).unwrap();
        assert_eq!(projection.lifecycle, TrainingLifecycle::Completed);
        event.sequence += 1;
        event.kind = "metric".into();
        projection.apply(&event).unwrap();
        assert_eq!(projection.lifecycle, TrainingLifecycle::Completed);
    }

    #[test]
    fn reducer_rejects_sequence_collision() {
        let mut projection = TrainingProjection::default();
        let event = fixture();
        projection.apply(&event).unwrap();
        let mut collision = event;
        collision.event_id = "different-event".into();
        assert_eq!(
            projection.apply(&collision),
            Err("training event sequence collision".into())
        );
    }

    #[test]
    fn reducer_opens_new_projection_only_at_resumed_attempt_acceptance() {
        let mut projection = TrainingProjection::default();
        let mut terminal = fixture();
        terminal.kind = "job.infrastructure_lost".into();
        terminal.phase = "terminal".into();
        projection.apply(&terminal).unwrap();

        let mut resumed = terminal.clone();
        resumed.sequence += 1;
        resumed.event_id = "attempt-2:accepted".into();
        resumed.attempt_id = "attempt-2".into();
        resumed.kind = "job.accepted".into();
        resumed.phase = "queued".into();
        projection.apply(&resumed).unwrap();

        assert_eq!(projection.lifecycle, TrainingLifecycle::Queued);
        assert_eq!(projection.attempt_id.as_deref(), Some("attempt-2"));
        assert_eq!(projection.attempt_history.len(), 1);
    }
}
