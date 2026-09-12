//! Model-free mailbox journeys against an in-process fake of the backend
//! grant endpoints and the MQ grant-credential routes (contract §3, §6, §8).
//! No provider or model is ever called; the executor fixtures count calls.
use super::*;
use crate::cloud::mailbox::grant::{HttpGrantAuthority, SecretToken};
use crate::cloud::mailbox::policy::{HandlerLimits, ToolRequest};
use crate::cloud::storage::{CloudStore, OutboundDisposition, OutboundDraft, PeerRef};
use crate::core_runtime::CoreRuntime;
use crate::domain::{RuntimeTarget, SessionCreate, SessionKind, SessionStatus};
use crate::storage::EventSource;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const API_KEY: &str = "synth-api-key-canary";
const ORIGIN: &str = "https://fixture.invalid";
const LOOPBACK: EndpointPolicy = EndpointPolicy { allow_loopback_http: true };

fn uuid_of(n: u128) -> String {
    uuid::Uuid::from_u128(n).to_string()
}
fn org() -> String {
    uuid_of(3)
}
fn peer() -> PeerRef {
    PeerRef { kind: "actor".into(), id: "cloud-evaluator".into(), org_id: org() }
}

fn observation(account: u128) -> IdentityObservation {
    let now = Utc::now();
    serde_json::from_value(json!({"schema_version":"synth.desktop-cloud-identity.v1","backend_origin":ORIGIN,"backend_id":uuid_of(1),"account_id":uuid_of(account),"org_id":org(),"profile_id":uuid_of(4),"verified_at":now.to_rfc3339(),"valid_until":(now+chrono::Duration::seconds(55)).to_rfc3339(),"credential_expiry":null,"revalidate_before_remote_operation":true,"revocation_contract":"fresh_database_key_and_membership_check"})).unwrap()
}

struct FixtureVerifier(Arc<AtomicU64>);
impl IdentityVerifier for FixtureVerifier {
    fn verify(&self) -> BoxFuture<'_, Result<IdentityObservation>> {
        let account = u128::from(self.0.load(Ordering::SeqCst));
        Box::pin(async move { Ok(observation(account)) })
    }
}

