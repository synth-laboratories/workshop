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

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> (tempfile::TempDir, VisualEngine) {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("engine.sqlite")).unwrap());
        db.with_conn(|conn|{
            conn.execute("INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,metadata_json,created_at,updated_at) VALUES('vis',1,'Diagram','diagram.mermaid.v1','draft','mermaid','{}','{}','now','now')",[])?;
            conn.execute("INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,bindings_json,created_at) VALUES('vis',1,'diagram.mermaid.v1','mermaid','{}','now')",[])?;
            Ok(())
        }).unwrap();
        (root, VisualEngine::new(db))
    }
    async fn call(engine: &VisualEngine, operation: &str, fields: Value) -> Result<Value> {
        let mut request = json!({"operation":operation,"revision":1});
        request
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        engine.request("vis".into(), request).await
    }
    async fn attach(engine: &VisualEngine) {
        call(
            engine,
            "attach",
            json!({"definition":{"id":"diagram","version":"1"},"controls":[
            {"id":"step","label":"Step","type":"number","minimum":0,"maximum":100},
            {"id":"source","label":"Source","type":"boolean"}
        ],"defaults":{"step":0,"source":false}}),
        )
        .await
        .unwrap();
    }
    fn action(version: i64, values: Value, id: &str) -> Value {
        json!({"action":{"id":id,"kind":"presentation.patch","expectedStateVersion":version,"payload":{"values":values}}})
    }
    #[test]
    fn canonical_digest_matches_javascript() {
        assert_eq!(
            digest(&json!({"a":1,"z":1e-7})),
            "sha256:31e8dfc1125300b8321f191a16cd8805806503f828ae6d612c4b2dd897c31ed1"
        );
        assert_eq!(digest(&json!({"z":-0.0})), digest(&json!({"z":0})));
    }
    #[test]
    fn discriminated_control_variants_match_sdk_validation() {
        let schema=json!({"id":"filter","type":"object","nullable":true,"oneOf":[
            {"type":"object","required":["kind","name"],"additionalProperties":false,"properties":{"kind":{"type":"string","options":["label"]},"name":{"type":"string"}}},
            {"type":"object","required":["kind","low","high"],"additionalProperties":false,"properties":{"kind":{"type":"string","options":["range"]},"low":{"type":"number"},"high":{"type":"number"}}}
        ]});
        for value in [json!({}),json!({"kind":"range","low":1}),json!({"kind":"label","name":"a","low":1})] {
            assert!(validate_control(&schema,&value).is_err());
        }
        validate_control(&schema,&Value::Null).unwrap();
        validate_control(&schema,&json!({"kind":"range","low":1,"high":2})).unwrap();
    }
    #[tokio::test]
    async fn event_and_relation_queries_match_portable_fixtures(){
        let (_root,engine)=engine();
        let fixture:Value=serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/../../../visuals/tests/fixtures/visual-event-query.json"))).unwrap();
        let corpus=json!({"id":"events","revision":"events-1","schema":"test.v1","count":fixture["rows"].as_array().unwrap().len()});
        call(&engine,"corpus.put",json!({"corpus":corpus,"offset":0,"rows":fixture["rows"]})).await.unwrap();
        for case in fixture["cases"].as_array().unwrap(){
            let answer=call(&engine,"corpus.query",json!({"corpus":corpus,"query":{"schemaVersion":"synth.visuals-core.v1","where":case["where"]}})).await.unwrap();
            let ids:Vec<Value>=answer["rows"].as_array().unwrap().iter().map(|row|row["id"].clone()).collect();
            assert_eq!(json!(ids),case["ids"],"{}",case["where"]);
        }
        assert!(call(&engine,"corpus.aggregate",json!({"corpus":corpus,"field":"metadata","query":{"schemaVersion":"synth.visuals-core.v1","where":{"op":"all"}}})).await.is_err());
    }
    #[tokio::test]
    async fn aggregate_order_matches_portable_fixture() {
        let (_root,engine)=engine();
        let fixture:Value=serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/../../../visuals/tests/fixtures/visual-aggregate-order.json"))).unwrap();
        let corpus=json!({"id":"ordering","revision":"1","schema":"test.v1","count":fixture["rows"].as_array().unwrap().len()});
        call(&engine,"corpus.put",json!({"corpus":corpus,"offset":0,"rows":fixture["rows"]})).await.unwrap();
        let answer=call(&engine,"corpus.aggregate",json!({"corpus":corpus,"field":"value","query":{"schemaVersion":"synth.visuals-core.v1","where":{"op":"all"}}})).await.unwrap();
        let values:Vec<Value>=answer["buckets"].as_array().unwrap().iter().map(|b|b.get("value").cloned().unwrap_or(json!({"missing":true}))).collect();
        assert_eq!(json!(values),fixture["values"]);
    }
    #[tokio::test]
    async fn evidence_cuts_survive_restart_and_checkpoint_restore(){
        let (root,engine)=engine();attach(&engine).await;
        call(&engine,"attach",json!({"definition":{"id":"diagram","version":"1"},"controls":[{"id":"source.test","label":"source.test","type":"string","nullable":true}],"defaults":{"source.test":null}})).await.unwrap();
        let old=call(&engine,"evidence.put",json!({"value":{"sequence":1,"observed":"old"}})).await.unwrap();
        call(&engine,"act",action(0,json!({"source.test":old["digest"]}),"pin-old")).await.unwrap();
        let saved=call(&engine,"capture",json!({"expectedStateVersion":1})).await.unwrap();
        let new=call(&engine,"evidence.put",json!({"value":{"sequence":2,"observed":"new"}})).await.unwrap();
        call(&engine,"act",action(1,json!({"source.test":new["digest"]}),"pin-new")).await.unwrap();
        drop(engine);
        let engine=VisualEngine::new(Arc::new(Database::open(root.path().join("engine.sqlite")).unwrap()));
        let restored=call(&engine,"restore",json!({"checkpointId":saved["checkpoint"]["id"],"expectedStateVersion":2})).await.unwrap();
        assert_eq!(restored["state"]["values"]["source.test"],old["digest"]);
        let value=call(&engine,"evidence.read",json!({"digest":old["digest"]})).await.unwrap();
        assert_eq!(value["value"]["observed"],"old");
        assert!(call(&engine,"evidence.read",json!({"digest":"missing"})).await.is_err());
    }
    #[tokio::test]
    async fn playback_ticks_are_single_clock_and_pause_invalidates_queued_ticks() {
        let (_root, engine) = engine();
        attach(&engine).await;
        call(&engine,"attach",json!({"definition":{"id":"diagram","version":"1"},
            "controls":[{"id":"playing","label":"Playing","type":"boolean"}],"defaults":{"playing":true}})).await.unwrap();
        let tick = |version, step, id| json!({"clock":"steps","playingControl":"playing","intervalMs":60_000,
            "action":{"id":id,"kind":"presentation.patch","expectedStateVersion":version,"payload":{"values":{"step":step}}}});
        let (a,b)=tokio::join!(call(&engine,"playback.tick",tick(0,1,"a")),call(&engine,"playback.tick",tick(0,1,"b")));
        let a=a.unwrap();let b=b.unwrap();
        assert_ne!(a["tickApplied"],b["tickApplied"]);
        assert_eq!(call(&engine,"inspect",json!({})).await.unwrap()["state"]["stateVersion"],1);
        // A different clock cannot use a stale projection to overwrite an action.
        let mut stale=tick(0,2,"stale");stale["clock"]=json!("other");
        assert_eq!(call(&engine,"playback.tick",stale).await.unwrap()["tickApplied"],false);
        call(&engine,"act",action(1,json!({"playing":false}),"pause")).await.unwrap();
        let mut paused=tick(2,3,"paused");paused["clock"]=json!("third");
        let response=call(&engine,"playback.tick",paused).await.unwrap();
        assert_eq!(response["tickApplied"],false);
        assert_eq!(response["state"]["values"]["step"],1);
    }
    #[tokio::test]
    async fn capture_barrier_blocks_commits_and_releases_exact_checkpoint() {
        let (_root,engine)=engine();attach(&engine).await;
        let frozen=call(&engine,"capture.freeze",json!({})).await.unwrap();
        assert!(call(&engine,"act",action(0,json!({"step":4}),"during")).await.is_err());
        assert!(call(&engine,"capture.freeze",json!({})).await.is_err());
        assert!(call(&engine,"capture.release",json!({"token":"wrong"})).await.is_err());
        assert_eq!(call(&engine,"inspect",json!({})).await.unwrap()["state"],frozen["checkpoint"]["state"]);
        let released=call(&engine,"capture.release",json!({"token":frozen["token"]})).await.unwrap();
        assert_eq!(released["checkpoint"],frozen["checkpoint"]);
        call(&engine,"act",action(0,json!({"step":4}),"after")).await.unwrap();
    }
    #[tokio::test]
    async fn restored_playing_state_is_inert_until_explicit_interaction() {
        let (_root,engine)=engine();attach(&engine).await;
        call(&engine,"attach",json!({"definition":{"id":"diagram","version":"1"},
            "controls":[{"id":"playing","label":"Playing","type":"boolean"}],"defaults":{"playing":true}})).await.unwrap();
        let saved=call(&engine,"capture",json!({"expectedStateVersion":0})).await.unwrap();
        let restored=call(&engine,"restore",json!({"checkpointId":saved["checkpoint"]["id"],"expectedStateVersion":0})).await.unwrap();
        assert_eq!(restored["state"]["replay"]["checkpointId"],saved["checkpoint"]["id"]);
        let tick=json!({"clock":"restore-test","playingControl":"playing","intervalMs":16,
            "action":{"id":"tick","kind":"presentation.patch","expectedStateVersion":1,"payload":{"values":{"step":1}}}});
        assert_eq!(call(&engine,"playback.tick",tick).await.unwrap()["tickApplied"],false);
        let resumed=call(&engine,"act",action(1,json!({"step":2}),"human")).await.unwrap();
        assert!(resumed["state"].get("replay").is_none());
    }
    #[tokio::test]
    async fn recording_playback_is_shared_and_single_clock() {
        let (_root,engine)=engine();attach(&engine).await;
        let recording=call(&engine,"record.start",json!({})).await.unwrap();
        call(&engine,"act",action(0,json!({"step":1}),"first")).await.unwrap();
        call(&engine,"act",action(1,json!({"step":2}),"second")).await.unwrap();
        call(&engine,"record.stop",json!({})).await.unwrap();
        let restored=call(&engine,"record.seek",json!({"recordingId":recording["recordingId"],"sequence":0,"expectedStateVersion":2})).await.unwrap();
        assert_eq!(restored["state"]["replay"]["eventCount"],2);
        call(&engine,"record.play",json!({"playing":true,"intervalMs":60_000,"expectedStateVersion":3})).await.unwrap();
        let wrong_speed=call(&engine,"record.tick",json!({"clock":"recording","intervalMs":16,"expectedStateVersion":4})).await.unwrap();
        assert_eq!(wrong_speed["tickApplied"],false);
        let tick=json!({"clock":"recording","intervalMs":60_000,"expectedStateVersion":4});
        let (a,b)=tokio::join!(call(&engine,"record.tick",tick.clone()),call(&engine,"record.tick",tick));
        assert_ne!(a.unwrap()["tickApplied"],b.unwrap()["tickApplied"]);
        let inspected=call(&engine,"inspect",json!({})).await.unwrap();
        assert_eq!(inspected["state"]["replay"]["sequence"],1);
        assert_eq!(inspected["state"]["values"]["step"],1);
        assert_eq!(inspected["state"]["replay"]["intervalMs"],60_000);
        call(&engine,"record.play",json!({"playing":false,"expectedStateVersion":5})).await.unwrap();
        let paused=call(&engine,"record.tick",json!({"clock":"other","intervalMs":16,"expectedStateVersion":6})).await.unwrap();
        assert_eq!(paused["tickApplied"],false);
    }
    #[tokio::test]
    async fn corpus_queries_are_pinned_bounded_and_type_safe() {
        let (_root, engine) = engine();
        let corpus = json!({"id":"rows","revision":"cut-1","schema":"test.v1","count":4});
        let query = json!({"schemaVersion":"synth.visuals-core.v1","where":{"op":"all"}});
        let rows = json!([
            {"id":"a","reward":-2,"failed":true,"tags":["x","x"],"optional":null},
            {"id":"b","reward":0,"failed":false,"tags":["x","y"]},
            {"id":"c","reward":1,"failed":false,"tags":[]},
            {"id":"d","reward":9,"failed":true,"tags":["y"]}
        ]);
        call(
            &engine,
            "corpus.put",
            json!({"corpus":corpus,"offset":0,"rows":[rows[0]]}),
        )
        .await
        .unwrap();
        assert!(call(
            &engine,
            "corpus.query",
            json!({"corpus":corpus,"query":query})
        )
        .await
        .is_err());
        call(
            &engine,
            "corpus.put",
            json!({"corpus":corpus,"offset":1,"rows":[rows[1],rows[2],rows[3]]}),
        )
        .await
        .unwrap();
        let bounded = call(
            &engine,
            "corpus.query",
            json!({"corpus":corpus,"query":query,"window":{"limit":1}}),
        )
        .await
        .unwrap();
        assert_eq!(bounded["total"], 4);
        assert_eq!(bounded["rows"].as_array().unwrap().len(), 1);
        for (predicate, expected) in [
            (json!({"op":"eq","field":"optional","value":null}), 1),
            (json!({"op":"exists","field":"optional","exists":false}), 3),
            (json!({"op":"eq","field":"failed","value":1}), 0),
            (json!({"op":"contains","field":"tags","value":"x"}), 2),
            (
                json!({"op":"not","expression":{"op":"gt","field":"optional","value":1}}),
                4,
            ),
        ] {
            let answer=call(&engine,"corpus.query",json!({"corpus":corpus,"query":{"schemaVersion":"synth.visuals-core.v1","where":predicate}})).await.unwrap();
            assert_eq!(answer["total"], expected, "{predicate}");
        }
        let aggregate = call(
            &engine,
            "corpus.aggregate",
            json!({"corpus":corpus,"query":query,"field":"tags","cohortId":"all"}),
        )
        .await
        .unwrap();
        assert_eq!(aggregate["denominator"], 4);
        assert_eq!(aggregate["buckets"][0]["count"], 2);
        assert_eq!(aggregate["buckets"].as_array().unwrap().len(), 2);
        for strategy in [
            "random",
            "diverse",
            "boundary",
            "outlier",
            "representative",
            "failure",
        ] {
            let answer=call(&engine,"corpus.sample",json!({"corpus":corpus,"query":query,"strategy":strategy,"count":2,"options":{"seed":7}})).await.unwrap();
            assert_eq!(answer["sourceCount"], 4);
            assert_eq!(answer["rows"].as_array().unwrap().len(), 2, "{strategy}");
            if strategy == "boundary" {
                assert_eq!(answer["rows"][0]["id"], "b");
            }
            if strategy == "outlier" {
                assert_eq!(answer["rows"][0]["id"], "d");
            }
        }
        assert!(call(
            &engine,
            "corpus.put",
            json!({"corpus":corpus,"offset":0,"rows":[{"id":"a","reward":100}]})
        )
        .await
        .is_err());
        let mut wrong = corpus.clone();
        wrong["count"] = json!(3);
        assert!(call(
            &engine,
            "corpus.query",
            json!({"corpus":wrong,"query":query})
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn commits_are_atomic_and_retries_do_not_duplicate_recording_events() {
        let (_root, engine) = engine();
        attach(&engine).await;
        let recording = call(&engine, "record.start", json!({})).await.unwrap();
        let command = action(0, json!({"step":8,"source":true}), "first");
        let first = call(&engine, "act", command.clone()).await.unwrap();
        assert_eq!(first["state"]["stateVersion"], 1);
        assert_eq!(
            call(&engine, "act", command).await.unwrap()["duplicate"],
            true
        );
        assert!(call(&engine, "act", action(0, json!({"step":9}), "stale"))
            .await
            .is_err());
        assert!(call(
            &engine,
            "act",
            action(1, json!({"source":false,"step":101}), "invalid")
        )
        .await
        .is_err());
        let state = call(&engine, "inspect", json!({})).await.unwrap();
        assert_eq!(state["state"]["values"], json!({"step":8,"source":true}));
        let replay = call(
            &engine,
            "record.read",
            json!({"recordingId":recording["recordingId"]}),
        )
        .await
        .unwrap();
        assert_eq!(replay["recording"]["events"].as_array().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn imported_checkpoints_validate_structure_without_mutating_live_state() {
        let (_root, engine) = engine();
        attach(&engine).await;
        call(&engine,"attach",json!({"definition":{"id":"diagram","version":"1"},"controls":[{"id":"viewport","label":"Viewport","type":"object","required":["scale"],"additionalProperties":false,"properties":{"scale":{"type":"number","minimum":0.25,"maximum":4}}}],"defaults":{"viewport":{"scale":1}}})).await.unwrap();
        assert!(
            call(&engine, "act", action(0, json!({"viewport":{}}), "missing"))
                .await
                .is_err()
        );
        assert!(call(
            &engine,
            "act",
            action(0, json!({"viewport":{"scale":100}}), "range")
        )
        .await
        .is_err());
        let saved = call(&engine, "capture", json!({"expectedStateVersion":0}))
            .await
            .unwrap()["checkpoint"]
            .clone();
        let imported = call(&engine, "checkpoint.import", json!({"checkpoint":saved}))
            .await
            .unwrap();
        assert_ne!(imported["checkpoint"]["id"], saved["id"]);
        assert_eq!(
            call(&engine, "inspect", json!({})).await.unwrap()["state"]["stateVersion"],
            0
        );
        let mut corrupt = saved.clone();
        corrupt["state"]["values"]["step"] = json!(3);
        assert!(
            call(&engine, "checkpoint.import", json!({"checkpoint":corrupt}))
                .await
                .is_err()
        );
        let listed = call(&engine, "checkpoints", json!({})).await.unwrap();
        assert!(listed["items"][0].get("state").is_none());
        let read = call(
            &engine,
            "checkpoint.read",
            json!({"checkpointId":imported["checkpoint"]["id"]}),
        )
        .await
        .unwrap();
        assert_eq!(read["checkpoint"]["state"], saved["state"]);
    }
    #[tokio::test]
    async fn recording_import_checks_transitions_and_never_executes_commands() {
        let (_root, engine) = engine();
        attach(&engine).await;
        let started = call(&engine, "record.start", json!({})).await.unwrap();
        call(&engine, "act", action(0, json!({"step":3}), "first"))
            .await
            .unwrap();
        call(&engine, "record.stop", json!({})).await.unwrap();
        let recording = call(
            &engine,
            "record.read",
            json!({"recordingId":started["recordingId"]}),
        )
        .await
        .unwrap()["recording"]
            .clone();
        let imported = call(&engine, "record.import", json!({"recording":recording}))
            .await
            .unwrap();
        assert_eq!(imported["eventCount"], 1);
        assert_eq!(
            call(&engine, "inspect", json!({})).await.unwrap()["state"]["stateVersion"],
            1
        );
        let mut invalid = recording.clone();
        invalid["events"][0]["state"]["values"]["step"] = json!(7);
        assert!(call(&engine, "record.import", json!({"recording":invalid}))
            .await
            .is_err());
        call(
            &engine,
            "record.seek",
            json!({"recordingId":imported["recordingId"],"sequence":0,"expectedStateVersion":1}),
        )
        .await
        .unwrap();
        assert_eq!(
            call(&engine, "inspect", json!({})).await.unwrap()["state"]["values"]["step"],
            0
        );
    }
    #[tokio::test]
    #[ignore = "explicit corpus scale gate; run with --ignored --nocapture"]
    async fn corpus_scale_gate() {
        let (_root, engine) = engine();
        for count in [1000, 10000, 100000] {
            let corpus = json!({"id":"scale","revision":format!("cut-{count}"),"schema":"scale.v1","count":count});
            let started = std::time::Instant::now();
            for offset in (0..count).step_by(500) {
                let rows=(offset..std::cmp::min(offset+500,count)).map(|index|json!({"id":format!("row-{index:06}"),"reward":(index%100) as f64/100.0,"group":index%4,"failed":index%7==0})).collect::<Vec<_>>();
                call(
                    &engine,
                    "corpus.put",
                    json!({"corpus":corpus,"offset":offset,"rows":rows}),
                )
                .await
                .unwrap();
            }
            let hydrated = started.elapsed();
            let query = json!({"schemaVersion":"synth.visuals-core.v1","where":{"op":"eq","field":"group","value":1}});
            let queried = std::time::Instant::now();
            let result = call(
                &engine,
                "corpus.query",
                json!({"corpus":corpus,"query":query,"window":{"limit":8}}),
            )
            .await
            .unwrap();
            assert_eq!(result["total"], count / 4);
            assert_eq!(result["rows"].as_array().unwrap().len(), 8);
            let aggregate = call(
                &engine,
                "corpus.aggregate",
                json!({"corpus":corpus,"query":query,"field":"failed","cohortId":"quarter"}),
            )
            .await
            .unwrap();
            assert_eq!(aggregate["denominator"], count / 4);
            let sample = call(
                &engine,
                "corpus.sample",
                json!({"corpus":corpus,"query":query,"strategy":"representative","count":8}),
            )
            .await
            .unwrap();
            assert_eq!(sample["rows"].as_array().unwrap().len(), 8);
            eprintln!(
                "visual corpus {count}: hydrate={hydrated:?}, query+aggregate+sample={:?}",
                queried.elapsed()
            );
        }
    }
    #[tokio::test]
    async fn restart_restores_checkpoint_and_acknowledged_recording() {
        let (root, engine) = engine();
        attach(&engine).await;
        let saved = call(&engine, "capture", json!({"expectedStateVersion":0}))
            .await
            .unwrap();
        let recording = call(&engine, "record.start", json!({})).await.unwrap();
        call(&engine, "act", action(0, json!({"step":9}), "first"))
            .await
            .unwrap();
        drop(engine);
        let restarted = VisualEngine::new(Arc::new(
            Database::open(root.path().join("engine.sqlite")).unwrap(),
        ));
        assert_eq!(
            call(&restarted, "inspect", json!({})).await.unwrap()["activeRecording"],
            recording["recordingId"]
        );
        call(&restarted, "record.stop", json!({})).await.unwrap();
        let restored = call(
            &restarted,
            "restore",
            json!({"checkpointId":saved["checkpoint"]["id"],"expectedStateVersion":1}),
        )
        .await
        .unwrap();
        assert_eq!(restored["state"]["values"]["step"], 0);
        let replayed = call(
            &restarted,
            "record.seek",
            json!({"recordingId":recording["recordingId"],"sequence":1,"expectedStateVersion":2}),
        )
        .await
        .unwrap();
        assert_eq!(replayed["state"]["values"]["step"], 9);
        assert!(call(
            &restarted,
            "record.seek",
            json!({"recordingId":recording["recordingId"],"sequence":2,"expectedStateVersion":3})
        )
        .await
        .is_err());
    }
}

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
