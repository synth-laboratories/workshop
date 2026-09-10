//! Freeze live and Trace V5 bindings into self-contained, provenance-bearing evidence.
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use super::artifacts::hex_sha256;

struct SealEvidence<'a> {
    visual_id: &'a str,
    revision: i64,
    content: &'a crate::storage::ContentStore,
    traces: BTreeMap<TraceRequest, ResolvedTraceEvidence>,
}

/// The trace projection one `trace_v5` descriptor names: the sealed archive
/// digest it points at, and the consumer projection it wants derived from it.
type TraceRequest = (String, String);

/// A Trace V5 projection document the seal froze into a binding, and the
/// provenance a verifier reads to know which archive it came from.
struct ResolvedTraceEvidence {
    /// The projection payload, verbatim. It is self-describing — it carries
    /// its own `schema_version` — which is what lets
    /// [`locate_sealed_projections`] name it as a view without a second
    /// registry of where projections live.
    payload: Value,
    /// The `sha256:`-qualified digest of the sealed archive it was derived
    /// from, as the resolver normalised it.
    trace_digest: String,
    /// The format the archive's own manifest declared.
    projection_schema: String,
    /// The digest of the projection payload, as the trace tooling computed it.
    /// A verifier re-deriving the projection compares this, not the bindings.
    payload_digest: String,
}

/// Replayable evidence for one live binding, and where the seal found it.
struct ResolvedEvidence {
    /// The evidence bodies. Empty for the opaque descriptor snapshot below,
    /// which is not an envelope log and cannot be projected.
    envelopes: Vec<Value>,
    /// The verbatim value to freeze into the binding's `data`.
    data: Value,
    /// `descriptor` (an inline `snapshot`), `spool` (a CAS digest named on the
    /// binding) or `host_observation` (what Desktop polled). Recorded on the
    /// binding so a verifier reads how the evidence was obtained rather than
    /// inferring it.
    origin: &'static str,
    spool_digest: Option<String>,
    truncated: bool,
}