struct Idle(Arc<AtomicBool>);
impl TurnBoundary for Idle {
    fn session_idle(&self, _session_id: String) -> BoxFuture<'_, Result<bool>> {
        let idle = self.0.load(Ordering::SeqCst);
        Box::pin(async move { Ok(idle) })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PublishMode {
    Normal,
    DropBeforeCommit,
    CommitThenDrop,
}

struct FakeEnrollment {
    id: String,
    device: String,
    session: String,
    incarnation: u64,
}
struct FakeGrant {
    id: String,
    thread: String,
    enrollment: String,
    ops: Vec<String>,
    floor: u64,
    expires: DateTime<Utc>,
    generation: u64,
    revoked: bool,
}

#[derive(Default)]
struct FakeState {
    endpoint: String,
    enrollments: Vec<FakeEnrollment>,
    grants: Vec<FakeGrant>,
    tokens: HashMap<String, (String, u64, u64)>,
    next_token: u64,
    messages: HashMap<String, Vec<Value>>,
    publish_posts: usize,
    publish_modes: VecDeque<PublishMode>,
    hang_history: bool,
    history_requests: usize,
    wakes: Vec<tokio::sync::mpsc::UnboundedSender<&'static str>>,
    enroll_account_override: Option<String>,
    mq_saw_api_key: bool,
    backend_saw_mq_token: bool,
}

enum Reply {
    Json(u16, Value),
    Hang,
    Drop,
    Sse(tokio::sync::mpsc::UnboundedReceiver<&'static str>),
}

fn problem(status: u16, code: &str) -> Reply {
    Reply::Json(status, json!({"detail": {"code": code}}))
}
fn mq_problem(status: u16, code: &str) -> Reply {
    Reply::Json(status, json!({"error": code}))
}

#[derive(Clone)]
struct Fake {
    state: Arc<std::sync::Mutex<FakeState>>,
    origin: String,
}

impl Fake {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let fake = Self { state: Arc::new(std::sync::Mutex::new(FakeState { endpoint: origin.clone(), ..Default::default() })), origin };
        let server = fake.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(handle(socket, server.clone()));
            }
        });
        fake
    }

    fn with<T>(&self, f: impl FnOnce(&mut FakeState) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }

    fn messages(&self, thread: &str) -> Vec<Value> {
        self.with(|state| state.messages.get(thread).cloned().unwrap_or_default())
    }

    fn replies(&self, thread: &str) -> Vec<Value> {
        self.messages(thread).into_iter().filter(|m| m["sender"]["id"].as_str().is_some_and(|id| id.starts_with("enrollment:"))).collect()
    }

    fn inject(&self, thread: &str, kind: &str, body: &str, payload: Value, correlation: Option<&str>) -> String {
        self.with(|state| {
            let messages = state.messages.entry(thread.to_owned()).or_default();
            let id = uuid::Uuid::new_v4().to_string();
            messages.push(json!({"message_id":id,"thread_id":thread,"seq":messages.len()+1,"kind":kind,"body":body,"payload":payload,
                "sender":{"kind":"actor","id":"cloud-evaluator","org_id":org()},"idempotency_key":null,"correlation_id":correlation,
                "parent_message_id":null,"causation_id":null,"created_at":Utc::now()}));
            id
        })
    }

    fn wake(&self) {
        self.with(|state| state.wakes.retain(|sender| sender.send("thread_wake").is_ok()));
    }

    fn revoke_all(&self) {
        self.with(|state| {
            for grant in &mut state.grants {
                if !grant.revoked {
                    grant.revoked = true;
                    grant.generation += 1;
                }
            }
        });
    }

    fn restore_all(&self) {
        self.with(|state| state.grants.iter_mut().for_each(|grant| grant.revoked = false));
    }

    fn enrollment_json(state: &FakeState, enrollment: &FakeEnrollment) -> Value {
        let account = state.enroll_account_override.clone().unwrap_or_else(|| uuid_of(2));
        json!({"enrollment_id":enrollment.id,"org_id":org(),"owner":{"kind":"human","id":account,"org_id":org()},
            "device_id":enrollment.device,"session_id":enrollment.session,"label":null,
            "principal":{"kind":"actor","id":format!("enrollment:{}",enrollment.id),"org_id":org()},
            "incarnation":enrollment.incarnation,"created_at":"x","updated_at":"x"})
    }

    fn grant_json(state: &FakeState, grant: &FakeGrant) -> Value {
        let incarnation = state.enrollments.iter().find(|e| e.id == grant.enrollment).map(|e| e.incarnation).unwrap_or(0);
        let lifecycle = if grant.revoked { "revoked" } else if grant.expires <= Utc::now() { "expired" } else { "active" };
        json!({"grant_id":grant.id,"org_id":org(),"thread_id":grant.thread,"enrollment_id":grant.enrollment,
            "principal":{"kind":"actor","id":format!("enrollment:{}",grant.enrollment),"org_id":org()},
            "operations":grant.ops,"history_after_seq":grant.floor,"expires_at":grant.expires,"incarnation":incarnation,
            "generation":grant.generation,"status":if grant.revoked {"revoked"} else {"active"},"state":lifecycle,
            "granted_by":{"kind":"human","id":uuid_of(2),"org_id":org()},"created_at":"x","updated_at":"x"})
    }

    fn authorize(state: &FakeState, auth: &str, thread: &str, op: &str) -> std::result::Result<(String, u64), Reply> {
        let token = auth.strip_prefix("Bearer ").unwrap_or_default();
        let Some((grant_id, generation, incarnation)) = state.tokens.get(token).cloned() else { return Err(mq_problem(401, "unauthenticated")) };
        let grant = state.grants.iter().find(|g| g.id == grant_id).unwrap();
        let enrollment = state.enrollments.iter().find(|e| e.id == grant.enrollment).unwrap();
        if grant.thread != thread {
            return Err(mq_problem(401, "unauthenticated"));
        }
        if grant.revoked {
            return Err(mq_problem(403, "grant_revoked"));
        }
        if generation != grant.generation {
            return Err(mq_problem(403, "grant_generation_stale"));
        }
        if incarnation != enrollment.incarnation {
            return Err(mq_problem(403, "grant_incarnation_fenced"));
        }
        if grant.expires <= Utc::now() {
            return Err(mq_problem(403, "grant_expired"));
        }
        if !grant.ops.iter().any(|o| o == op) {
            return Err(mq_problem(403, "grant_operation_denied"));
        }
        Ok((format!("enrollment:{}", enrollment.id), grant.floor))
    }

    fn route(&self, method: &str, target: &str, auth: &str, body: Value) -> Reply {
        let mut state = self.state.lock().unwrap();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let query: HashMap<&str, &str> = query.split('&').filter_map(|pair| pair.split_once('=')).collect();
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if path.starts_with("/api/v1/mq/") {
            if auth.contains("tok-") {
                state.backend_saw_mq_token = true;
            }
            if auth != format!("Bearer {API_KEY}") {
                return problem(401, "desktop_cloud_identity_revoked_or_unavailable");
            }
            return match (method, &segments[3..]) {
                ("POST", ["enrollments"]) => {
                    let (device, session) = (body["device_id"].as_str().unwrap().to_owned(), body["session_id"].as_str().unwrap().to_owned());
                    let index = match state.enrollments.iter().position(|e| e.device == device && e.session == session) {
                        Some(index) => {
                            state.enrollments[index].incarnation += 1;
                            index
                        }
                        None => {
                            state.enrollments.push(FakeEnrollment { id: uuid::Uuid::new_v4().to_string(), device, session, incarnation: 1 });
                            state.enrollments.len() - 1
                        }
                    };
                    let account = state.enroll_account_override.clone().unwrap_or_else(|| uuid_of(2));
                    let enrollment = Self::enrollment_json(&state, &state.enrollments[index]);
                    Reply::Json(201, json!({"enrollment":enrollment,"mq_endpoint":state.endpoint,
                        "identity":{"backend_origin":ORIGIN,"backend_id":uuid_of(1),"profile_id":uuid_of(4),"account_id":account,"org_id":org()}}))
                }
                ("POST", ["grants"]) => {
                    let thread = body["thread_id"].as_str().unwrap().to_owned();
                    let enrollment = body["enrollment_id"].as_str().unwrap().to_owned();
                    if state.grants.iter().any(|g| g.thread == thread && g.enrollment == enrollment) {
                        return problem(409, "grant_exists");
                    }
                    let head = state.messages.get(&thread).map_or(0, Vec::len) as u64;
                    let grant = FakeGrant {
                        id: uuid::Uuid::new_v4().to_string(),
                        thread,
                        enrollment,
                        ops: body["operations"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect(),
                        floor: body["history_after_seq"].as_u64().unwrap_or(head),
                        expires: Utc::now() + chrono::Duration::seconds(body["ttl_seconds"].as_i64().unwrap()),
                        generation: 0,
                        revoked: false,
                    };
                    let doc = Self::grant_json(&state, &grant);
                    state.grants.push(grant);
                    Reply::Json(201, json!({"grant": doc}))
                }
                ("GET", ["grants"]) => {
                    let grants: Vec<Value> = state.grants.iter()
                        .filter(|g| Some(g.enrollment.as_str()) == query.get("enrollment_id").copied() && Some(g.thread.as_str()) == query.get("thread_id").copied())
                        .map(|g| Self::grant_json(&state, g)).collect();
                    Reply::Json(200, json!({"grants": grants}))
                }
                ("GET", ["grants", id]) => match state.grants.iter().find(|g| g.id == *id) {
                    Some(grant) => Reply::Json(200, json!({"grant": Self::grant_json(&state, grant)})),
                    None => problem(404, "not_found"),
                },
                ("POST", ["grants", id, "revoke"]) => {
                    let Some(index) = state.grants.iter().position(|g| g.id == *id) else { return problem(404, "not_found") };
                    if !state.grants[index].revoked {
                        state.grants[index].revoked = true;
                        state.grants[index].generation += 1;
                    }
                    Reply::Json(200, json!({"grant": Self::grant_json(&state, &state.grants[index])}))
                }
                ("POST", ["grants", id, "credential"]) => {
                    let Some(index) = state.grants.iter().position(|g| g.id == *id) else { return problem(404, "not_found") };
                    let current = state.enrollments.iter().find(|e| e.id == state.grants[index].enrollment).unwrap().incarnation;
                    if body["incarnation"].as_u64() != Some(current) {
                        return problem(403, "grant_incarnation_fenced");
                    }
                    if state.grants[index].revoked {
                        return problem(403, "grant_revoked");
                    }
                    if state.grants[index].expires <= Utc::now() {
                        return problem(403, "grant_expired");
                    }
                    state.next_token += 1;
                    let token = format!("tok-{}", state.next_token);
                    let generation = state.grants[index].generation;
                    state.tokens.insert(token.clone(), (id.to_string(), generation, current));
                    Reply::Json(200, json!({"mq_endpoint":state.endpoint,"token":token,"token_type":"Bearer",
                        "expires_at":Utc::now()+chrono::Duration::seconds(300),"kid":"fixture-kid","grant":Self::grant_json(&state, &state.grants[index])}))
                }
                _ => problem(404, "not_found"),
            };
        }
        if auth.contains(API_KEY) {
            state.mq_saw_api_key = true;
        }
        match (method, segments.as_slice()) {
            ("GET", ["v1", "threads", thread, "history"]) => {
                state.history_requests += 1;
                if state.hang_history {
                    return Reply::Hang;
                }
                let (_, floor) = match Self::authorize(&state, auth, thread, "read") { Ok(value) => value, Err(reply) => return reply };
                let after: u64 = query.get("after_seq").and_then(|v| v.parse().ok()).unwrap_or(0);
                let limit: usize = query.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50);
                let effective = after.max(floor);
                let all = state.messages.get(*thread).cloned().unwrap_or_default();
                let page: Vec<Value> = all.iter().filter(|m| m["seq"].as_u64().unwrap() > effective).take(limit).cloned().collect();
                let next = page.last().map_or(effective, |m| m["seq"].as_u64().unwrap());
                let skipped = (after < floor).then(|| json!({"after_seq":after,"through_seq":floor,"reason":"before_grant_history"}));
                Reply::Json(200, json!({"thread_id":thread,"requested_after_seq":after,"history_after_seq":floor,"effective_after_seq":effective,
                    "skipped":skipped,"messages":page,"next_after_seq":next,"has_more":all.iter().any(|m| m["seq"].as_u64().unwrap() > next)}))
            }
            ("POST", ["v1", "threads", thread, "messages"]) => {
                let (principal, _) = match Self::authorize(&state, auth, thread, "publish") { Ok(value) => value, Err(reply) => return reply };
                state.publish_posts += 1;
                let mode = state.publish_modes.pop_front().unwrap_or(PublishMode::Normal);
                let key = body["idempotency_key"].clone();
                let messages = state.messages.entry(thread.to_string()).or_default();
                if let Some(existing) = messages.iter().find(|m| m["sender"]["id"] == json!(principal) && m["idempotency_key"] == key) {
                    return Reply::Json(200, existing.clone());
                }
                if mode == PublishMode::DropBeforeCommit {
                    return Reply::Drop;
                }
                let message = json!({"message_id":uuid::Uuid::new_v4(),"thread_id":thread,"seq":messages.len()+1,"kind":body["kind"],"body":body["body"],
                    "payload":body["payload"],"sender":{"kind":"actor","id":principal,"org_id":org()},"idempotency_key":key,
                    "correlation_id":body["correlation_id"],"parent_message_id":body.get("parent_message_id").cloned().unwrap_or(Value::Null),
                    "causation_id":body.get("causation_id").cloned().unwrap_or(Value::Null),"created_at":Utc::now()});
                messages.push(message.clone());
                if mode == PublishMode::CommitThenDrop {
                    return Reply::Drop;
                }
                Reply::Json(201, message)
            }
            ("GET", ["v1", "threads", thread, "events"]) => {
                if let Err(reply) = Self::authorize(&state, auth, thread, "read") {
                    return reply;
                }
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                state.wakes.push(sender);
                Reply::Sse(receiver)
            }
            _ => mq_problem(401, "unauthenticated"),
        }
    }
}

