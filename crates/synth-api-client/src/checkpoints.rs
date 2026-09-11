//! Adapter-specific persisted checkpoints, not a claim about selected wire DTOs.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternCheckpoint {
    pub sequence: u64,
    pub generation: u64,
}

impl InternCheckpoint {
    pub fn next(&self, sequence: u64, generation: u64) -> Result<Self, &'static str> {
        if self.sequence.checked_add(1) != Some(sequence) || generation < self.generation {
            return Err("noncontiguous sequence or regressed generation");
        }
        Ok(Self {
            sequence,
            generation,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwarmCheckpoint {
    pub event_id: Option<String>,
    pub state_version: Option<String>,
    pub transcript_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqCheckpoint {
    pub subscription_id: String,
    pub durable_sequence: u64,
}

impl MqCheckpoint {
    /// A wake hint never acknowledges durable mailbox delivery.
    pub fn on_wake(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapters_preserve_distinct_cursor_semantics() {
        let intern = InternCheckpoint {
            sequence: 0,
            generation: 0,
        };
        assert_eq!(intern.next(1, 0).unwrap().generation, 0);
        assert!(intern.next(2, 0).is_err());
        let swarm = SwarmCheckpoint {
            event_id: Some("opaque:001/x".into()),
            state_version: None,
            transcript_cursor: Some("archive:page:A".into()),
        };
        assert_eq!(
            serde_json::from_value::<SwarmCheckpoint>(serde_json::to_value(&swarm).unwrap())
                .unwrap(),
            swarm
        );
        let mq = MqCheckpoint {
            subscription_id: "subscription".into(),
            durable_sequence: 2,
        };
        assert_eq!(mq.on_wake(), mq);
    }
}
