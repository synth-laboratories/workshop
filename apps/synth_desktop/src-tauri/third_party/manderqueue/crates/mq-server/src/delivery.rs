//! Worker-only signed delivery and truthful transport outcomes. See docs/DELIVERY_SECURITY.md.
use jsonwebtoken::{encode, EncodingKey, Header};
use mq_core::DeliveryStatus;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const DELIVERY_PATH: &str = "/internal/mq/v1/delivery";

pub fn delivery_token(body: &[u8], secret: &str, now: i64) -> Result<String, String> {
    if secret.as_bytes().len() < 32 {
        return Err("MQ_DELIVERY_JWT_SECRET requires at least 32 bytes".into());
    }
    let claims = json!({
        "iss": "manderqueue-worker", "aud": "synth-mq-delivery", "sub": "mq-worker",
        "iat": now, "exp": now + 60, "jti": uuid::Uuid::new_v4().to_string(),
        "method": "POST", "path": DELIVERY_PATH,
        "body_sha256": format!("{:x}", Sha256::digest(body)),
    });
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|error| format!("delivery signing failed: {error}"))
}

pub fn bridge_outcome(body: &Value) -> Option<DeliveryStatus> {
    match body.get("status").and_then(Value::as_str) {
        Some("dispatched") => Some(DeliveryStatus::Dispatched),
        Some("awaiting_pull") => Some(DeliveryStatus::AwaitingPull),
        Some("not_routable") => Some(DeliveryStatus::NotRoutable),
        _ => None,
    }
}

/// A successful response for another job or lease attempt cannot settle this job.
/// When the envelope carries a grant, the receipt must echo exactly that grant,
/// so a disposition for other (or no) grant authority cannot settle it.
pub fn matching_bridge_outcome(body: &Value, envelope: &Value) -> Option<DeliveryStatus> {
    for key in ["job_id", "message_id", "thread_id", "recipient", "attempts"] {
        let expected = envelope.get(key)?;
        if expected.is_null() || body.get(key) != Some(expected) {
            return None;
        }
    }
    if let Some(grant) = envelope.get("grant") {
        if body.get("grant") != Some(grant) {
            return None;
        }
    }
    bridge_outcome(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_http_is_not_delivery() {
        for value in [
            json!({}),
            json!({"status":"ignored"}),
            json!({"status":"acked"}),
            json!({"status":"delivered"}),
        ] {
            assert_eq!(bridge_outcome(&value), None);
        }
        assert_eq!(
            bridge_outcome(&json!({"status":"dispatched"})),
            Some(DeliveryStatus::Dispatched)
        );
        assert_eq!(
            bridge_outcome(&json!({"status":"awaiting_pull"})),
            Some(DeliveryStatus::AwaitingPull)
        );
        assert_eq!(
            bridge_outcome(&json!({"status":"not_routable"})),
            Some(DeliveryStatus::NotRoutable)
        );
    }
    #[test]
    fn signing_requires_dedicated_strong_secret() {
        assert!(delivery_token(b"{}", "short", 1).is_err());
    }

    #[test]
    fn receipt_must_match_every_delivery_identity_field() {
        let envelope = json!({"job_id":"job", "message_id":"message", "thread_id":"thread",
            "recipient":{"kind":"actor","id":"actor","org_id":"org"}, "attempts":2});
        let mut receipt = envelope.clone();
        receipt["status"] = json!("dispatched");
        assert_eq!(matching_bridge_outcome(&receipt, &envelope), Some(DeliveryStatus::Dispatched));
        for key in ["job_id", "message_id", "thread_id", "recipient", "attempts"] {
            let mut wrong = receipt.clone();
            wrong[key] = json!("wrong");
            assert_eq!(matching_bridge_outcome(&wrong, &envelope), None);
            wrong.as_object_mut().unwrap().remove(key);
            assert_eq!(matching_bridge_outcome(&wrong, &envelope), None);
        }
        assert_eq!(matching_bridge_outcome(&json!({"status":"dispatched"}), &envelope), None);
    }

    #[test]
    fn receipt_must_echo_the_exact_envelope_grant() {
        let mut envelope = json!({"job_id":"job", "message_id":"message", "thread_id":"thread",
            "recipient":{"kind":"actor","id":"enrollment:e","org_id":"org"}, "attempts":1});
        envelope["grant"] = json!({"grant_id":"g","generation":1,"incarnation":2});
        let mut receipt = envelope.clone();
        receipt["status"] = json!("awaiting_pull");
        assert_eq!(matching_bridge_outcome(&receipt, &envelope), Some(DeliveryStatus::AwaitingPull));
        for stale in [json!({"grant_id":"g","generation":0,"incarnation":2}), json!({"grant_id":"g","generation":1,"incarnation":1}), Value::Null] {
            let mut wrong = receipt.clone();
            wrong["grant"] = stale;
            assert_eq!(matching_bridge_outcome(&wrong, &envelope), None);
        }
        receipt.as_object_mut().unwrap().remove("grant");
        assert_eq!(matching_bridge_outcome(&receipt, &envelope), None);
        // Envelopes without a grant keep the previous identity-only binding.
        envelope.as_object_mut().unwrap().remove("grant");
        assert_eq!(matching_bridge_outcome(&receipt, &envelope), Some(DeliveryStatus::AwaitingPull));
    }
}