async fn handle(mut socket: TcpStream, fake: Fake) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let count = match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(count) => count,
        };
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut request = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (request.next().unwrap_or_default().to_owned(), request.next().unwrap_or_default().to_owned());
    let (mut length, mut auth) = (0usize, String::new());
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => auth = value.trim().to_owned(),
                _ => {}
            }
        }
    }
    while buffer.len() < header_end + length {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    }
    let body = serde_json::from_slice(&buffer[header_end..header_end + length]).unwrap_or(Value::Null);
    let reply = fake.route(&method, &target, &auth, body);
    match reply {
        Reply::Json(status, value) => {
            let text = value.to_string();
            let wire = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
            let _ = socket.write_all(wire.as_bytes()).await;
        }
        Reply::Hang => std::future::pending::<()>().await,
        Reply::Drop => {
            let _ = socket.shutdown().await;
        }
        Reply::Sse(mut events) => {
            if socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.is_err() {
                return;
            }
            while let Some(event) = events.recv().await {
                let data = format!("event: {event}\ndata: wake\n\n");
                if socket.write_all(format!("{:x}\r\n{data}\r\n", data.len()).as_bytes()).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Probe {
    Answer,
    Overreach,
    Sleep,
    Cost(u64),
}

struct ProbeExecutor {
    calls: AtomicUsize,
    probe: Probe,
    inside: std::path::PathBuf,
    outside: std::path::PathBuf,
}

impl RestrictedExecutor for ProbeExecutor {
    fn run(&self, turn: RestrictedTurn, gate: Arc<ToolGate>) -> BoxFuture<'_, Result<RestrictedOutcome>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match self.probe {
                Probe::Answer => Ok(RestrictedOutcome { answer: format!("answer to {}", turn.message_id), cost_usd_micros: 0 }),
                Probe::Overreach => {
                    let attempts = [
                        ToolRequest::Deploy,
                        ToolRequest::ExpandAccess,
                        ToolRequest::InvitePeer,
                        ToolRequest::SpawnWork,
                        ToolRequest::WriteFile { path: self.inside.clone() },
                        ToolRequest::ReadFile { path: self.outside.clone() },
                        ToolRequest::Tool { name: "shell".into() },
                        ToolRequest::Spend { usd_micros: 1 },
                    ];
                    let refused = attempts.iter().filter(|attempt| gate.authorize(attempt).is_err()).count();
                    let allowed = gate.read_allowed_file(&self.inside, 64)?;
                    Ok(RestrictedOutcome { answer: format!("refused={refused} read={allowed}"), cost_usd_micros: 0 })
                }
                Probe::Sleep => {
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    Ok(RestrictedOutcome { answer: "late".into(), cost_usd_micros: 0 })
                }
                Probe::Cost(cost) => Ok(RestrictedOutcome { answer: "costly".into(), cost_usd_micros: cost }),
            }
        })
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    core: CoreRuntime,
    runtime: ScopedCloudRuntime,
    fake: Fake,
    deps: MailboxDeps,
    account: Arc<AtomicU64>,
    idle: Arc<AtomicBool>,
    session: String,
    thread: String,
}

