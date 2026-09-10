//! Native authority for generic visual presentation. React and MCP use this same
//! transaction/reducer. Domain effects are deliberately not presentation actions.
use crate::storage::Database;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::Emitter;

static EVENT_HOST: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
pub fn attach_event_host(app: tauri::AppHandle) {
    let _ = EVENT_HOST.set(app);
}

pub struct VisualEngine {
    db: Arc<Database>,
}
const SCHEMA: &str = "synth.visual-session.v1";


fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("{key} is required"))
}

// Canonical v1 uses tagged JSON with IEEE754 numeric bits and UTF16-sorted keys.
// Unlike ordinary JSON text this is identical across JS and Rust number printers.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Null => json!(["null"]),
        Value::Bool(v) => json!(["boolean", v]),
        Value::Number(v) => {
            let n = v.as_f64().unwrap_or(0.0);
            json!([
                "number",
                format!("{:016x}", if n == 0.0 { 0 } else { n.to_bits() })
            ])
        }
        Value::String(v) => json!(["string", v]),
        Value::Array(v) => json!(["array", v.iter().map(canonical).collect::<Vec<_>>()]),
        Value::Object(v) => {
            let mut keys = v.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            json!([
                "object",
                keys.into_iter()
                    .map(|key| json!([key, canonical(&v[key])]))
                    .collect::<Vec<_>>()
            ])
        }
    }
}
fn digest(value: &Value) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(canonical(value).to_string().as_bytes())
    )
}

