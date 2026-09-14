//! SDK enrollment/grant operations against a live in-process server.
//! Grant-credential verification is covered in mq-server tests/grants_http.rs.

use mq_core::*;
use mq_sdk::MqClient;
use tokio::net::TcpListener;

#[tokio::test]
async fn sdk_enrollment_and_grant_lifecycle() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, mq_server::app()).await.unwrap() });
    tokio::task::yield_now().await;
    let base = format!("http://{addr}");
    let owner = MqClient::with_dev_principal(&base, "human", "org", "owner");
    let member = MqClient::with_dev_principal(&base, "human", "org", "member");
    let principal = |id: &str| Principal { kind: PrincipalKind::Human, id: id.into(), org_id: "org".into() };
    let thread = owner.create_thread(CreateThread {
        org_id: "org".into(), scope: ScopeBinding { kind: ScopeKind::Org, id: "org".into() }, title: None,
        participants: vec![Participant::new(principal("owner"), Role::Owner), Participant::new(principal("member"), Role::Member)],
        idempotency_key: None,
    }).await.unwrap().thread_id;
    for body in ["one", "two"] {
        owner.publish(thread, PublishMessage { body: body.into(), ..Default::default() }).await.unwrap();
    }

    let request = EnrollDevice { device_id: "dev".into(), session_id: "s1".into(), label: Some("laptop".into()) };
    let first = owner.enroll(&request).await.unwrap();
    let second = owner.enroll(&request).await.unwrap();
    assert_eq!((first.incarnation, second.incarnation), (1, 2));
    assert_eq!(owner.list_enrollments().await.unwrap().len(), 1);
    assert_eq!(owner.get_enrollment(first.enrollment_id).await.unwrap().incarnation, 2);
    assert_eq!(member.get_enrollment(first.enrollment_id).await.unwrap_err().api_code().as_deref(), Some("not_found"));

    let create = CreateGrant {
        thread_id: thread, enrollment_id: first.enrollment_id, operations: vec![GrantOperation::Read],
        ttl_seconds: 3600, history_after_seq: None,
    };
    let grant = owner.create_grant(&create).await.unwrap();
    assert_eq!((grant.history_after_seq, grant.incarnation), (2, 2));
    let duplicate = owner.create_grant(&create).await.unwrap_err();
    assert_eq!((duplicate.status().map(|s| s.as_u16()), duplicate.api_code().as_deref()), (Some(409), Some("grant_exists")));
    let filter = GrantFilter { thread_id: Some(thread), enrollment_id: None };
    assert_eq!(owner.list_grants(&filter).await.unwrap().len(), 1);
    assert!(member.list_grants(&filter).await.unwrap().is_empty());
    assert_eq!(member.get_grant(grant.grant_id).await.unwrap_err().api_code().as_deref(), Some("not_found"));

    let stale = GrantIssuanceRequest { enrollment_id: first.enrollment_id, incarnation: 1 };
    assert_eq!(owner.grant_issuance(grant.grant_id, &stale).await.unwrap_err().api_code().as_deref(), Some("grant_incarnation_fenced"));
    let current = GrantIssuanceRequest { enrollment_id: first.enrollment_id, incarnation: 2 };
    let issuance = owner.grant_issuance(grant.grant_id, &current).await.unwrap();
    assert_eq!(issuance.not_after, issuance.grant.expires_at);

    let revoked = owner.revoke_grant(grant.grant_id).await.unwrap();
    assert_eq!((revoked.state, revoked.generation), (GrantState::Revoked, 1));
    assert_eq!(owner.renew_grant(grant.grant_id, 600).await.unwrap_err().api_code().as_deref(), Some("grant_revoked"));
    assert_eq!(owner.grant_issuance(grant.grant_id, &current).await.unwrap_err().api_code().as_deref(), Some("grant_revoked"));
    let restored = owner.restore_grant(grant.grant_id).await.unwrap();
    assert_eq!((restored.state, restored.generation), (GrantState::Active, 1));
    assert_eq!(owner.renew_grant(grant.grant_id, 59).await.unwrap_err().api_code().as_deref(), Some("invalid_ttl"));
    assert!(owner.renew_grant(grant.grant_id, 600).await.unwrap().expires_at < grant.expires_at);

    let page = owner.read_history(thread, 0, 1).await.unwrap();
    assert_eq!((page.history_after_seq, page.next_after_seq, page.has_more), (0, 1, true));
    assert!(page.skipped.is_none());

    // Device sign-out.
    assert_eq!(member.revoke_enrollment(first.enrollment_id).await.unwrap_err().api_code().as_deref(), Some("not_found"));
    let signed_out = owner.revoke_enrollment(first.enrollment_id).await.unwrap();
    assert!(signed_out.revoked_at.is_some());
    assert_eq!(owner.get_grant(grant.grant_id).await.unwrap().state, GrantState::Revoked);
    assert_eq!(owner.enroll(&request).await.unwrap_err().api_code().as_deref(), Some("enrollment_revoked"));
    assert_eq!(owner.grant_issuance(grant.grant_id, &current).await.unwrap_err().api_code().as_deref(), Some("enrollment_revoked"));
}