async fn local_session(core: &CoreRuntime, id: &str, remote_id: Option<&str>) -> String {
    core.sessions()
        .create_or_update(SessionCreate {
            id: id.into(),
            title: "Local agent".into(),
            kind: SessionKind::Codex,
            target: RuntimeTarget::local_laguna(),
            project_id: None,
            remote_id: remote_id.map(str::to_owned),
            codex_thread_id: None,
            status: SessionStatus::Ready,
            state_generation: None,
            metadata: json!({}),
            source: EventSource::Codex,
        })
        .await
        .unwrap();
    id.into()
}

fn connect_request(thread: &str, session: &str, preset: Preset, policy: ParticipantPolicy) -> ConnectRequest {
    ConnectRequest {
        thread_id: thread.into(),
        local_session_id: session.into(),
        peers: vec![peer()],
        preset,
        policy,
        grant_ttl_seconds: 3600,
        history_after_seq: None,
        label: Some("fixture".into()),
    }
}

async fn harness(preset: Preset, policy: ParticipantPolicy, executor: Option<Arc<dyn RestrictedExecutor>>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let core = CoreRuntime::open(dir.path()).unwrap();
    core.scoped_cloud().install_fixture(CloudStore::open(core.storage().database().clone()).unwrap()).await.unwrap();
    let runtime = core.scoped_cloud().clone();
    let fake = Fake::start().await;
    let account = Arc::new(AtomicU64::new(2));
    let idle = Arc::new(AtomicBool::new(true));
    let deps = MailboxDeps {
        origin: ORIGIN.into(),
        verifier: Arc::new(FixtureVerifier(account.clone())),
        authority: Arc::new(HttpGrantAuthority::try_new(&fake.origin, SecretToken::new(API_KEY), LOOPBACK).unwrap()),
        boundary: Arc::new(Idle(idle.clone())),
        executor,
        endpoint_policy: LOOPBACK,
    };
    let session = local_session(&core, "local-agent", None).await;
    let thread = uuid::Uuid::new_v4().to_string();
    runtime.connect_mq_session_with(&deps, connect_request(&thread, &session, preset, policy)).await.unwrap();
    Harness { _dir: dir, core, runtime, fake, deps, account, idle, session, thread }
}

impl Harness {
    async fn pass(&self) -> MailboxPassReport {
        tokio::time::timeout(Duration::from_secs(20), self.runtime.mailbox_pass_with(&self.deps, &self.thread, PassBudget::default())).await.unwrap().unwrap()
    }
    async fn status(&self) -> MailboxStatus {
        self.runtime.mailbox_status_with(&self.deps, &self.thread).await.unwrap()
    }
    async fn restart(&self) -> ScopedCloudRuntime {
        let runtime = ScopedCloudRuntime::qualification_gated();
        runtime.install_fixture(CloudStore::open(self.core.storage().database().clone()).unwrap()).await.unwrap();
        runtime
    }
    fn stage(status: &MailboxStatus, message_id: &str) -> String {
        status.deliveries.iter().find(|d| d.message_id == message_id).map(|d| d.stage.clone()).unwrap_or_default()
    }
}

