//! SDK against a live in-process server.

use mq_core::*;
use mq_sdk::MqClient;
use tokio::net::TcpListener;

#[tokio::test]
async fn sdk_effort_judgment_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mq_server::app()).await.unwrap();
    });
    // tiny yield for accept
    tokio::task::yield_now().await;

    let base = format!("http://{addr}");
    let async_c = MqClient::with_dev_principal(&base, "intern_async", "org-1", "intern-a");
    let human_c = MqClient::with_dev_principal(&base, "human", "org-1", "user-1");

    async_c.health().await.unwrap();

    let thread = async_c
        .create_thread(CreateThread {
            org_id: "org-1".into(),
            scope: ScopeBinding {
                kind: ScopeKind::Effort,
                id: "e-sdk".into(),
            },
            title: Some("sdk".into()),
            participants: vec![
                Participant::new(
                    Principal {
                        kind: PrincipalKind::InternAsync,
                        id: "intern-a".into(),
                        org_id: "org-1".into(),
                    },
                    Role::Owner,
                ),
                Participant::new(
                    Principal {
                        kind: PrincipalKind::Human,
                        id: "user-1".into(),
                        org_id: "org-1".into(),
                    },
                    Role::Member,
                ),
            ],
            idempotency_key: None,
        })
        .await
        .unwrap();

    let ask = async_c
        .publish(
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Ask,
                body: "sdk metric?".into(),
                idempotency_key: Some("sdk-ask".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    human_c
        .publish(
            thread.thread_id,
            PublishMessage {
                kind: MessageKind::Answer,
                body: "ok".into(),
                correlation_id: Some(ask.message_id.0.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let page = async_c
        .read_messages(thread.thread_id, 0, 10)
        .await
        .unwrap();
    assert_eq!(page.len(), 2);

    let mut recovery = mq_sdk::CatchUpSupervisor::new(thread.thread_id, 0);
    let failed = recovery
        .catch_up(&async_c, 1, |_, _| async {
            Err(mq_sdk::SdkError::Decode("inbox transaction failed".into()))
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(recovery.cursor(), 0);
    let accepted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let storage = accepted.clone();
    let outcome = recovery
        .catch_up(&async_c, 1, move |messages, next| {
            let storage = storage.clone();
            async move {
                assert_eq!(next, messages.last().unwrap().seq);
                storage.lock().unwrap().extend(messages);
                Ok(())
            }
        })
        .await
        .unwrap();
    assert_eq!(outcome, mq_sdk::CatchUpOutcome::PageBudgetReached);
    assert_eq!(accepted.lock().unwrap().len(), 2);
    assert_eq!(recovery.cursor(), page.last().unwrap().seq);
    let mut restored = mq_sdk::CatchUpSupervisor::new(thread.thread_id, recovery.cursor());
    assert_eq!(
        restored
            .catch_up(&async_c, 1, |_, _| async {
                panic!("already accepted messages must not replay")
            })
            .await
            .unwrap(),
        mq_sdk::CatchUpOutcome::CaughtUp
    );

    // Invalid pages cannot move the inbox cursor or reach persistence.
    for corruption in ["thread", "gap", "duplicate_id", "reversed", "oversized"] {
        let mut foreign_page = page.clone();
        match corruption {
            "thread" => foreign_page[0].thread_id = ThreadId::new(),
            "gap" => foreign_page[1].seq += 1,
            "duplicate_id" => foreign_page[1].message_id = foreign_page[0].message_id,
            "reversed" => foreign_page.reverse(),
            "oversized" => foreign_page = vec![page[0].clone(); 201],
            _ => unreachable!(),
        }
        let fixture = axum::Router::new().route(
            "/v1/threads/{id}/messages",
            axum::routing::get(move || {
                let page = foreign_page.clone();
                async move { axum::Json(page) }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, fixture).await.unwrap();
        });
        let fixture_client = MqClient::new(format!("http://{addr}"), "fixture");
        let mut invalid = mq_sdk::CatchUpSupervisor::new(thread.thread_id, 0);
        assert!(invalid
            .catch_up(&fixture_client, 1, |_, _| async {
                panic!("invalid page must not reach storage")
            })
            .await
            .is_err());
        assert_eq!(invalid.cursor(), 0);
        server.abort();
    }
}
