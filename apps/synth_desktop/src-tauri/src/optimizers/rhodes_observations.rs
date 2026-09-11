//! Critical source facts retained independently of lifecycle verdicts.
use serde_json::Value;

/// The caller commits these amendments with the source cursor, including after
/// terminal scientific status. Absent events never clear an earlier refusal.
pub(crate) fn critical_observations(events: &[Value]) -> serde_json::Map<String, Value> {
    let mut observations = serde_json::Map::new();
    for event in events {
        let field = match event.get("event_type").and_then(Value::as_str) {
            Some("rollout.phase") => "lastExecutionPhase",
            Some("rollout.resource_intent") => "lastResourceIntent",
            Some("rollout.native_journal") => "lastNativeJournal",
            Some("rollout.limit_refused") => "lastLimitRefusal",
            Some("rollout.inference_budget_reserved") => "lastBudgetReservation",
            Some("rollout.inference_accounting") => "lastInferenceAccounting",
            _ => continue,
        };
        observations.insert(field.into(), event.clone());
    }
    observations
}

#[cfg(test)]
mod tests {
    #[test]
    fn critical_observations_preserve_independent_facts() {
        let events = vec![
            serde_json::json!({"event_type":"rollout.limit_refused","sequence":1,"error_code":"spend_limit"}),
            serde_json::json!({"event_type":"rollout.inference_accounting","sequence":2}),
            serde_json::json!({"event_type":"rollout.inference_accounting","sequence":3}),
            serde_json::json!({"event_type":"rollout.phase","sequence":4,"phase":"agent","state":"started"}),
            serde_json::json!({"event_type":"rollout.phase","sequence":5,"phase":"agent","state":"exited","exit_code":7}),
            serde_json::json!({"event_type":"rollout.completed","sequence":6}),
            serde_json::json!({"event_type":"rollout.resource_intent","sequence":7,"provider":"docker","owner":"owned"}),
        ];
        let projection = super::critical_observations(&events);
        assert_eq!(projection.len(), 4);
        assert_eq!(projection["lastResourceIntent"]["owner"], "owned");
        assert_eq!(projection["lastExecutionPhase"]["sequence"], 5);
        assert_eq!(projection["lastExecutionPhase"]["exit_code"], 7);
        assert!(projection["lastExecutionPhase"].get("score").is_none());
        assert_eq!(projection["lastLimitRefusal"]["sequence"], 1);
        assert_eq!(projection["lastInferenceAccounting"]["sequence"], 3);
        assert!(super::critical_observations(&[]).is_empty());
    }
}