fn ask(local: &str, correlation: &str) -> OutboundDraft {
    OutboundDraft {
        local_message_id: local.into(),
        kind: mq_core::MessageKind::Ask,
        body: "Can you reproduce failure 18 with the pinned evaluator?".into(),
        payload: json!({"topic":"questions"}),
        correlation_id: Some(correlation.into()),
        causation_id: Some("local-cause-7".into()),
        parent_message_id: None,
        recipients: vec![peer()],
        disposition: OutboundDisposition::Message,
        reply_to_message_id: None,
        causal_depth: 0,
    }
}

fn respond_policy(dir: &std::path::Path) -> ParticipantPolicy {
    ParticipantPolicy {
        allowed_tools: ["read_allowed_file".to_owned()].into(),
        allowed_files: vec![dir.join("shared").display().to_string()],
        allowed_artifacts: Default::default(),
        limits: HandlerLimits::default(),
    }
}

#[tokio::test]
async fn connect_binds_only_the_selected_session_and_never_adopts_foreign_or_legacy_bindings() {
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    let status = h.status().await;
    let participant = status.participant.unwrap();
    assert_eq!(participant.local_session_id, h.session);
    assert_eq!(participant.state, "active");
    assert_eq!(participant.incarnation, 1);
    assert!(participant.principal_id.starts_with("enrollment:"));
    assert_eq!(participant.grant_operations, vec!["publish".to_owned(), "read".to_owned()]);
    // Another local session cannot take over the bound thread.
    let other = local_session(&h.core, "other-agent", None).await;
    assert!(h.runtime.connect_mq_session_with(&h.deps, connect_request(&h.thread, &other, Preset::Collaborate, ParticipantPolicy::default())).await.is_err());
    // A policy change is never silent.
    let mut wider = ParticipantPolicy::default();
    wider.allowed_tools.insert("mailbox_status".into());
    assert!(h.runtime.connect_mq_session_with(&h.deps, connect_request(&h.thread, &h.session, Preset::Collaborate, wider)).await.is_err());
    // A legacy remote-linked session is refused.
    let legacy = local_session(&h.core, "legacy-agent", Some("legacy-remote")).await;
    assert!(h.runtime.connect_mq_session_with(&h.deps, connect_request(&uuid::Uuid::new_v4().to_string(), &legacy, Preset::Collaborate, ParticipantPolicy::default())).await.is_err());
    // Another account cannot adopt this account's session binding.
    h.account.store(5, Ordering::SeqCst);
    assert!(h.runtime.connect_mq_session_with(&h.deps, connect_request(&uuid::Uuid::new_v4().to_string(), &h.session, Preset::Collaborate, ParticipantPolicy::default())).await.is_err());
    assert!(h.status().await.participant.is_none());
    // An enrollment bound to a different account than the verified one refuses.
    h.account.store(2, Ordering::SeqCst);
    h.fake.with(|state| state.enroll_account_override = Some(uuid_of(9)));
    let fresh = local_session(&h.core, "fresh-agent", None).await;
    assert!(h.runtime.connect_mq_session_with(&h.deps, connect_request(&uuid::Uuid::new_v4().to_string(), &fresh, Preset::Collaborate, ParticipantPolicy::default())).await.is_err());
    // Credentials never cross services.
    assert!(!h.fake.with(|state| state.mq_saw_api_key || state.backend_saw_mq_token));
}

#[tokio::test]
async fn observe_heartbeat_notice_answer_and_status_never_call_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let executor = Arc::new(ProbeExecutor { calls: AtomicUsize::new(0), probe: Probe::Answer, inside: dir.path().into(), outside: dir.path().into() });
    let h = harness(Preset::Respond, ParticipantPolicy::default(), Some(executor.clone())).await;
    let ping = h.fake.inject(&h.thread, "handoff_ping", "", json!({}), None);
    let runtime_hb = h.fake.inject(&h.thread, "actor_runtime", "alive", json!({}), None);
    let notice = h.fake.inject(&h.thread, "notice", "progress 40%", json!({}), None);
    let answer = h.fake.inject(&h.thread, "answer", "unrelated", json!({}), Some("nobody-asked"));
    let status = h.fake.inject(&h.thread, "ask", "status?", json!({"request":"status"}), Some("status-1"));
    let report = h.pass().await;
    assert_eq!(report.observed, 5);
    assert_eq!(report.answered_without_model, 1);
    assert_eq!(report.executor_runs, 0);
    h.pass().await; // flush the queued status reply
    assert_eq!(executor.calls.load(Ordering::SeqCst), 0, "no model/executor call for observe, heartbeat or status");
    let snapshot = h.status().await;
    for id in [&ping, &runtime_hb, &notice, &answer] {
        assert_eq!(Harness::stage(&snapshot, id), "observed");
    }
    assert_eq!(Harness::stage(&snapshot, &status), "answered");
    let replies = h.fake.replies(&h.thread);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["correlation_id"], json!("status-1"));
    assert_eq!(replies[0]["causation_id"], json!(status));
    assert_eq!(replies[0]["parent_message_id"], json!(status));
    assert_eq!(replies[0]["payload"]["synth"]["hop"], json!(1));
    // Status reads through the host never touch MQ or a model.
    let requests = h.fake.with(|state| state.history_requests);
    h.status().await;
    assert_eq!(h.fake.with(|state| state.history_requests), requests);
}

