//! Resolve existing optimizer membership; Containers owns V5 query semantics.
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, process::Stdio};
pub const SCHEMA: &str = "synth.trace-query.v2";
fn parse(s: String) -> Result<Value> {
    Ok(serde_json::from_str(&s)?)
}
fn first(v: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|k| v.get(*k).filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null)
}
pub fn job_ids(q: &Value) -> Result<Vec<String>> {
    if q["schemaVersion"] != SCHEMA {
        bail!("unsupported query schema");
    }
    let a = q["evalJobIds"].as_array().context("evalJobIds required")?;
    if a.is_empty() || a.len() > 100 {
        bail!("select 1..100 existing jobs");
    }
    a.iter()
        .map(|x| {
            x.as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .context("invalid job ID")
        })
        .collect()
}
fn refs(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Array(a) => {
            for x in a {
                refs(x, out)
            }
        }
        Value::Object(o) => {
            if o.get("kind")
                .and_then(Value::as_str)
                .is_some_and(|s| s.contains("trace"))
            {
                for k in ["id", "digest"] {
                    if let Some(s) = o.get(k).and_then(Value::as_str) {
                        out.insert(s.into());
                    }
                }
            }
            for (k, x) in o {
                if ["traceRef", "trace_ref", "traceDigest", "trace_digest"].contains(&k.as_str()) {
                    if let Some(s) = x.as_str() {
                        out.insert(s.into());
                    } else {
                        refs(x, out);
                    }
                } else if ["refs", "artifactRefs", "artifact_refs", "trial", "evidence"]
                    .contains(&k.as_str())
                {
                    refs(x, out);
                }
            }
        }
        _ => {}
    }
}
fn attach_trace(c: &Connection, row: &mut Value, details: &Value) -> Result<()> {
    let mut candidates = BTreeSet::new();
    refs(details, &mut candidates);
    let mut found = BTreeSet::new();
    if let Some(td) = row["traceDigest"].as_str() {
        found.insert(td.to_string());
    }
    for r in candidates {
        let item=c.query_row("SELECT id,digest,path,metadata_json,container_id FROM traces WHERE id=?1 OR digest=?1",[r],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).optional()?;
        if let Some((id, digest, path, metadata, container)) = item {
            found.insert(digest.clone());
            if found.len() > 1 {
                bail!("trial has multiple sealed trace authorities; explicit selection required");
            }
            let m = parse(metadata)?;
            row["localTraceId"] = json!(id);
            row["traceDigest"] = json!(digest);
            row["archivePath"] = json!(path);
            row["containerId"] = json!(container);
            for (a, b) in [
                ("model", "model"),
                ("environment", "benchmark"),
                ("taskId", "taskId"),
                ("seed", "seed"),
                ("captureStatus", "captureStatus"),
            ] {
                if row[a].is_null() {
                    row[a] = m.get(b).cloned().unwrap_or(Value::Null);
                }
            }
        }
    }
    Ok(())
}
/// Called in a single DB snapshot; no trace parsing or provider work in the transaction.
pub fn resolve_inputs(c: &Connection, q: &Value) -> Result<Value> {
    let requested = job_ids(q)?;
    let mut ids: BTreeSet<String> = requested.iter().cloned().collect();
    if q["includeChildEvals"].as_bool() == Some(true) {
        let mut queue = requested.clone();
        while let Some(id) = queue.pop() {
            let mut s=c.prepare("SELECT details_json FROM optimizer_run_collection_rows WHERE optimizer_run_id=?1 AND kind='go_ex_child_eval'")?;
            for x in s.query_map([id], |r| r.get::<_, String>(0))? {
                let v = parse(x?)?;
                if let Some(child) = first(
                    &v,
                    &["optimizerRunId", "runId", "run_id", "childEvalRunId", "id"],
                )
                .as_str()
                {
                    if ids.insert(child.into()) {
                        queue.push(child.into());
                    }
                }
            }
            if ids.len() > 1000 {
                bail!("too many child evals");
            }
        }
    }
    let mut episodes = vec![];
    for id in ids {
        let registered = c
            .query_row(
                "SELECT algorithm_id,payload_json FROM optimizer_runs WHERE id=?1",
                [&id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((algorithm, payload)) = registered else {
            let mut statement = c.prepare("SELECT id,digest,path,metadata_json,container_id,reward FROM traces WHERE run_id=?1 OR json_extract(metadata_json,'$.runId')=?1 ORDER BY digest")?;
            let mut found = false;
            for item in statement.query_map([&id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<f64>>(5)?)))? {
                let (local,digest,path,metadata,container,reward) = item?;
                let m = parse(metadata)?;
                episodes.push(json!({"jobId":id,"trialId":m.get("trialId").filter(|v| !v.is_null()).or_else(||m.get("episodeId")).cloned().unwrap_or(json!(local)),"localTraceId":local,"traceDigest":digest,"archivePath":path,"containerId":container,"reward":reward,"model":m.get("model"),"effort":m.get("effort"),"environment":m.get("benchmark"),"taskId":m.get("taskId"),"seed":m.get("seed"),"captureStatus":m.get("captureStatus"),"status":m.get("lifecycleStatus"),"sourceRevision":digest,"includedChild":!requested.contains(&id)}));
                found = true;
            }
            if !found { bail!("unknown optimizer/eval job: {id}; import its sealed traces first"); }
            continue;
        };
        let spec = parse(
            c.query_row(
                "SELECT spec_json FROM optimizer_run_specs WHERE optimizer_run_id=?1",
                [&id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or(payload),
        )?;
        let config = spec
            .get("recipe")
            .or_else(|| spec.get("config"))
            .or_else(|| spec.get("parameters"))
            .unwrap_or(&spec);
        let mut s=c.prepare("SELECT item_id,kind,details_json,score,status,revision FROM optimizer_run_collection_rows WHERE optimizer_run_id=?1 AND collection='evaluations' ORDER BY ordinal,item_id")?;
        for x in s.query_map([&id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })? {
            let (trial, kind, details, reward, status, revision) = x?;
            if kind.contains("scorecard") || kind == "go_ex_child_eval" {
                continue;
            }
            let d = parse(details)?;
            let research = d
                .get("researchContext")
                .or_else(|| d.get("research_context"))
                .unwrap_or(&d);
            let mut row = json!({"jobId":id,"trialId":trial,"algorithm":algorithm,"candidateId":first(&d,&["candidateId","candidate_id"]),"checkpointId":first(&d,&["checkpointId","checkpoint_id"]),"stage":d.get("stage"),"seed":d.get("seed"),"scenario":d.get("scenario"),"taskId":first(&d,&["taskId","task_id","exampleId","example_id"]),"repeat":first(&d,&["repeat","repeatIndex"]),"status":status,"valid":d.get("valid"),"reward":reward,"sourceRevision":revision,"includedChild":!requested.contains(&id)});
            for f in [
                "environment",
                "environmentVersion",
                "promptRevision",
                "protocolRevision",
                "harnessRevision",
                "model",
                "effort",
                "rewardId",
                "units",
            ] {
                row[f] = research
                    .get(f)
                    .or_else(|| config.get(f))
                    .cloned()
                    .unwrap_or(Value::Null);
            }
            for field in [
                "taskId",
                "seed",
                "repeat",
                "candidateId",
                "checkpointId",
                "rewardVersion",
                "definitionDigest",
            ] {
                if let Some(value) = research.get(field).filter(|v| field == "seed" || !v.is_null()) {
                    row[field] = value.clone();
                }
            }
            if row["model"].is_object() {
                row["model"] = row["model"]["modelId"].clone();
            }
            if row["model"].is_null() {
                row["model"] = config
                    .pointer("/model/modelId")
                    .cloned()
                    .unwrap_or(Value::Null);
            }
            if row["effort"].is_null() {
                row["effort"] = config
                    .pointer("/policy/configuration/reasoning_effort")
                    .or_else(|| config.pointer("/policy/configuration/effort"))
                    .cloned()
                    .unwrap_or(Value::Null);
            }
            let extra=c.query_row("SELECT details_json FROM optimizer_run_collection_rows WHERE optimizer_run_id=?1 AND collection='rollouts' AND item_id=?2",rusqlite::params![id,trial],|r|r.get::<_,String>(0)).optional()?;
            attach_trace(c, &mut row, &d)?;
            if let Some(extra) = extra {
                attach_trace(c, &mut row, &parse(extra)?)?;
            }
            // Explicit absence in producer context outranks legacy archive metadata.
            if research.get("seed").is_some_and(Value::is_null) { row["seed"] = Value::Null; }
            episodes.push(row);
        }
    }
    let mut annotations = vec![];
    let digests: BTreeSet<String> = episodes
        .iter()
        .filter_map(|r| r["traceDigest"].as_str().map(str::to_string))
        .collect();
    for td in digests {
        let mut s=c.prepare("SELECT f.finding_id,f.evidence_head_digest,f.annotator_id,f.taxonomy_label,f.status,f.target_selector_json,f.evidence_selectors_json,f.payload_json,(SELECT decision FROM annotation_reviews r WHERE r.finding_id=f.finding_id AND r.evidence_head_digest=f.evidence_head_digest ORDER BY r.created_at DESC,r.review_id DESC LIMIT 1) FROM annotation_findings f JOIN annotation_evidence_heads h ON h.digest=f.evidence_head_digest WHERE h.trace_digest=?1 ORDER BY f.finding_id")?;
        for x in s.query_map([&td], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })? {
            let (a, h, def, label, status, target, evidence, payload, review) = x?;
            let payload = parse(payload)?;
            let canonical = payload.get("sourceAnnotation").unwrap_or(&Value::Null);
            annotations.push(json!({"itemId":a,"annotationId":a,"traceDigest":td,"evidenceDigest":h,"annotatorId":def,
                "annotatorVersion":canonical.get("annotator_version"),"label":canonical.get("labels").cloned().unwrap_or(json!(label.into_iter().collect::<Vec<_>>())),
                "annotationState":status,"selector":parse(target)?,"evidence":parse(evidence)?,"score":payload.get("score"),
                "confidence":canonical.get("confidence"),"supersedesId":canonical.get("supersedes_id"),
                "reviewState":review.map(Value::String).unwrap_or_else(||canonical.get("review_state").cloned().unwrap_or(Value::Null)),"payload":payload,"current":true}));
        }
    }
    let superseded: BTreeSet<String> = annotations
        .iter()
        .filter_map(|a| a["supersedesId"].as_str().map(str::to_owned))
        .collect();
    for a in &mut annotations {
        a["current"] = json!(!a["annotationId"]
            .as_str()
            .is_some_and(|id| superseded.contains(id)));
    }
    for row in &mut episodes {
        if let Some(td) = row["traceDigest"].as_str() {
            let mut stmt=c.prepare("SELECT state,applied_count FROM annotation_jobs WHERE trace_digest=?1 ORDER BY updated_at,job_id")?;
            let jobs = stmt
                .query_map([td], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            row["analysisJobs"] = json!(jobs);
            row["analysisState"] = json!(if jobs.is_empty() {
                "not_requested"
            } else if jobs
                .iter()
                .any(|(s, _)| ["running", "prepared", "submitted"].contains(&s.as_str()))
            {
                "running"
            } else if jobs.iter().any(|(s, _)| s == "failed") {
                "failed"
            } else if jobs.iter().any(|(s, _)| s == "cancelled") {
                "cancelled"
            } else if jobs.iter().any(|(s, _)| s == "abstained") {
                "abstained"
            } else if jobs.iter().all(|(s, n)| s == "sealed" && *n == Some(0)) {
                "no_findings"
            } else {
                "completed"
            });
        }
    }
    Ok(json!({"query":q,"episodes":episodes,"annotations":annotations}))
}
pub async fn execute(input: Value, root: &Path) -> Result<crate::trace_query::QuerySnapshot> {
    Ok(serde_json::from_value(execute_value(input, root).await?)?)
}
pub async fn execute_value(input: Value, root: &Path) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let dir = root.join(format!("request-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&dir)?;
    let result = async {
        let request = dir.join("request.json");
        let output = dir.join("result.json");
        std::fs::write(&request, serde_json::to_vec(&input)?)?;
        let cli = crate::trace_ingest::resolve_trace_cli()?;
        let process = tokio::process::Command::new(cli)
            .arg("research-query")
            .arg("--request")
            .arg(request)
            .arg("--cache")
            .arg(root.join("catalog.sqlite"))
            .arg("--output")
            .arg(&output)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        let r = tokio::time::timeout(std::time::Duration::from_secs(120), process)
            .await
            .context("research query timed out")??;
        if !r.status.success() {
            bail!(
                "research query failed: {}",
                String::from_utf8_lossy(&r.stderr)
            );
        }
        if std::fs::metadata(&output)?.len() > 64 * 1024 * 1024 {
            bail!("snapshot exceeds 64 MiB; narrow the query");
        }
        Ok(serde_json::from_slice(&std::fs::read(output)?)?)
    }
    .await;
    let _ = std::fs::remove_dir_all(dir);
    result
}
pub fn page(s: &crate::trace_query::QuerySnapshot, offset: usize, limit: usize) -> Result<Value> {
    if !(1..=200).contains(&limit) {
        bail!("page limit must be 1..200");
    }
    let rows = s.facets["rows"]
        .as_array()
        .context("snapshot omitted rows")?;
    if offset > rows.len() {
        bail!("offset exceeds result count");
    }
    let end = offset.saturating_add(limit).min(rows.len());
    Ok(
        json!({"schemaVersion":s.schema_version,"snapshotId":s.snapshot_id,"resultDigest":s.result_digest,"resultCount":s.result_count,"rows":&rows[offset..end],"resultIds":&s.result_ids[offset..end],"offset":offset,"nextOffset":if end<rows.len(){Some(end)}else{None},"truncated":s.truncated}),
    )
}


pub fn annotation_selection(
    snapshot: &crate::trace_query::QuerySnapshot,
    ids: &[String],
) -> Result<Value> {
    use std::collections::{BTreeMap, BTreeSet};
    if snapshot.query_schema_version != crate::trace_research::SCHEMA {
        bail!("Annotation selection requires a v2 research snapshot");
    }
    if ids.is_empty() || ids.len() > 200 {
        bail!("select 1..200 explicit result IDs per preparation");
    }
    let rows = snapshot.facets["rows"]
        .as_array()
        .context("snapshot has no rows")?;
    let mut targets: BTreeMap<String, Value> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let index = snapshot
            .result_ids
            .iter()
            .position(|x| x == id)
            .context("result ID does not belong to this snapshot")?;
        let row = rows.get(index).context("snapshot row missing")?;
        let trace = row["traceDigest"]
            .as_str()
            .context("select trace rows, not aggregate rows")?;
        if row["traceAvailability"] != "available" {
            bail!("selected trace is unavailable");
        }
        row["traceId"]
            .as_str()
            .context("selected trace has no canonical trace ID")?;
        let container = row["containerId"]
            .as_str()
            .context("selected trace has no source container")?;
        let key = format!("{container}:{trace}");
        let target=targets.entry(key).or_insert_with(||json!({"container_id":container,"trace_id":row["traceId"],"trace_digest":trace,"resultIds":[],"selectors":[]}));
        target["resultIds"].as_array_mut().unwrap().push(json!(id));
        if !row["selector"].is_null() {
            target["selectors"]
                .as_array_mut()
                .unwrap()
                .push(row["selector"].clone());
        }
    }
    Ok(
        json!({"schemaVersion":"synth.annotation-selection.v1","snapshotId":snapshot.snapshot_id,"resultDigest":snapshot.result_digest,"targets":targets.into_values().collect::<Vec<_>>(),"startsCompute":false,"nextOperation":"annotation_list_definitions"}),
    )
}

