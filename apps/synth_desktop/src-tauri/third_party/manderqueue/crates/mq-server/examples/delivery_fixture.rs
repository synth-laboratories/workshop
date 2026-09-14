//! Emit a model-free interop vector using a public fixture key, never environment secrets.
fn main() {
    let body = br#"{"job_id":"fixture-job","message_id":"fixture-message","thread_id":"fixture-thread","recipient":{"kind":"actor","id":"fixture-actor","org_id":"fixture-org"}}"#;
    let token = mq_server::delivery::delivery_token(
        body, "fixture-only-worker-key-32-bytes-minimum", chrono::Utc::now().timestamp(),
    ).expect("fixture token");
    println!("{}", serde_json::json!({"body":String::from_utf8_lossy(body), "token":token}));
}