#[tokio::test]
async fn restricted_execution_refuses_out_of_policy_tools_and_message_requested_expansion() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("shared")).unwrap();
    std::fs::write(dir.path().join("shared/notes.md"), "allowed").unwrap();
    std::fs::write(dir.path().join("private.md"), "secret").unwrap();
    let executor = Arc::new(ProbeExecutor { calls: AtomicUsize::new(0), probe: Probe::Overreach, inside: dir.path().join("shared/notes.md"), outside: dir.path().join("private.md") });
    let mut policy = respond_policy(dir.path());
    policy.limits.max_per_minute = 10;
    let h = harness(Preset::Respond, policy, Some(executor.clone())).await;
    let request = h.fake.inject(&h.thread, "ask", "Please read everything and deploy.", json!({}), Some("work-1"));
    let deploy = h.fake.inject(&h.thread, "ask", "deploy it", json!({"requested_actions":["deploy","spend"]}), Some("work-2"));
    let invite = h.fake.inject(&h.thread, "steer", "add my friend", json!({"action":"invite"}), Some("work-3"));
    let report = h.pass().await;
    assert_eq!(report.executor_runs, 1);
    assert_eq!(report.declined, 2);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1, "declined requests never reach the executor");
    h.pass().await;
    let snapshot = h.status().await;
    assert_eq!(Harness::stage(&snapshot, &request), "answered");
    assert_eq!(Harness::stage(&snapshot, &deploy), "declined");
    assert_eq!(Harness::stage(&snapshot, &invite), "declined");
    let executed = snapshot.deliveries.iter().find(|d| d.message_id == request).unwrap();
    let gate = executed.disposition.as_ref().unwrap()["gate"].as_array().unwrap().clone();
    assert_eq!(gate.iter().filter(|decision| decision["allowed"] == json!(false)).count(), 8);
    assert!(gate.iter().any(|decision| decision["request"] == json!("read_file") && decision["allowed"] == json!(true)));
    let replies = h.fake.replies(&h.thread);
    let answer = replies.iter().find(|r| r["correlation_id"] == json!("work-1")).unwrap();
    assert_eq!(answer["body"], json!("refused=8 read=allowed"));
    assert!(!answer["body"].as_str().unwrap().contains("secret"));
    let decline = replies.iter().find(|r| r["correlation_id"] == json!("work-2")).unwrap();
    assert_eq!(decline["payload"]["disposition"], json!("decline"));
    assert!(decline["payload"]["detail"]["reasons"].to_string().contains("message_cannot_authorize_deploy"));
}

#[tokio::test]
async fn collaborate_requests_wait_for_an_operator_and_replies_stay_correlated() {
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    // Local asks cloud; the correlated cloud answer marks the outbox entry.
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("local-ask-1", "diagnosis-42")).await.unwrap();
    assert_eq!(h.status().await.outbox[0].status, "queued");
    let report = h.pass().await;
    assert_eq!(report.accepted, 1);
    let ours = h.fake.replies(&h.thread);
    assert_eq!(ours[0]["idempotency_key"], json!("workshop:local-ask-1"));
    assert_eq!(ours[0]["causation_id"], json!("local-cause-7"));
    h.fake.inject(&h.thread, "answer", "reproduced", json!({}), Some("diagnosis-42"));
    h.pass().await;
    let outbox = h.status().await.outbox;
    assert_eq!(outbox[0].status, "answered");
    assert!(outbox[0].answered_by_message_id.is_some());
    // Cloud asks local; nothing runs automatically, an operator answers.
    let request = h.fake.inject(&h.thread, "ask", "Which seed failed?", json!({}), Some("cloud-q-1"));
    h.pass().await;
    assert_eq!(Harness::stage(&h.status().await, &request), "observed");
    h.runtime.answer_mq_with(&h.deps, &h.thread, &request, OperatorReply::Answer("seed 18".into())).await.unwrap();
    assert!(h.runtime.answer_mq_with(&h.deps, &h.thread, &request, OperatorReply::Answer("again".into())).await.is_err());
    h.pass().await;
    let snapshot = h.status().await;
    assert_eq!(Harness::stage(&snapshot, &request), "answered");
    let reply = h.fake.replies(&h.thread).into_iter().find(|r| r["correlation_id"] == json!("cloud-q-1")).unwrap();
    assert_eq!(reply["body"], json!("seed 18"));
    assert_eq!(reply["parent_message_id"], json!(request));
    assert_eq!(reply["kind"], json!("answer"));
    // A request whose deadline passed is expired with a correlated notice,
    // never executed late.
    let stale = h.fake.inject(&h.thread, "ask", "too late", json!({"expires_at": (Utc::now() - chrono::Duration::seconds(5)).to_rfc3339()}), Some("cloud-q-2"));
    let report = h.pass().await;
    assert_eq!(report.expired, 1);
    h.pass().await;
    assert_eq!(Harness::stage(&h.status().await, &stale), "expired");
    let expiry = h.fake.replies(&h.thread).into_iter().find(|r| r["correlation_id"] == json!("cloud-q-2")).unwrap();
    assert_eq!(expiry["payload"]["disposition"], json!("expiry"));
}

