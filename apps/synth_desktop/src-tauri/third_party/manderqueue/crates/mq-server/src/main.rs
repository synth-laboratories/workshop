use std::env;
use std::time::Duration;

use mq_core::DeliveryStatus;
use mq_server::delivery::{matching_bridge_outcome, delivery_token, DELIVERY_PATH};
use mq_server::{boot_from_env, router, AppState};
use serde_json::json;

#[tokio::main]
async fn main() {
    let mut args = env::args().skip(1);
    // Prefer MQ_CMD so one image can run serve + worker on Railway.
    let cmd = env::var("MQ_CMD")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| args.next())
        .unwrap_or_else(|| "serve".into());
    match cmd.as_str() {
        "embedded" => mq_server::embedded::serve().await.expect("embedded boot"),
        "worker" => run_worker().await,
        "serve" => run_serve().await,
        other => {
            eprintln!("unknown command {other:?}; use: mq-server [serve|worker] (or MQ_CMD)");
            std::process::exit(2);
        }
    }
}

async fn run_serve() {
    let boot = boot_from_env().await.expect("boot");
    let backend = if boot.database_url.is_some() {
        "postgres"
    } else {
        "memory"
    };
    let redis = if env::var("REDIS_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        "on"
    } else {
        "off"
    };
    let auth = match &boot.auth {
        mq_server::AuthMode::Dev => "dev",
        mq_server::AuthMode::Jwt { .. } => "jwt",
    };
    let bind = env::var("MQ_BIND").unwrap_or_else(|_| {
        env::var("PORT")
            .map(|p| format!("0.0.0.0:{p}"))
            .unwrap_or_else(|_| "0.0.0.0:8088".into())
    });
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    eprintln!(
        "mq-server listening on {} store={backend} redis={redis} write_buffer={} auth={auth}",
        listener.local_addr().unwrap(),
        boot.write_buffer
    );
    axum::serve(
        listener,
        router(AppState::from_parts(
            boot.fabric,
            boot.local_wake,
            boot.database_url,
            boot.auth,
        )),
    )
    .await
    .expect("serve");
}

async fn run_worker() {
    // Refuse missing delivery wiring before opening stores or claiming work.
    let bridge = env::var("MQ_BRIDGE_BASE_URL").expect("worker requires MQ_BRIDGE_BASE_URL");
    let bridge_url = reqwest::Url::parse(&bridge).expect("valid bridge URL");
    assert!(
        matches!(bridge_url.scheme(), "http" | "https"),
        "HTTP(S) bridge required"
    );
    assert!(
        bridge_url.query().is_none() && bridge_url.fragment().is_none() && bridge_url.path() == "/",
        "bridge URL must be an origin"
    );
    assert!(
        bridge_url.username().is_empty() && bridge_url.password().is_none(),
        "bridge URL must not contain credentials"
    );
    let secret =
        env::var("MQ_DELIVERY_JWT_SECRET").expect("worker requires MQ_DELIVERY_JWT_SECRET");
    delivery_token(b"{}", &secret, chrono::Utc::now().timestamp())
        .expect("delivery signing configuration");
    let boot = boot_from_env().await.expect("boot");
    let fabric = boot.fabric;
    let local = boot.local_wake;
    let max_attempts: u32 = env::var("MQ_WORKER_MAX_ATTEMPTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let interval_ms: u64 = env::var("MQ_WORKER_POLL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("delivery HTTP client");

    eprintln!(
        "mq-server worker started poll_ms={interval_ms} bridge={}",
        bridge_url.origin().ascii_serialization()
    );

    let mut wake_rx = local.subscribe();
    loop {
        let _ = tokio::time::timeout(Duration::from_millis(interval_ms), wake_rx.recv()).await;

        match fabric.claim_delivery_jobs(1).await {
            Ok(jobs) if jobs.is_empty() => {}
            Ok(jobs) => {
                for job in jobs {
                    let outcome = {
                        let url = format!("{}{}", bridge.trim_end_matches('/'), DELIVERY_PATH);
                        let message = match fabric.get_message(job.message_id).await {
                            Ok(Some(m)) => Some(m),
                            Ok(None) => {
                                eprintln!(
                                    "bridge missing message {} for job {}",
                                    job.message_id.0, job.job_id.0
                                );
                                None
                            }
                            Err(e) => {
                                eprintln!("bridge load message failed: {e}");
                                None
                            }
                        };
                        let Some(message) = message else {
                            // Retry later; do not settle delivered.
                            let _ = fabric
                                .settle_delivery_job(
                                    job.job_id,
                                    job.attempts,
                                    DeliveryStatus::Pending,
                                )
                                .await;
                            continue;
                        };
                        let body = json!({
                            "job_id": job.job_id.0,
                            "message_id": job.message_id.0,
                            "thread_id": job.thread_id.0,
                            "recipient": job.recipient,
                            "attempts": job.attempts,
                            "message": {
                                "seq": message.seq,
                                "kind": message.kind,
                                "body": message.body,
                                "payload": message.payload,
                                "sender": message.sender,
                                "idempotency_key": message.idempotency_key,
                                "correlation_id": message.correlation_id,
                                "parent_message_id": message.parent_message_id.map(|m| m.0),
                                "causation_id": message.causation_id,
                                "created_at": message.created_at,
                            }
                        });
                        let bytes = serde_json::to_vec(&body).expect("JSON envelope");
                        let token = delivery_token(&bytes, &secret, chrono::Utc::now().timestamp())
                            .expect("delivery signature");
                        match http
                            .post(&url)
                            .bearer_auth(token)
                            .header("content-type", "application/json")
                            .body(bytes)
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                match resp.json::<serde_json::Value>().await {
                                    Ok(receipt) => matching_bridge_outcome(&receipt, &body),
                                    Err(error) => {
                                        eprintln!("invalid bridge receipt: {error}");
                                        None
                                    }
                                }
                            }
                            Ok(resp) => {
                                eprintln!("bridge HTTP {} for job {}", resp.status(), job.job_id.0);
                                None
                            }
                            Err(e) => {
                                eprintln!("bridge error for job {}: {e}", job.job_id.0);
                                None
                            }
                        }
                    };

                    let status = if let Some(status) = outcome {
                        status
                    } else if job.attempts >= max_attempts {
                        DeliveryStatus::DeadLetter
                    } else {
                        DeliveryStatus::Pending
                    };
                    if let Err(e) = fabric
                        .settle_delivery_job(job.job_id, job.attempts, status)
                        .await
                    {
                        eprintln!("settle failed: {e}");
                    }
                }
            }
            Err(e) => eprintln!("claim failed: {e}"),
        }
    }
}