fn validate_json(value: &Value, depth: usize) -> Result<()> {
    if depth > 40 {
        bail!("visual state exceeds maximum nesting");
    }
    match value {
        Value::Array(values) => {
            for value in values {
                validate_json(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if ["__proto__", "prototype", "constructor"].contains(&key.as_str()) {
                    bail!("unsafe visual state key");
                }
                validate_json(value, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn validate_control(control: &Value, value: &Value) -> Result<()> {
    validate_value(control, value, 0)
}
fn validate_value(control: &Value, value: &Value, depth: usize) -> Result<()> {
    if depth > 40 {
        bail!("visual schema exceeds maximum nesting");
    }
    if value.is_null() && control["nullable"] == true {
        return Ok(());
    }
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    if control["type"] != kind {
        bail!("control {} requires {}", control["id"], control["type"]);
    }
    if let Some(variants)=control.get("oneOf") {
        let variants=variants.as_array().context("visual union requires variants")?;
        if variants.is_empty() || variants.len()>16 { bail!("visual union requires 1..16 variants"); }
        if variants.iter().filter(|schema|validate_value(schema,value,depth+1).is_ok()).count()!=1 { bail!("value must match exactly one declared variant"); }
    }
    if let Some(options) = control["options"].as_array() {
        if !options.contains(value) {
            bail!("value is not an allowed option");
        }
    }
    if let Some(number) = value.as_f64() {
        if control["minimum"].as_f64().is_some_and(|min| number < min)
            || control["maximum"].as_f64().is_some_and(|max| number > max)
        {
            bail!("value is outside declared range");
        }
    }
    if let Some(items) = value.as_array() {
        if control["maxItems"]
            .as_u64()
            .is_some_and(|max| items.len() as u64 > max)
        {
            bail!("control exceeds item limit");
        }
        if let Some(schema) = control.get("items") {
            for item in items {
                validate_value(schema, item, depth + 1)?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = control["required"].as_array() {
            for key in required {
                if !object.contains_key(key.as_str().context("invalid required property")?) {
                    bail!("missing required control property {key}");
                }
            }
        }
        for (key, item) in object {
            if let Some(schema) = control["properties"].get(key) {
                validate_value(schema, item, depth + 1)?;
            } else if control["additionalProperties"] == false {
                bail!("unknown control property {key}");
            } else if control["additionalProperties"].is_object() {
                validate_value(&control["additionalProperties"], item, depth + 1)?;
            }
        }
    }
    Ok(())
}
fn validate_state(state: &Value) -> Result<()> {
    validate_json(state, 0)?;
    if state["schemaVersion"] != SCHEMA || !state["values"].is_object() {
        bail!("invalid visual session schema or values");
    }
    field(&state["definition"], "id")?;
    field(&state["definition"], "version")?;
    if state["stateVersion"].as_u64().is_none() {
        bail!("invalid state version");
    }
    if let Some(replay) = state.get("replay") {
        let valid = replay.as_object().is_some_and(|object| {
            (object.len() == 1 && field(replay, "checkpointId").is_ok())
                || (object.keys().all(|key| matches!(key.as_str(),"recordingId"|"sequence"|"eventCount"|"playing"|"intervalMs")) && field(replay, "recordingId").is_ok()
                    && replay.get("intervalMs").is_none_or(|value|value.as_u64().is_some_and(|n|(16..=60_000).contains(&n)))
                    && replay["sequence"].as_u64().is_some_and(|n| n <= 9_007_199_254_740_991)
                    && replay.get("playing").is_none_or(Value::is_boolean)
                    && replay.get("eventCount").is_none_or(|count| count.as_u64().is_some_and(|n| n >= replay["sequence"].as_u64().unwrap_or(u64::MAX) && n <= 9_007_199_254_740_991)))
        });
        if !valid { bail!("invalid replay cursor"); }
    }
    let controls = state["controls"]
        .as_array()
        .context("controls are required")?;
    if controls.len() > 512 {
        bail!("visual has too many controls");
    }
    let mut ids = std::collections::HashSet::new();
    for control in controls {
        let id = field(control, "id")?;
        if !ids.insert(id) {
            bail!("duplicate control {id}");
        }
        validate_control(
            control,
            state["values"].get(id).context("control value missing")?,
        )?;
    }
    if state["values"]
        .as_object()
        .unwrap()
        .keys()
        .any(|id| !ids.contains(id.as_str()))
    {
        bail!("unregistered visual state value");
    }
    if state.to_string().len() > 1_048_576 {
        bail!("visual state exceeds 1 MiB; use evidence references");
    }
    Ok(())
}
fn checkpoint(state: &Value) -> Value {
    json!({"schemaVersion":"synth.visual-checkpoint.v1","id":uuid::Uuid::new_v4().to_string(),"capturedAt":Utc::now().to_rfc3339(),"state":state,"digest":digest(state),"renditionRefs":[]})
}

impl VisualEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub async fn request(&self, visual_id: String, request: Value) -> Result<Value> {
        // Serialize the freeze boundary with every presentation commit. A
        // dropped capture cannot strand a session: its barrier expires.
        static FREEZES: std::sync::OnceLock<tokio::sync::Mutex<std::collections::HashMap<String,(String,std::time::Instant,Value)>>> = std::sync::OnceLock::new();
        let key=format!("{:p}:{}",Arc::as_ptr(&self.db),json!([visual_id,request["revision"],request["viewKey"].as_str().unwrap_or("default")]));
        let mut freezes=FREEZES.get_or_init(Default::default).lock().await;
        freezes.retain(|_,(_,until,_)|*until>std::time::Instant::now());
        let op=field(&request,"operation")?;
        if op=="capture.freeze" {
            if freezes.contains_key(&key){bail!("visual capture already in progress");}
            let mut inspect=request.clone();inspect["operation"]=json!("inspect");
            let response=self.request_unlocked(visual_id,inspect).await?;
            let token=uuid::Uuid::new_v4().to_string();
            let checkpoint=checkpoint(&response["state"]);
            freezes.insert(key,(token.clone(),std::time::Instant::now()+std::time::Duration::from_secs(30),checkpoint.clone()));
            return Ok(json!({"token":token,"checkpoint":checkpoint}));
        }
        if op=="capture.release" {
            let (token,_,checkpoint)=freezes.get(&key).context("visual capture barrier expired")?;
            if request["token"]!=*token{bail!("visual capture token mismatch");}
            let checkpoint=checkpoint.clone();
            freezes.remove(&key);
            return Ok(json!({"checkpoint":checkpoint}));
        }
        if freezes.contains_key(&key) && !matches!(op,"inspect"|"checkpoints"|"checkpoint.read"|"recordings"|"record.read"|"capture"|"evidence.read") && !op.starts_with("corpus.") {
            if matches!(op,"playback.tick"|"record.tick") {
                let mut inspect=request.clone();inspect["operation"]=json!("inspect");
                let mut response=self.request_unlocked(visual_id,inspect).await?;
                response["tickApplied"]=json!(false);
                return Ok(response);
            }
            bail!("visual capture in progress; retry after the capture barrier releases");
        }
        self.request_unlocked(visual_id,request).await
    }
    async fn request_unlocked(&self, visual_id: String, request: Value) -> Result<Value> {
        if request["operation"] == "playback.tick" || request["operation"] == "record.tick" {
            // One native scheduler gate per clock, regardless of how many panes
            // are mounted. Only committed ticks consume the interval. The lease
            // is intentionally ephemeral; presentation and events are durable.
            static TICKS: std::sync::OnceLock<tokio::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();
            let clock = field(&request, "clock")?;
            let interval = request["intervalMs"].as_u64().filter(|n| (16..=60_000).contains(n)).context("playback interval must be 16..60000 ms")?;
            if clock.len() > 200 || request.to_string().len() > 2_097_152 { bail!("invalid playback request"); }
            let key = format!("{:p}:{}", Arc::as_ptr(&self.db), json!([visual_id, request["revision"], request.get("viewKey").unwrap_or(&json!("default")), clock]));
            let mut ticks = TICKS.get_or_init(Default::default).lock().await;
            let now = std::time::Instant::now();
            ticks.retain(|_, time| now.duration_since(*time).as_secs() < 60);
            let mut inner = request.clone();
            if ticks.get(&key).is_some_and(|time| now.duration_since(*time).as_millis() < interval as u128) {
                inner["operation"] = json!("inspect");
                let mut response = self.request_inner(visual_id, inner).await?;
                response["tickApplied"] = json!(false);
                return Ok(response);
            }
            if ticks.len() >= 10_000 { bail!("too many active visual clocks"); }
            inner["operation"] = if request["operation"] == "record.tick" { json!("record.commit") } else { json!("playback.commit") };
            let mut response = self.request_inner(visual_id, inner).await?;
            if response["tickApplied"] != false {
                ticks.insert(key, std::time::Instant::now());
                response["tickApplied"] = json!(true);
            }
            return Ok(response);
        }
        self.request_inner(visual_id, request).await
    }
    async fn request_inner(&self, visual_id: String, request: Value) -> Result<Value> {
        let op = field(&request, "operation")?.to_owned();
        if op.starts_with("corpus.") {
            return super::query_engine::request(self.db.clone(), visual_id, request).await;
        }
        let view = request["viewKey"].as_str().unwrap_or("default").to_owned();
        let revision = request["revision"]
            .as_i64()
            .filter(|r| *r > 0)
            .context("positive revision required")?;
        if view.is_empty() || view.len() > 200 || request.to_string().len() > 2_097_152 {
            bail!("invalid visual request size or view");
        }
        validate_json(&request, 0)?;
        let read_only = matches!(
            op.as_str(),
            "inspect" | "checkpoints" | "checkpoint.read" | "recordings" | "record.read" | "evidence.read"
        );
        let signal = json!({"visualId":visual_id,"revision":revision,"viewKey":view});
        let execute = move |conn: &rusqlite::Connection| {
            let raw: Option<String> = conn.query_row("SELECT state_json FROM visual_engine_sessions WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view],|row|row.get(0)).optional()?;
            let mut state: Value = if let Some(raw) = raw {
                serde_json::from_str(&raw)?
            } else {
                if op != "attach" {
                    bail!("visual session unavailable; open this visual revision first");
                }
                json!({"schemaVersion":SCHEMA,"visualId":visual_id,"revision":revision,"viewKey":view,"stateVersion":0,"definition":request["definition"],"values":{},"controls":[]})
            };
            let version = state["stateVersion"]
                .as_i64()
                .context("invalid stored state version")?;
            let active: Option<String> = conn.query_row("SELECT active_recording FROM visual_engine_sessions WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view],|row|row.get(0)).optional()?.flatten();
            match op.as_str() {
                "attach" | "publish" => {
                    if op == "publish" && request["expectedStateVersion"].as_i64() != Some(version)
                    {
                        bail!("stale visual publication");
                    }
                    if op == "attach" && request["definition"] != state["definition"] {
                        bail!("visual definition version mismatch");
                    }
                    if let Some(controls) = request["controls"].as_array() {
                        for control in controls {
                            let id = field(control, "id")?;
                            let known = state["controls"]
                                .as_array_mut()
                                .context("invalid controls")?;
                            if let Some(previous) = known.iter().find(|c| c["id"] == id) {
                                if previous != control {
                                    bail!("conflicting control {id}");
                                }
                            } else {
                                known.push(control.clone());
                            }
                            if !state["values"].as_object().unwrap().contains_key(id) {
                                let value = request["defaults"]
                                    .get(id)
                                    .context("new control needs a default")?;
                                validate_control(control, value)?;
                                state["values"][id] = value.clone();
                            }
                        }
                    }
                    if let Some(scene) = request.get("scene") {
                        if scene["visualId"] != visual_id
                            || scene["revision"] != revision
                            || scene["stateVersion"] != version
                        {
                            bail!("scene identity/version mismatch");
                        }
                        state["scene"] = scene.clone();
                    }
                    validate_state(&state)?;
                    conn.execute("INSERT INTO visual_engine_sessions(visual_id,revision,view_key,state_json,state_version,published_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(visual_id,revision,view_key) DO UPDATE SET state_json=excluded.state_json,published_at=excluded.published_at",params![visual_id,revision,view,state.to_string(),version,Utc::now().to_rfc3339()])?;
                    Ok(json!({"state":state,"activeRecording":active}))
                }
                "inspect" => Ok(json!({"state":state,"activeRecording":active})),
                "evidence.put" => {
                    let value=request.get("value").context("evidence value required")?;
                    let raw=value.to_string();
                    if raw.len()>1_500_000 {bail!("evidence cut exceeds 1.5 MB; use a domain paginated source adapter");}
                    let hash=digest(value);
                    conn.execute("INSERT OR IGNORE INTO visual_evidence_cuts VALUES(?1,?2,?3,?4,?5)",params![visual_id,revision,hash,raw,Utc::now().to_rfc3339()])?;
                    Ok(json!({"digest":hash}))
                }
                "evidence.read" => {
                    let hash=field(&request,"digest")?;
                    let raw:String=conn.query_row("SELECT value_json FROM visual_evidence_cuts WHERE visual_id=?1 AND visual_revision=?2 AND digest=?3",params![visual_id,revision,hash],|r|r.get(0)).optional()?.context("pinned evidence unavailable for this visual revision")?;
                    let value:Value=serde_json::from_str(&raw)?;
                    if digest(&value)!=hash {bail!("pinned evidence digest mismatch");}
                    Ok(json!({"digest":hash,"value":value}))
                }
                "act" | "playback.commit" => {
                    if op == "playback.commit" {
                        let playing = field(&request, "playingControl")?;
                        if state.get("replay").is_some() || state["values"][playing] != true || request["action"]["expectedStateVersion"].as_i64() != Some(version) {
                            return Ok(json!({"state":state,"activeRecording":active,"tickApplied":false}));
                        }
                    }
                    let action = request.get("action").context("action required")?;
                    let command_id = field(action, "id")?;
                    let key = action["idempotencyKey"].as_str().unwrap_or(command_id);
                    let prior: Option<(String,String)> = conn.query_row("SELECT request_json,receipt_json FROM visual_engine_receipts WHERE visual_id=?1 AND revision=?2 AND view_key=?3 AND command_key=?4",params![visual_id,revision,view,key],|row| Ok((row.get(0)?,row.get(1)?))).optional()?;
                    if let Some((input, receipt)) = prior {
                        if serde_json::from_str::<Value>(&input)? != *action {
                            bail!("idempotency key reused with different input");
                        }
                        let mut receipt: Value = serde_json::from_str(&receipt)?;
                        receipt["duplicate"] = json!(true);
                        return Ok(receipt);
                    }
                    if action["expectedStateVersion"].as_i64() != Some(version) {
                        bail!(
                            "stale visual state: expected {}, current {version}",
                            action["expectedStateVersion"]
                        );
                    }
                    let patch = match action["kind"].as_str() {
                        Some("presentation.set") => {
                            let id = field(&action["target"], "id")?;
                            let value = action["payload"]
                                .get("value")
                                .context("control value required")?;
                            let mut map = serde_json::Map::new();
                            map.insert(id.to_owned(), value.clone());
                            map
                        }
                        Some("presentation.patch") => action["payload"]["values"]
                            .as_object()
                            .context("control values required")?
                            .clone(),
                        _ => bail!(
                            "unsupported visual action; use an advertised presentation control"
                        ),
                    };
                    if patch.is_empty() {
                        bail!("control values cannot be empty");
                    }
                    let before = state.clone();
                    state.as_object_mut().unwrap().remove("replay");
                    for (id, value) in patch {
                        let control = state["controls"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|c| c["id"] == id)
                            .context("unknown visual control")?;
                        validate_control(control, &value)?;
                        state["values"][&id] = value;
                    }
                    state["stateVersion"] = json!(version + 1);
                    // The previous scene belongs to the previous committed state.
                    state.as_object_mut().unwrap().remove("scene");
                    validate_state(&state)?;
                    let receipt = json!({"commandId":command_id,"state":state});
                    conn.execute("UPDATE visual_engine_sessions SET state_json=?4,state_version=?5 WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view,state.to_string(),version+1])?;
                    conn.execute(
                        "INSERT INTO visual_engine_receipts VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            visual_id,
                            revision,
                            view,
                            key,
                            action.to_string(),
                            receipt.to_string()
                        ],
                    )?;
                    if let Some(recording_id) = active {
                        let sequence: i64 = conn.query_row("SELECT COALESCE(MAX(sequence),0)+1 FROM visual_engine_events WHERE recording_id=?1",[&recording_id],|row|row.get(0))?;
                        let event = json!({"sequence":sequence,"action":action,"before":before,"state":state,"occurredAt":Utc::now().to_rfc3339()});
                        conn.execute(
                            "INSERT INTO visual_engine_events VALUES(?1,?2,?3)",
                            params![recording_id, sequence, event.to_string()],
                        )?;
                    }
                    Ok(receipt)
                }
                "capture" => {
                    if request["expectedStateVersion"].as_i64() != Some(version) {
                        bail!("stale capture state");
                    }
                    let saved = checkpoint(&state);
                    conn.execute(
                        "INSERT INTO visual_engine_checkpoints VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            saved["id"].as_str(),
                            visual_id,
                            revision,
                            view,
                            saved.to_string(),
                            saved["capturedAt"].as_str()
                        ],
                    )?;
                    Ok(json!({"checkpoint":saved,"state":state}))
                }
                "checkpoint.read" => {
                    let id = field(&request, "checkpointId")?;
                    let raw:String=conn.query_row("SELECT checkpoint_json FROM visual_engine_checkpoints WHERE id=?1 AND visual_id=?2 AND revision=?3 AND view_key=?4",params![id,visual_id,revision,view],|row|row.get(0)).optional()?.context("Checkpoint not found for this visual revision and view")?;
                    Ok(json!({"checkpoint":serde_json::from_str::<Value>(&raw)?}))
                }
                "checkpoint.import" => {
                    let saved = request.get("checkpoint").context("checkpoint required")?;
                    let imported = &saved["state"];
                    if saved["schemaVersion"] != "synth.visual-checkpoint.v1"
                        || saved["digest"] != digest(imported)
                    {
                        bail!("checkpoint integrity mismatch");
                    }
                    validate_state(imported)?;
                    if imported["visualId"] != visual_id
                        || imported["revision"] != revision
                        || imported["viewKey"] != view
                        || imported["definition"] != state["definition"]
                    {
                        bail!("checkpoint identity or definition mismatch");
                    }
                    for control in imported["controls"].as_array().unwrap() {
                        if !state["controls"].as_array().unwrap().contains(control) {
                            bail!(
                                "imported checkpoint has an unregistered or incompatible control"
                            );
                        }
                    }
                    // Import creates a new local identity. It does not change the
                    // live presentation, certify pixels, or execute any effects.
                    let mut local = saved.clone();
                    local["id"] = json!(uuid::Uuid::new_v4().to_string());
                    local["renditionRefs"] = json!([]);
                    conn.execute(
                        "INSERT INTO visual_engine_checkpoints VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            local["id"].as_str(),
                            visual_id,
                            revision,
                            view,
                            local.to_string(),
                            field(&local, "capturedAt")?
                        ],
                    )?;
                    Ok(json!({"checkpoint":local}))
                }
                "restore" => {
                    if active.is_some() {
                        bail!("stop recording before restoring a checkpoint");
                    }
                    if request["expectedStateVersion"].as_i64() != Some(version) {
                        bail!("stale restore state");
                    }
                    let id = field(&request, "checkpointId")?;
                    let raw: String = conn.query_row("SELECT checkpoint_json FROM visual_engine_checkpoints WHERE id=?1 AND visual_id=?2 AND revision=?3 AND view_key=?4",params![id,visual_id,revision,view],|row|row.get(0)).optional()?.context("Checkpoint not found for this visual revision and view")?;
                    let saved: Value = serde_json::from_str(&raw)?;
                    if saved["digest"] != digest(&saved["state"])
                        || saved["state"]["definition"] != state["definition"]
                    {
                        bail!("checkpoint integrity or definition mismatch");
                    }
                    state = saved["state"].clone();
                    state["replay"]=json!({"checkpointId":id});
                    state["stateVersion"] = json!(version + 1);
                    state.as_object_mut().unwrap().remove("scene");
                    conn.execute("UPDATE visual_engine_sessions SET state_json=?4,state_version=?5 WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view,state.to_string(),version+1])?;
                    Ok(json!({"state":state}))
                }
                "record.import" => {
                    let recording = request.get("recording").context("recording required")?;
                    if recording["schemaVersion"] != "synth.visual-session-recording.v1" {
                        bail!("unsupported recording schema");
                    }
                    let initial = &recording["initial"];
                    if initial["schemaVersion"] != "synth.visual-checkpoint.v1"
                        || initial["digest"] != digest(&initial["state"])
                    {
                        bail!("recording checkpoint integrity mismatch");
                    }
                    let mut previous = initial["state"].clone();
                    validate_state(&previous)?;
                    let events = recording["events"]
                        .as_array()
                        .filter(|events| events.len() <= 10000)
                        .context("bounded recording events required")?;
                    let identity_matches = |candidate: &Value| {
                        candidate["visualId"] == visual_id
                            && candidate["revision"] == revision
                            && candidate["viewKey"] == view
                            && candidate["definition"] == state["definition"]
                    };
                    let controls_match = |candidate: &Value| {
                        candidate["controls"].as_array().is_some_and(|controls| {
                            controls.iter().all(|control| {
                                state["controls"].as_array().unwrap().contains(control)
                            })
                        })
                    };
                    if !identity_matches(&previous) || !controls_match(&previous) {
                        bail!("recording identity or controls mismatch");
                    }
                    for (index, event) in events.iter().enumerate() {
                        if event["sequence"].as_u64() != Some(index as u64 + 1) {
                            bail!("recording sequence gap");
                        }
                        let before = event.get("before").unwrap_or(&previous);
                        validate_state(before)?;
                        if !identity_matches(before)
                            || !controls_match(before)
                            || before["stateVersion"] != previous["stateVersion"]
                        {
                            bail!("recording context mismatch");
                        }
                        for control in previous["controls"].as_array().unwrap() {
                            let id = field(control, "id")?;
                            if !before["controls"].as_array().unwrap().contains(control)
                                || before["values"][id] != previous["values"][id]
                            {
                                bail!("recording changed committed presentation");
                            }
                        }
                        let action = &event["action"];
                        if action["expectedStateVersion"] != before["stateVersion"] {
                            bail!("recording command version mismatch");
                        }
                        field(action, "id")?;
                        let mut expected = before["values"].as_object().unwrap().clone();
                        let patch = match action["kind"].as_str() {
                            Some("presentation.patch") => action["payload"]["values"]
                                .as_object()
                                .context("recording patch required")?
                                .clone(),
                            Some("presentation.set") => {
                                let mut values = serde_json::Map::new();
                                values.insert(
                                    field(&action["target"], "id")?.to_owned(),
                                    action["payload"]
                                        .get("value")
                                        .context("recording value required")?
                                        .clone(),
                                );
                                values
                            }
                            _ => bail!("recording contains unsupported effect or action"),
                        };
                        if patch.is_empty() {
                            bail!("recording action cannot be empty");
                        }
                        for (id, value) in patch {
                            let control = before["controls"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .find(|control| control["id"] == id)
                                .context("unregistered recording control")?;
                            validate_control(control, &value)?;
                            expected.insert(id, value);
                        }
                        let next = &event["state"];
                        validate_state(next)?;
                        if !identity_matches(next)
                            || !controls_match(next)
                            || next["values"] != json!(expected)
                            || next["stateVersion"].as_i64()
                                != before["stateVersion"]
                                    .as_i64()
                                    .and_then(|value| value.checked_add(1))
                        {
                            bail!("recording transition mismatch");
                        }
                        previous = next.clone();
                    }
                    for control in previous["controls"].as_array().unwrap() {
                        if !state["controls"].as_array().unwrap().contains(control) {
                            bail!("recording control is not registered in this host");
                        }
                    }
                    let id = uuid::Uuid::new_v4().to_string();
                    let now = Utc::now().to_rfc3339();
                    conn.execute(
                        "INSERT INTO visual_engine_recordings VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![
                            id,
                            visual_id,
                            revision,
                            view,
                            initial.to_string(),
                            field(initial, "capturedAt")?,
                            recording["endedAt"].as_str().unwrap_or(&now)
                        ],
                    )?;
                    for event in events {
                        conn.execute(
                            "INSERT INTO visual_engine_events VALUES(?1,?2,?3)",
                            params![id, event["sequence"].as_i64(), event.to_string()],
                        )?;
                    }
                    Ok(json!({"recordingId":id,"eventCount":events.len()}))
                }
                "record.play" => {
                    if active.is_some() { bail!("stop recording before replay"); }
                    if request["expectedStateVersion"].as_i64()!=Some(version) { bail!("stale replay state"); }
                    field(&state["replay"],"recordingId").context("select a recording before playback")?;
                    let playing=request["playing"].as_bool().context("playing boolean required")?;
                    if let Some(interval)=request.get("intervalMs") {
                        interval.as_u64().filter(|n|(16..=60_000).contains(n)).context("playback interval must be 16..60000 ms")?;
                        state["replay"]["intervalMs"]=interval.clone();
                    }
                    state["replay"]["playing"]=json!(playing && state["replay"]["sequence"].as_i64()<state["replay"]["eventCount"].as_i64());
                    state["stateVersion"]=json!(version+1);
                    state.as_object_mut().unwrap().remove("scene");
                    conn.execute("UPDATE visual_engine_sessions SET state_json=?4,state_version=?5 WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view,state.to_string(),version+1])?;
                    Ok(json!({"state":state}))
                }
                "record.seek" | "record.commit" => {
                    let ticking=op=="record.commit";
                    if ticking && (active.is_some() || state["replay"]["playing"]!=true || request["expectedStateVersion"].as_i64()!=Some(version)) {
                        return Ok(json!({"state":state,"tickApplied":false}));
                    }
                    if active.is_some() {
                        bail!("stop recording before replay");
                    }
                    if request["expectedStateVersion"].as_i64() != Some(version) {
                        bail!("stale replay state");
                    }
                    let replay_cursor=state["replay"].clone();
                    if ticking && replay_cursor.get("intervalMs").is_some_and(|interval|request.get("intervalMs")!=Some(interval)) {
                        return Ok(json!({"state":state,"tickApplied":false}));
                    }
                    let id = field(if ticking {&replay_cursor} else {&request}, "recordingId")?;
                    let sequence = if ticking { replay_cursor["sequence"].as_i64().context("invalid replay cursor")?+1 } else { request["sequence"]
                        .as_i64()
                        .filter(|s| *s >= 0)
                        .context("non-negative sequence required")? };
                    let count:i64=conn.query_row("SELECT COUNT(*) FROM visual_engine_events WHERE recording_id=?1",params![id],|row|row.get(0))?;
                    let initial: String = conn.query_row("SELECT initial_json FROM visual_engine_recordings WHERE id=?1 AND visual_id=?2 AND revision=?3 AND view_key=?4",params![id,visual_id,revision,view],|row|row.get(0))?;
                    let saved: Value = if sequence == 0 {
                        serde_json::from_str::<Value>(&initial)?["state"].clone()
                    } else {
                        let event: String = conn.query_row("SELECT event_json FROM visual_engine_events WHERE recording_id=?1 AND sequence=?2",params![id,sequence],|row|row.get(0))?;
                        serde_json::from_str::<Value>(&event)?["state"].clone()
                    };
                    if saved["definition"] != state["definition"] {
                        bail!("recording definition mismatch");
                    }
                    validate_state(&saved)?;
                    state = saved;
                    state["replay"]=json!({"recordingId":id,"sequence":sequence,"eventCount":count,"playing":ticking && sequence<count});
                    if replay_cursor["recordingId"]==id {
                        if let Some(interval)=replay_cursor.get("intervalMs") {state["replay"]["intervalMs"]=interval.clone();}
                    }
                    state["stateVersion"] = json!(version + 1);
                    state.as_object_mut().unwrap().remove("scene");
                    conn.execute("UPDATE visual_engine_sessions SET state_json=?4,state_version=?5 WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view,state.to_string(),version+1])?;
                    Ok(json!({"state":state,"sequence":sequence}))
                }
                "record.start" => {
                    if let Some(id) = active {
                        return Ok(json!({"recordingId":id,"state":state}));
                    }
                    let id = uuid::Uuid::new_v4().to_string();
                    let initial = checkpoint(&state);
                    conn.execute(
                        "INSERT INTO visual_engine_recordings VALUES(?1,?2,?3,?4,?5,?6,NULL)",
                        params![
                            id,
                            visual_id,
                            revision,
                            view,
                            initial.to_string(),
                            Utc::now().to_rfc3339()
                        ],
                    )?;
                    conn.execute("UPDATE visual_engine_sessions SET active_recording=?4 WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view,id])?;
                    Ok(json!({"recordingId":id,"state":state}))
                }
                "record.stop" => {
                    if let Some(id) = &active {
                        conn.execute(
                            "UPDATE visual_engine_recordings SET ended_at=?2 WHERE id=?1",
                            params![id, Utc::now().to_rfc3339()],
                        )?;
                    }
                    conn.execute("UPDATE visual_engine_sessions SET active_recording=NULL WHERE visual_id=?1 AND revision=?2 AND view_key=?3",params![visual_id,revision,view])?;
                    Ok(json!({"recordingId":active,"state":state}))
                }
                "checkpoints" | "recordings" => {
                    let offset = request["offset"].as_i64().unwrap_or(0).max(0);
                    let limit = request["limit"].as_i64().unwrap_or(50).clamp(1, 100);
                    let sql = if op == "checkpoints" {
                        "SELECT json_object('id',id,'capturedAt',created_at,'digest',json_extract(checkpoint_json,'$.digest')) FROM visual_engine_checkpoints WHERE visual_id=?1 AND revision=?2 AND view_key=?3 ORDER BY created_at DESC,id LIMIT ?4 OFFSET ?5"
                    } else {
                        "SELECT json_object('id',id,'createdAt',created_at,'endedAt',ended_at,'eventCount',(SELECT COUNT(*) FROM visual_engine_events WHERE recording_id=visual_engine_recordings.id)) FROM visual_engine_recordings WHERE visual_id=?1 AND revision=?2 AND view_key=?3 ORDER BY created_at DESC,id LIMIT ?4 OFFSET ?5"
                    };
                    let mut statement = conn.prepare(sql)?;
                    let rows = statement
                        .query_map(params![visual_id, revision, view, limit, offset], |row| {
                            row.get::<_, String>(0)
                        })?
                        .map(|row| Ok(serde_json::from_str::<Value>(&row?)?))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(json!({"items":rows,"offset":offset,"limit":limit}))
                }
                "record.read" => {
                    let id = field(&request, "recordingId")?;
                    let (initial,ended): (String,Option<String>) = conn.query_row("SELECT initial_json,ended_at FROM visual_engine_recordings WHERE id=?1 AND visual_id=?2 AND revision=?3 AND view_key=?4",params![id,visual_id,revision,view],|row|Ok((row.get(0)?,row.get(1)?)))?;
                    let after = request["after"].as_i64().unwrap_or(0).max(0);
                    let limit = request["limit"].as_i64().unwrap_or(100).clamp(1, 100);
                    let mut statement = conn.prepare("SELECT event_json FROM visual_engine_events WHERE recording_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?;
                    let rows = statement
                        .query_map(params![id, after, limit], |row| row.get::<_, String>(0))?;
                    let mut events = Vec::new();
                    let mut bytes = initial.len();
                    let mut last = after;
                    for row in rows {
                        let raw = row?;
                        if !events.is_empty() && bytes + raw.len() > 4_194_304 {
                            break;
                        }
                        bytes += raw.len();
                        let event: Value = serde_json::from_str(&raw)?;
                        last = event["sequence"]
                            .as_i64()
                            .context("invalid recording sequence")?;
                        events.push(event);
                    }
                    let has_more:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM visual_engine_events WHERE recording_id=?1 AND sequence>?2)",params![id,last],|row|row.get(0))?;
                    let mut recording = json!({"schemaVersion":"synth.visual-session-recording.v1","id":id,"initial":serde_json::from_str::<Value>(&initial)?,"events":events});
                    if let Some(ended) = ended {
                        recording["endedAt"] = json!(ended);
                    }
                    Ok(
                        json!({"recording":recording,"after":after,"limit":limit,"hasMore":has_more}),
                    )
                }
                _ => bail!("unknown visual engine operation {op}"),
            }
        };
        let result = if read_only {
            self.db.run_read(execute).await
        } else {
            self.db.run_transaction(execute).await
        };
        if result.is_ok() && !read_only {
            if let Some(app) = EVENT_HOST.get() {
                let _ = app.emit("visual-engine-changed", signal);
            }
        }
        result
    }
}