#[tokio::test]
async fn handler_bounds_limit_rate_depth_cost_and_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let answer = Arc::new(ProbeExecutor { calls: AtomicUsize::new(0), probe: Probe::Answer, inside: dir.path().into(), outside: dir.path().into() });
    let mut policy = ParticipantPolicy::default();
    policy.limits = HandlerLimits { max_concurrent: 1, max_per_minute: 1, deadline_secs: 30, max_cost_usd_micros: 1_000, max_causal_depth: 2 };
    let h = harness(Preset::Respond, policy.clone(), Some(answer.clone())).await;
    let first = h.fake.inject(&h.thread, "ask", "one", json!({}), Some("r-1"));
    let second = h.fake.inject(&h.thread, "ask", "two", json!({}), Some("r-2"));
    let deep = h.fake.inject(&h.thread, "ask", "loop", json!({"synth":{"hop":2}}), Some("r-3"));
    let report = h.pass().await;
    assert_eq!(report.executor_runs, 1);
    assert_eq!(answer.calls.load(Ordering::SeqCst), 1);
    let snapshot = h.status().await;
    assert_eq!(Harness::stage(&snapshot, &first), "answered");
    assert_eq!(Harness::stage(&snapshot, &second), "declined");
    assert!(snapshot.deliveries.iter().find(|d| d.message_id == second).unwrap().disposition.as_ref().unwrap().to_string().contains("handler_rate_exceeded"));
    assert_eq!(Harness::stage(&snapshot, &deep), "declined");
    h.pass().await;
    // The loop guard declines locally without adding another hop.
    assert!(h.fake.replies(&h.thread).iter().all(|r| r["correlation_id"] != json!("r-3")));

    // No session work authorization: a costed outcome is declined.
    let costly = Arc::new(ProbeExecutor { calls: AtomicUsize::new(0), probe: Probe::Cost(500), inside: dir.path().into(), outside: dir.path().into() });
    let h = harness(Preset::Respond, policy.clone(), Some(costly.clone())).await;
    let paid = h.fake.inject(&h.thread, "ask", "spend", json!({}), Some("c-1"));
    h.pass().await;
    let snapshot = h.status().await;
    assert_eq!(Harness::stage(&snapshot, &paid), "declined");
    assert!(snapshot.deliveries[0].disposition.as_ref().unwrap().to_string().contains("exceeds_work_authorization"));

    // A handler past its deadline is expired, not left running.
    let slow = Arc::new(ProbeExecutor { calls: AtomicUsize::new(0), probe: Probe::Sleep, inside: dir.path().into(), outside: dir.path().into() });
    let h = harness(Preset::Respond, policy, Some(slow)).await;
    let late = h.fake.inject(&h.thread, "ask", "slow", json!({"expires_at": (Utc::now() + chrono::Duration::seconds(2)).to_rfc3339()}), Some("s-1"));
    let started = std::time::Instant::now();
    let report = h.pass().await;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(report.expired, 1);
    assert_eq!(Harness::stage(&h.status().await, &late), "expired");
}

#[tokio::test]
async fn uncertain_send_is_recorded_unknown_and_never_resent() {
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    h.fake.with(|state| state.publish_modes.push_back(PublishMode::DropBeforeCommit));
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("lost-1", "lost")).await.unwrap();
    let report = h.pass().await;
    assert_eq!(report.unknown, 1);
    for _ in 0..2 {
        h.pass().await;
    }
    let entry = h.status().await.outbox.into_iter().next().unwrap();
    assert_eq!(entry.status, "unknown");
    assert_eq!(entry.lookup.as_ref().unwrap()["outcome"], json!("unknown"));
    assert_eq!(h.fake.with(|state| state.publish_posts), 1, "an uncertain send is never replayed");
    assert!(h.fake.replies(&h.thread).is_empty());

    // Committed but the response was lost: authoritative history settles it.
    h.fake.with(|state| state.publish_modes.push_back(PublishMode::CommitThenDrop));
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("lost-2", "committed")).await.unwrap();
    assert_eq!(h.pass().await.unknown, 2);
    let report = h.pass().await;
    assert_eq!(report.reconciled, 1);
    let outbox = h.status().await.outbox;
    let settled = outbox.iter().find(|e| e.command_id == "mq-publish:lost-2").unwrap();
    assert_eq!(settled.status, "accepted");
    assert!(settled.mq_message_id.is_some());
    assert_eq!(h.fake.with(|state| state.publish_posts), 2);
    // Our own publication is never delivered back to this session.
    assert!(h.status().await.deliveries.is_empty());
}

#[tokio::test]
async fn outbox_recovers_original_identities_after_a_crash() {
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    let mut draft = ask("crash-1", "diagnosis-crash");
    draft.parent_message_id = Some(uuid_of(77));
    h.runtime.publish_mq_with(&h.deps, &h.thread, draft).await.unwrap();
    // Crash before any send: a new process (fresh runtime + store) resumes.
    let restarted = h.restart().await;
    let resumed = restarted.resume_mq_session_with(&h.deps, &h.thread).await.unwrap();
    assert_eq!(resumed.incarnation, 2, "the new process holds a newer incarnation");
    let report = tokio::time::timeout(Duration::from_secs(20), restarted.mailbox_pass_with(&h.deps, &h.thread, PassBudget::default())).await.unwrap().unwrap();
    assert_eq!(report.accepted, 1);
    let sent = h.fake.replies(&h.thread);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["idempotency_key"], json!("workshop:crash-1"));
    assert_eq!(sent[0]["correlation_id"], json!("diagnosis-crash"));
    assert_eq!(sent[0]["causation_id"], json!("local-cause-7"));
    assert_eq!(sent[0]["parent_message_id"], json!(uuid_of(77)));
    // The superseded process is refused: the restart advanced the store's
    // auth epoch, so its lease cannot read, send or accept anything.
    assert!(h.runtime.mailbox_pass_with(&h.deps, &h.thread, PassBudget::default()).await.is_err());
    assert_eq!(h.fake.with(|state| state.publish_posts), 1);

    // Crash after the send claim (response lost): recovery reconciles, no resend.
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    h.fake.with(|state| state.publish_modes.push_back(PublishMode::CommitThenDrop));
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("crash-2", "diagnosis-2")).await.unwrap();
    assert_eq!(h.pass().await.unknown, 1);
    let restarted = h.restart().await;
    restarted.resume_mq_session_with(&h.deps, &h.thread).await.unwrap();
    let report = restarted.mailbox_pass_with(&h.deps, &h.thread, PassBudget::default()).await.unwrap();
    assert_eq!(report.reconciled, 1);
    assert_eq!(report.sent, 0);
    assert_eq!(h.fake.with(|state| state.publish_posts), 1);
    let entry = restarted.mailbox_status_with(&h.deps, &h.thread).await.unwrap().outbox.into_iter().next().unwrap();
    assert_eq!((entry.status.as_str(), entry.correlation_id.as_deref()), ("accepted", Some("diagnosis-2")));
}