/// The identity a declared live stream is recorded and resolved under.
///
/// The same rule `stream_receipt::declared_streams` applies — declared
/// `source`, falling back to the poll URL — so the evidence the host recorded
/// while polling and the evidence the seal asks for are the same key by
/// construction, not by two functions agreeing.
fn binding_stream_id(object: &Map<String, Value>) -> Option<String> {
    for key in ["source", "poll_url", "pollUrl"] {
        if let Some(value) = object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

/// The archive digest and projection kind one `trace_v5` descriptor names.
///
/// `projection` is the key a chart panel writes; `schema` is the key the trace
/// pane stamps when it creates the inspector visual. Both name the same thing —
/// which consumer projection to derive — so both are read here rather than one
/// being privileged and the other silently defaulted. The strip mirrors
/// `data.rs::projection_consumer_kind`, which does the same in the other
/// direction for the cache key.
fn trace_binding_request(object: &Map<String, Value>) -> Option<TraceRequest> {
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let kind = object
        .get("projection")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            object
                .get("schema")
                .and_then(Value::as_str)
                .and_then(|schema| schema.strip_prefix("synth.trace-projection."))
                .and_then(|rest| rest.strip_suffix(".v1"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| super::registry::CHART_DEFAULT_PROJECTION.to_string());
    Some((source.to_string(), kind))
}

/// Every distinct Trace V5 projection a bindings tree asks for.
///
/// Two descriptors naming the same archive and the same projection resolve
/// once; two naming different projections of one archive resolve twice, which
/// is what the key being a pair buys.
fn trace_binding_requests(bindings: &Value) -> Vec<TraceRequest> {
    fn walk(value: &Value, out: &mut Vec<TraceRequest>) {
        match value {
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) == Some("trace_v5") {
                    if let Some(request) = trace_binding_request(object) {
                        if !out.contains(&request) {
                            out.push(request);
                        }
                    }
                }
                for child in object.values() {
                    walk(child, out);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(bindings, &mut out);
    out
}

/// Freeze one `trace_v5` binding into the projection it names.
///
/// The defect this closes is the live-eval one in the other major visual
/// class. A `trace.rollout_inspector.v1` seal carried `{kind: "trace_v5",
/// source: <digest>}` verbatim: a pointer into a content-addressed store the
/// reader does not have. The bundle was reproducible only on the machine that
/// wrote it, and the frozen viewer — having nothing to render — printed the
/// bindings into a `<pre>`.
///
/// There is deliberately no caller-supplied rung here, unlike the live ladder.
/// `visuals_ipc` already refuses an MCP caller's projection bytes outright and
/// re-resolves from the local inventory; a seal that accepted them would
/// reopen that door in the one place whose whole product is a receipt. The
/// single rung is the trusted local Trace V5 inventory, which requires nothing
/// of any caller — the property the live ladder was rebuilt around.
fn resolve_trace_evidence<'a>(
    object: &Map<String, Value>,
    evidence: &'a SealEvidence<'_>,
) -> Result<&'a ResolvedTraceEvidence> {
    let input = super::descriptor_input_name(&Value::Object(object.clone()))
        .unwrap_or_else(|_| "projection".to_string());
    let Some(request) = trace_binding_request(object) else {
        bail!(
            "trace input \"{input}\" is bound as trace_v5 but names no `source`, so the seal has \
             no sealed archive to derive its projection from. Bind the Trace V5 digest with \
             visual_bind_data_source before sealing."
        );
    };
    evidence.traces.get(&request).ok_or_else(|| {
        anyhow!(
            "trace input \"{input}\" names Trace V5 archive {} but the seal found no replayable \
             evidence for it: this host holds no trusted, self-contained bundle for that digest, \
             so the {} projection cannot be derived. Import the sealed Trace V5 bundle on this \
             machine before sealing visual {} revision {}.",
            request.0,
            request.1,
            evidence.visual_id,
            evidence.revision,
        )
    })
}

/// Find replayable evidence for one `live_sse` binding, or say what is missing.
///
/// The ladder is ordered so that the rung requiring nothing of a caller is the
/// one that normally answers. A required key nothing produces is how this path
/// came to be dead code; a host observation nobody has to remember to attach
/// cannot fail the same way.
fn resolve_live_evidence(
    object: &mut Map<String, Value>,
    evidence: &SealEvidence<'_>,
) -> Result<ResolvedEvidence> {
    let input = super::descriptor_input_name(&Value::Object(object.clone()))
        .unwrap_or_else(|_| super::LIVE_EVAL_INPUT.to_string());
    let stream_id = binding_stream_id(object);

    // 1. An inline snapshot on the descriptor. Kept because a caller that
    //    genuinely holds the evidence should not be refused, and because a
    //    snapshot may be any shape a template renders — it is frozen verbatim
    //    and, not being an envelope log, yields no projection.
    if let Some(snapshot) = object.remove("snapshot") {
        let envelopes = snapshot_envelopes(&snapshot);
        return Ok(ResolvedEvidence {
            data: snapshot,
            envelopes,
            origin: "descriptor",
            spool_digest: None,
            truncated: false,
        });
    }

    // 2. A CAS spool named on the descriptor. `storage/live_spool.rs` persists
    //    raw envelopes for exactly this after-the-fact replay, and a digest
    //    survives the engine, the process and the machine.
    let declared_digest = ["spool_digest", "spoolDigest"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::to_string);
    if let Some(digest) = declared_digest {
        let spool = crate::storage::load_live_spool(evidence.content, &digest)
            .with_context(|| format!("sealing live input \"{input}\" from spool {digest}"))?;
        return Ok(ResolvedEvidence {
            data: json!({ "events": spool.envelopes.clone() }),
            envelopes: spool.envelopes,
            origin: "spool",
            spool_digest: Some(spool.digest),
            truncated: false,
        });
    }

    // 3. What this host actually polled. Nothing had to be attached for this
    //    to be here, which is the point.
    if let Some(stream_id) = stream_id.as_deref() {
        if let Some(observed) = super::live_eval::observed_stream_evidence(
            evidence.visual_id,
            evidence.revision,
            stream_id,
        ) {
            let spool = crate::storage::persist_live_envelopes(
                evidence.content,
                Some(stream_id),
                None,
                observed.events.clone(),
            )
            .with_context(|| format!("spooling observed evidence for live input \"{input}\""))?;
            return Ok(ResolvedEvidence {
                data: json!({ "events": spool.envelopes.clone() }),
                envelopes: spool.envelopes,
                origin: "host_observation",
                spool_digest: Some(spool.digest),
                truncated: observed.truncated,
            });
        }
    }

    // Naming the stream and the three ways to supply it, because "live SSE
    // binding has no frozen snapshot" named a key no caller could write and
    // sent every reader looking for a producer that did not exist.
    bail!(
        "live input \"{input}\" declares stream {} but the seal found no replayable evidence for it: \
         this host recorded no envelopes for visual {} revision {}, the binding names no \
         spool_digest, and it carries no inline snapshot. Open the visual in Desktop so the \
         declared stream is polled, or bind a {} digest before sealing.",
        stream_id.as_deref().unwrap_or("<none declared>"),
        evidence.visual_id,
        evidence.revision,
        crate::storage::LIVE_SPOOL_SCHEMA,
    )
}

/// The envelope log inside a descriptor snapshot, if it is one.
///
/// A snapshot may be any shape a template renders. Only the two shapes that
/// *are* an ordered envelope log are projected; anything else is frozen
/// verbatim and carries no projection, which is honest rather than a guess.
fn snapshot_envelopes(snapshot: &Value) -> Vec<Value> {
    if let Some(rows) = snapshot.as_array() {
        return rows.clone();
    }
    snapshot
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Freeze a visual's bindings into evidence, and collect the projections a
/// sealed viewer renders.
///
/// Returns the frozen bindings and one view per live stream. The views are the
/// point: a sealed bundle that stores raw bindings and a runtime that re-folds
/// them stores a *promise* that the fold will still exist and still agree.
/// Storing the fold's output instead is what makes a seal survive the plugin,
/// the template and the build that produced it.
fn freeze_bindings(mut value: Value, evidence: &SealEvidence<'_>) -> Result<(Value, Vec<Value>)> {
    fn walk(value: &mut Value, evidence: &SealEvidence<'_>, views: &mut Vec<Value>) -> Result<()> {
        match value {
            Value::Object(object) => {
                let mut froze_evidence = false;
                if object.get("kind").and_then(Value::as_str) == Some("live_sse") {
                    froze_evidence = true;
                    let stream_id = binding_stream_id(object);
                    let input = super::descriptor_input_name(&Value::Object(object.clone())).ok();
                    let resolved = resolve_live_evidence(object, evidence)?;
                    object.insert("kind".into(), Value::String("inline".into()));
                    object.insert("data".into(), resolved.data);
                    object.remove("source");
                    object.remove("poll_url");
                    object.remove("pollUrl");
                    object.remove("spool_digest");
                    object.remove("spoolDigest");
                    // Absent, not null, for the descriptor snapshot path: a
                    // key that is always present would re-digest every seal
                    // that already worked, and a verbatim snapshot has no
                    // provenance to report beyond having been supplied.
                    if resolved.origin != "descriptor" {
                        let mut provenance = Map::new();
                        provenance.insert("origin".into(), json!(resolved.origin));
                        // The stream's *digest*, never its URL. A sealed
                        // bundle that names a loopback engine points at a
                        // machine the reader does not have and leaks the
                        // topology of one they do; the digest still tells a
                        // verifier holding the bindings that this evidence
                        // came from that stream and not another.
                        provenance.insert(
                            "stream_digest".into(),
                            stream_id
                                .as_deref()
                                .map(|id| json!(hex_sha256(id.as_bytes())))
                                .unwrap_or(Value::Null),
                        );
                        provenance.insert("envelope_count".into(), json!(resolved.envelopes.len()));
                        provenance.insert("truncated".into(), json!(resolved.truncated));
                        if let Some(digest) = &resolved.spool_digest {
                            provenance.insert("spool_digest".into(), json!(digest));
                            provenance.insert(
                                "spool_schema".into(),
                                json!(crate::storage::LIVE_SPOOL_SCHEMA),
                            );
                        }
                        object.insert("evidence".into(), Value::Object(provenance));
                    }
                    if !resolved.envelopes.is_empty() {
                        let mut view = Map::new();
                        if let Some(input) = input {
                            view.insert("input".into(), json!(input));
                        }
                        let projection = super::live_eval::seal_projection(&resolved.envelopes)?;
                        view.insert(
                            "schema_version".into(),
                            projection
                                .get("schema_version")
                                .cloned()
                                .unwrap_or(Value::Null),
                        );
                        view.insert("data".into(), projection);
                        views.push(Value::Object(view));
                    }
                }
                if object.get("kind").and_then(Value::as_str) == Some("trace_v5") {
                    froze_evidence = true;
                    let projection_kind = trace_binding_request(object)
                        .map(|request| request.1)
                        .unwrap_or_default();
                    let resolved = resolve_trace_evidence(object, evidence)?;
                    // A projection that does not say what it is would be
                    // frozen, sealed, and then silently unrenderable: the
                    // viewer and `locate_sealed_projections` both key on the
                    // document's own `schema_version`, and a document without
                    // one falls through to the `<pre>` this change exists to
                    // remove. Refusing here is the difference between a seal
                    // that carries evidence and one that promises it.
                    let declared = resolved
                        .payload
                        .get("schema_version")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !declared.starts_with("synth.trace-projection.") {
                        bail!(
                            "Trace V5 archive {} resolved a {} projection whose document declares \
                             schema_version {:?}; a sealed projection must declare its own \
                             synth.trace-projection.* schema or no reader can render it.",
                            resolved.trace_digest,
                            resolved.projection_schema,
                            declared,
                        );
                    }
                    object.insert("kind".into(), Value::String("inline".into()));
                    object.insert("data".into(), resolved.payload.clone());
                    // The archive digest moves into `evidence` rather than
                    // staying in `source`: a frozen binding whose `source`
                    // still named a CAS entry is exactly the pointer this
                    // change removes, and a reader must not be able to mistake
                    // one for a thing they can fetch.
                    object.remove("source");
                    object.remove("projection");
                    object.insert(
                        "evidence".into(),
                        json!({
                            "origin": "trace_inventory",
                            "trace_digest": resolved.trace_digest,
                            "projection_kind": projection_kind,
                            "projection_schema": resolved.projection_schema,
                            "payload_digest": resolved.payload_digest,
                        }),
                    );
                    // No view is pushed. The projection now *is* the binding's
                    // document, so `locate_sealed_projections` names it by
                    // pointer — one mechanism, and a bundle that does not carry
                    // a megabyte-scale projection twice.
                }
                // Frozen evidence is producer data, not a binding tree. An
                // envelope whose payload happens to describe a `live_sse`
                // binding — an eval streaming a visual's own configuration —
                // would otherwise be "frozen" a second time and fail the seal.
                for (key, child) in object.iter_mut() {
                    if froze_evidence && key.as_str() == "data" {
                        continue;
                    }
                    walk(child, evidence, views)?;
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, evidence, views)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut views = Vec::new();
    walk(&mut value, evidence, &mut views)?;
    if value.get("inputs").is_some() || value.get("slots").is_some() {
        if let Some(object) = value.as_object_mut() {
            object
                .entry("schemaVersion")
                .or_insert_with(|| json!(super::VISUAL_BINDINGS_SCHEMA_VERSION));
        }
        return Ok((super::canonicalize_bindings(&value)?.value, views));
    }
    Ok((value, views))
}

/// Whether this descriptor is one the seal froze producer evidence into.
///
/// One predicate, because three passes over the frozen bindings — the
/// limitations report, the projection locator and the redaction scan — all have
/// to tell the host's own binding *metadata* from the producer bytes underneath
/// it, and three spellings of that test would drift into three different
/// answers about the same document.
///
/// A descriptor snapshot supplied inline by a caller is deliberately *not*
/// this: it writes no `evidence` block (so that seals of that shape keep their
/// digests), and it is author-supplied rather than host-observed, so it stays
/// under the stricter reading everywhere.
fn frozen_evidence_descriptor(object: &Map<String, Value>) -> bool {
    object.get("kind").and_then(Value::as_str) == Some("inline")
        && object.get("evidence").is_some_and(Value::is_object)
}

/// Projection documents a template's own resolver already placed in the
/// bindings, named by JSON Pointer rather than copied.
///
/// The trace inspector's projection is computed upstream and rides inside the
/// bindings today. The runtime used to *find* it by scanning every binding for
/// a known `schema_version` — a locator in the viewer, which is the thing item
/// 3 removes. Naming its location at seal time moves that knowledge into the
/// sealed document, where it is pinned, and costs a pointer rather than a
/// second copy of a projection that can run to megabytes.
///
/// A frozen evidence document is a candidate, never a haystack. A `trace_v5`
/// binding's frozen `data` *is* the projection, so it is checked; a live
/// stream's frozen `data` is a hundred thousand producer envelopes, and one of
/// them may legitimately quote a `synth.trace-projection.*` document — an
/// optimizer event carrying a proposer's trace does exactly that. Searching
/// inside producer bytes would name that envelope as the seal's own view and
/// point the rollout inspector at it.
pub(super) fn locate_sealed_projections(bindings: &Value) -> Vec<Value> {
    fn escape(key: &str) -> String {
        key.replace('~', "~0").replace('/', "~1")
    }
    fn projection_schema(value: &Value) -> Option<&str> {
        value
            .get("schema_version")
            .and_then(Value::as_str)
            .filter(|schema| schema.starts_with("synth.trace-projection."))
    }
    fn walk(value: &Value, pointer: &str, out: &mut Vec<Value>) {
        match value {
            Value::Object(object) => {
                if let Some(schema) = projection_schema(value) {
                    out.push(json!({
                        "schema_version": schema,
                        "ref": format!("/bindings{pointer}"),
                    }));
                    return;
                }
                let frozen = frozen_evidence_descriptor(object);
                for (key, child) in object {
                    if frozen && key.as_str() == "data" {
                        if let Some(schema) = projection_schema(child) {
                            out.push(json!({
                                "schema_version": schema,
                                "ref": format!("/bindings{pointer}/data"),
                            }));
                        }
                        continue;
                    }
                    walk(child, &format!("{pointer}/{}", escape(key)), out);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{pointer}/{index}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(bindings, "", &mut out);
    out
}


pub(super) async fn freeze(
    registry: &super::VisualRegistry, value: Value, visual_id: &str, revision: i64,
) -> Result<(Value, Vec<Value>)> {
    let data = crate::data::DataStore::new(registry.db.clone(), registry.content.clone());
    let mut traces = BTreeMap::new();
    for request in trace_binding_requests(&value) {
        let projection = data.resolve_trace_projection(request.0.clone(), request.1.clone()).await
            .with_context(|| format!("sealing visual {visual_id} revision {revision}: import the trusted Trace V5 bundle {} before sealing its {} projection", request.0, request.1))?;
        traces.insert(request, ResolvedTraceEvidence {
            payload: projection.payload, trace_digest: projection.trace_digest,
            projection_schema: projection.projection_schema, payload_digest: projection.payload_digest,
        });
    }
    freeze_bindings(value, &SealEvidence { visual_id, revision, content: &registry.content, traces })
}

#[cfg(test)]
pub(super) fn freeze_fixture(value: Value, content: &crate::storage::ContentStore, visual_id: &str, revision: i64) -> Result<(Value, Vec<Value>)> {
    freeze_bindings(value, &SealEvidence { visual_id, revision, content, traces: BTreeMap::new() })
}

#[cfg(test)]
mod tests;
