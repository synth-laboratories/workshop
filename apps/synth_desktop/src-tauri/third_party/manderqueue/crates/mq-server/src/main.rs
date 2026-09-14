use std::env;
use std::time::Duration;

use mq_server::worker::{http_client, run_once, WorkerConfig};
use mq_server::{boot_from_env, router, AppState};

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
    let auth = boot.auth.label();
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
    let secret =
        env::var("MQ_DELIVERY_JWT_SECRET").expect("worker requires MQ_DELIVERY_JWT_SECRET");
    let max_attempts: u32 = env::var("MQ_WORKER_MAX_ATTEMPTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let config = WorkerConfig::new(&bridge, &secret, max_attempts).expect("delivery configuration");
    let boot = boot_from_env().await.expect("boot");
    let fabric = boot.fabric;
    let local = boot.local_wake;
    let interval_ms: u64 = env::var("MQ_WORKER_POLL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let http = http_client();

    eprintln!(
        "mq-server worker started poll_ms={interval_ms} bridge={}",
        config.bridge_origin()
    );

    let mut wake_rx = local.subscribe();
    loop {
        let _ = tokio::time::timeout(Duration::from_millis(interval_ms), wake_rx.recv()).await;
        // Each claimed job is rechecked against live grant authority before dispatch.
        if let Err(e) = run_once(&fabric, &http, &config, 1).await {
            eprintln!("claim failed: {e}");
        }
    }
}