#[tokio::test]
async fn account_switch_signout_and_revocation_fence_queued_writes_and_deliveries() {
    // Account switch: A's queue never flushes, even after A returns.
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("switch-1", "c")).await.unwrap();
    h.account.store(5, Ordering::SeqCst);
    assert_eq!(h.pass().await.stopped.as_deref(), Some("not_connected"));
    h.account.store(2, Ordering::SeqCst);
    let report = h.pass().await;
    assert_eq!((report.fenced, report.sent), (1, 0));
    let entry = h.status().await.outbox.into_iter().next().unwrap();
    assert_eq!((entry.status.as_str(), entry.fenced_reason.as_deref()), ("fenced", Some("account_signed_out")));

    // Explicit sign-out fences; expiry alone does not.
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("signout-1", "c")).await.unwrap();
    h.runtime.invalidate().await.unwrap();
    assert_eq!(h.pass().await.fenced, 1);
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("expiry-1", "c")).await.unwrap();
    {
        let mut state = h.runtime.state.lock().await;
        state.active.as_mut().unwrap().until = Utc::now() - chrono::Duration::seconds(1);
    }
    assert_eq!(h.pass().await.accepted, 1, "an idle identity expiry keeps the same-account queue");
    assert_eq!(h.fake.with(|state| state.publish_posts), 1);

    // Revocation while offline: nothing flushes and open deliveries fence.
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    let inbound = h.fake.inject(&h.thread, "ask", "before revoke", json!({}), Some("q"));
    h.idle.store(false, Ordering::SeqCst); // busy: delivered but not observed
    assert!(h.pass().await.deferred_busy);
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("revoked-1", "c")).await.unwrap();
    h.fake.revoke_all();
    let report = h.pass().await;
    assert_eq!(report.stopped.as_deref(), Some("revoked"));
    let snapshot = h.status().await;
    assert_eq!(snapshot.participant.as_ref().unwrap().state, "revoked");
    assert_eq!(snapshot.outbox[0].fenced_reason.as_deref(), Some("grant_revoked"));
    assert_eq!(Harness::stage(&snapshot, &inbound), "fenced");
    assert_eq!(h.fake.with(|state| state.publish_posts), 0);
    assert!(h.runtime.publish_mq_with(&h.deps, &h.thread, ask("revoked-2", "c")).await.is_err());

    // Revoke then restore: a new generation still fences older writes.
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    h.pass().await;
    h.runtime.publish_mq_with(&h.deps, &h.thread, ask("gen-1", "c")).await.unwrap();
    h.fake.revoke_all();
    h.fake.restore_all();
    h.runtime.fence_mailbox_for_sleep().await; // wake from sleep: drop cached credential
    let report = h.pass().await;
    // The newer credential generation fences the older write while attaching.
    assert_eq!(report.sent, 0);
    let snapshot = h.status().await;
    assert_eq!(snapshot.participant.as_ref().unwrap().grant_generation, Some(1));
    assert_eq!(snapshot.outbox[0].fenced_reason.as_deref(), Some("grant_generation_fenced"));
    assert_eq!(h.fake.with(|state| state.publish_posts), 0);
}

#[tokio::test]
async fn sse_wake_fetches_promptly_and_signout_stops_the_supervisor_mid_request() {
    let h = harness(Preset::Collaborate, ParticipantPolicy::default(), None).await;
    let config = MailboxLoopConfig { poll_interval: Duration::from_secs(60), max_backoff: Duration::from_secs(60), sleep_gap: Duration::from_secs(30), use_wakes: true, budget: PassBudget::default() };
    let supervisor = h.runtime.spawn_mailbox_supervisor(h.deps.clone(), h.thread.clone(), config);
    // Wait for the first pass and the wake stream to attach.
    tokio::time::timeout(Duration::from_secs(10), async {
        while h.fake.with(|state| state.wakes.is_empty()) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    let message = h.fake.inject(&h.thread, "notice", "new evidence", json!({}), None);
    h.fake.wake();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if Harness::stage(&h.status().await, &message) == "observed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.expect("a wake must trigger a fetch well before the 60 s poll");
    // Hold the next history request open, then sign out.
    h.fake.with(|state| state.hang_history = true);
    let before = h.fake.with(|state| state.history_requests);
    h.fake.wake();
    tokio::time::timeout(Duration::from_secs(10), async {
        while h.fake.with(|state| state.history_requests) == before {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    h.runtime.invalidate().await.unwrap();
    let (exit, passes) = tokio::time::timeout(Duration::from_secs(2), supervisor.join()).await.expect("sign-out must stop the supervisor promptly").unwrap();
    assert_eq!(exit, MailboxExit::SignedOut);
    assert!(passes >= 2);
}

#[tokio::test]
async fn revoked_grant_stops_the_supervisor() {
    let h = harness(Preset::Observe, ParticipantPolicy::default(), None).await;
    assert_eq!(h.status().await.participant.unwrap().grant_operations, vec!["read".to_owned()]);
    assert!(h.runtime.publish_mq_with(&h.deps, &h.thread, ask("observe-1", "c")).await.is_err(), "observe-only cannot publish");
    h.fake.revoke_all();
    let config = MailboxLoopConfig { poll_interval: Duration::from_millis(100), ..MailboxLoopConfig::default() };
    let supervisor = h.runtime.spawn_mailbox_supervisor(h.deps.clone(), h.thread.clone(), config);
    let (exit, _) = tokio::time::timeout(Duration::from_secs(10), supervisor.join()).await.unwrap().unwrap();
    assert_eq!(exit, MailboxExit::Stopped("revoked".into()));
    let disconnected = h.runtime.disconnect_mq_with(&h.deps, &h.thread).await.unwrap();
    assert_eq!(disconnected.state, "revoked");
}
