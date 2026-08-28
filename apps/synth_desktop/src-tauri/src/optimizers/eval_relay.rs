//! Relay a container's rollout event journal into the optimizer log *while the
//! rollout is still running*.
//!
//! The failure this module exists to remove: the Workshop eval driver started a
//! rollout with the container's blocking `POST /rollouts`, waited for the whole
//! request, and then read the retained journal for exactly two facts — the last
//! observation and the achievement list. A rollout that emitted 13 native
//! 768×768 PNG frames, 10 policy calls and 12 applied actions therefore reached
//! the visual as one terminal row saying "0 native frames", and the pane only
//! moved when an entire seed finished.
//!
//! Two rules shape everything here.
//!
//! **The rollout request stays synchronous.** Containers admits an `async`
//! submission without executing it until the separate completion route is
//! called, so flipping `submission_mode` would trade a slow visual for a
//! rollout that never runs. Instead the blocking request becomes a concurrent
//! future and the declared poll URL is drained beside it, which keeps the
//! existing error semantics exactly as they were.
//!
//! **PNGs are media, not optimizer JSON.** A frame event names a loopback URL;
//! this fetches it, validates it as a real PNG within declared bounds, stores it
//! by Workshop's *own* SHA-256 in the content store, and puts a reference in the
//! event. The producer's digest travels beside it as provenance and is never
//! treated as cryptographic — the one observed in the field is 16 hex
//! characters, which is a truncation, not a SHA-256.

use super::{events::OptimizerEventDraft, service::OptimizerService};
use crate::container_stream::{poll_event_list, STREAM_SUBSCRIBED_KIND};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Algorithm id every relayed event is filed under. Same lane as the rest of
/// the container-eval evidence, so one cursor reads the whole run.
const EVAL_ALGORITHM_ID: &str = "eval";

/// A frame wider or taller than this is refused before a decode buffer is
/// allocated. Craftax renders 768×768; four times that in each dimension is
/// generous for any environment while still bounding one decode to ~400 MB of
/// pixels rather than "whatever the container claimed in its IHDR".
const MAX_FRAME_DIMENSION: u32 = 8192;

/// Total pixels in one frame. Bounds the decode buffer independently of the
/// per-dimension cap, so 8192×8192 is refused even though each side passes.
const MAX_FRAME_PIXELS: u64 = 4096 * 4096;

/// How long the relay keeps draining after the rollout request has settled but
/// the journal has not reported `closed`. Bounded so a container that never
/// closes its stream cannot hold a trial open forever.
const JOURNAL_DRAIN_GRACE: Duration = Duration::from_secs(20);

/// Consecutive empty drains after the rollout has settled that count as "the
/// journal has finished arriving".
///
/// `capture.closed` is appended before `run_episode` returns, so a well-behaved
/// container is already closed by the time the response lands and this never
/// applies. It exists for producers whose page carries no cursor block at all:
/// without it a trial would sit out the full grace on every seed, turning a
/// correctness fix into a 20-second-per-rollout regression.
const SETTLED_IDLE_DRAINS: u32 = 2;

/// Per-request timeout for one frame fetch. A frame is a small local file; a
/// slow one is a degraded frame, not a reason to stall the trial.
const FRAME_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// How the recipe asks for the event stream to be drained.
///
/// Recipe-driven rather than constant: a 500-step environment and a two-call
/// classifier do not want the same cadence, and neither should be a code edit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EventStreamSettings {
    pub poll_interval: Duration,
    pub page_limit: u32,
    pub max_events_per_rollout: usize,
}

impl Default for EventStreamSettings {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(150),
            page_limit: 1000,
            max_events_per_rollout: 10_000,
        }
    }
}

impl EventStreamSettings {
    /// Clamp to the container's own documented bounds. `GET /rollouts/{id}/events`
    /// rejects `limit` outside 1..=10_000 with a 422, so a recipe that asks for
    /// more must be corrected here rather than failing every page.
    pub(crate) fn normalized(mut self) -> Self {
        self.page_limit = self.page_limit.clamp(1, 10_000);
        self.poll_interval = self
            .poll_interval
            .clamp(Duration::from_millis(20), Duration::from_secs(10));
        self.max_events_per_rollout = self.max_events_per_rollout.max(1);
        self
    }
}

/// What the recipe wants retained of the rollout's native frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameRetention {
    /// Every frame the producer offers, up to the declared budgets.
    All,
    /// No frame media at all. Frame *events* still relay; only the bytes are
    /// skipped, and the trial says so rather than reporting no frames existed.
    None,
}

impl FrameRetention {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "none" => Ok(Self::None),
            other => bail!("media.frame_retention must be \"all\" or \"none\", not {other:?}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaSettings {
    pub frame_retention: FrameRetention,
    pub max_frame_bytes: u64,
    pub max_frames_per_rollout: usize,
    pub max_total_frame_bytes: u64,
}

impl Default for MediaSettings {
    fn default() -> Self {
        Self {
            frame_retention: FrameRetention::All,
            max_frame_bytes: 4 * 1024 * 1024,
            max_frames_per_rollout: 1000,
            max_total_frame_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RelaySettings {
    pub event_stream: EventStreamSettings,
    pub media: MediaSettings,
}

impl RelaySettings {
    /// Telemetry frame block for `prepare`/`start`.
    ///
    /// The eval driver used to hard-code `frame.enabled: false`, which is the
    /// other half of "0 native frames": the container was asked not to render
    /// them. Retention policy decides this now.
    pub(crate) fn telemetry_frame(&self) -> Value {
        match self.media.frame_retention {
            FrameRetention::All => json!({"enabled": true, "format": "png", "every_n_steps": 1}),
            FrameRetention::None => json!({"enabled": false}),
        }
    }
}

/// One retained frame, as the optimizer event and the visual see it.
#[derive(Clone, Debug)]
pub(crate) struct FrameMedia {
    pub cas_digest: String,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
    pub byte_size: u64,
    /// The producer's own digest, verbatim. Not a SHA-256 and never used as one.
    pub producer_digest: Option<String>,
}

impl FrameMedia {
    fn to_json(&self) -> Value {
        json!({
            "casDigest": self.cas_digest,
            "mediaType": self.media_type,
            "width": self.width,
            "height": self.height,
            "byteSize": self.byte_size,
            "producerDigest": self.producer_digest,
        })
    }
}

/// The relayed history disagrees with the producer's own journal.
///
/// Kept as its own type because it is the one relay failure that must not
/// degrade quietly. A page that could not be fetched costs freshness; a
/// sequence gap means the events Workshop holds are not the episode that ran,
/// and a workbench folded from them shows a trajectory that never happened.
/// Everything else about the rollout can be fine and the evidence still be
/// wrong, so the trial fails closed.
#[derive(Debug)]
pub(crate) struct RelayIntegrityError {
    detail: String,
}

impl std::fmt::Display for RelayIntegrityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for RelayIntegrityError {}

/// A bound that was reached, with what it cost.
///
/// Every one of these becomes a durable receipt. The rule the whole module is
/// built around: a frame that was dropped is reported as dropped. Silence would
/// be indistinguishable from "the environment rendered nothing", which is the
/// exact claim that sent someone looking for a bug in Craftax.
#[derive(Clone, Debug)]
pub(crate) struct Degradation {
    pub reason: &'static str,
    pub detail: String,
    pub dropped: usize,
}

impl Degradation {
    fn to_json(&self) -> Value {
        json!({ "reason": self.reason, "detail": self.detail, "dropped": self.dropped })
    }
}

/// What the relay observed, beside whatever the rollout request returned.
#[derive(Clone, Debug, Default)]
pub(crate) struct RelayOutcome {
    pub relayed_events: usize,
    pub high_water: u64,
    pub journal_closed: bool,
    pub frames_declared: usize,
    pub frames_retained: usize,
    /// Distinct content objects behind retained frame observations. Multiple
    /// steps may render byte-identical PNGs and therefore share one CAS blob.
    pub unique_frame_blobs: std::collections::BTreeSet<String>,
    pub frame_bytes: u64,
    pub degradations: Vec<Degradation>,
}

impl RelayOutcome {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "relayedEvents": self.relayed_events,
            "highWater": self.high_water,
            "journalClosed": self.journal_closed,
            "framesDeclared": self.frames_declared,
            "framesRetained": self.frames_retained,
            "frameObservationsDeclared": self.frames_declared,
            "frameObservationsRetained": self.frames_retained,
            "uniqueFrameBlobs": self.unique_frame_blobs.len(),
            "frameBytes": self.frame_bytes,
            "degradations": self.degradations.iter().map(Degradation::to_json).collect::<Vec<_>>(),
        })
    }

    fn note(&mut self, reason: &'static str, detail: impl Into<String>, dropped: usize) {
        self.degradations.push(Degradation {
            reason,
            detail: detail.into(),
            dropped,
        });
    }
}

/// Identity and transport for one trial's relay. Everything correlated, so a
/// relayed event can never be filed against the wrong trial or rollout.
pub(crate) struct RelayContext<'a> {
    pub service: &'a OptimizerService,
    pub run_id: &'a str,
    pub trial_id: &'a str,
    pub rollout_id: &'a str,
    pub seed: i64,
    pub pool: &'a str,
    pub scenario: &'a str,
    /// Validated loopback origin of the registered container. Frame URLs are
    /// resolved against this and nothing else.
    pub base: &'a str,
    pub poll_url: &'a str,
    pub client: &'a reqwest::Client,
    /// Redirect-refusing client used only for frame bodies.
    pub media_client: &'a reqwest::Client,
    pub settings: RelaySettings,
}

/// Drain the declared event journal while `rollout` runs, relaying every
/// semantic event into the optimizer log.
///
/// Returns the rollout request's own result untouched — the relay is evidence,
/// and a relay problem must not turn a completed rollout into a failed one, nor
/// a failed rollout into a success.
pub(crate) async fn relay_while<F>(
    ctx: &RelayContext<'_>,
    rollout: F,
    cancel: &mut watch::Receiver<bool>,
) -> (Result<Value>, RelayOutcome)
where
    F: std::future::Future<Output = Result<Value>>,
{
    let mut outcome = RelayOutcome::default();
    let mut cursor: u64 = 0;
    let mut rollout = Box::pin(rollout);
    let mut settled: Option<Result<Value>> = None;
    let mut settled_at: Option<Instant> = None;
    let mut idle_drains: u32 = 0;
    let mut declares_cursor = false;
    let poll_interval = ctx.settings.event_stream.poll_interval;

    loop {
        if *cancel.borrow() && settled.is_none() {
            // Dropping the in-flight request is how a blocking rollout is
            // terminated: there is no abort route, and the container ends the
            // episode when its client goes away. The journal it already wrote
            // is still drained below, so a cancelled trial keeps its evidence.
            drop(rollout);
            let drained = drain(ctx, &mut cursor, &mut outcome).await;
            if let Err(error) = drained {
                outcome.note("relay_failed", format!("{error:#}"), 0);
            }
            outcome.note(
                "cancelled",
                "the run was cancelled while this rollout was open",
                0,
            );
            return (Err(anyhow!("container eval cancelled")), outcome);
        }

        match drain(ctx, &mut cursor, &mut outcome).await {
            Ok(summary) => {
                if summary.closed {
                    outcome.journal_closed = true;
                }
                declares_cursor |= summary.declares_cursor;
                if settled.is_some() {
                    idle_drains = if summary.relayed == 0 {
                        idle_drains + 1
                    } else {
                        0
                    };
                }
            }
            Err(error) => {
                let integrity = error.downcast_ref::<RelayIntegrityError>().is_some();
                outcome.note(
                    if integrity {
                        "relay_integrity"
                    } else {
                        "relay_failed"
                    },
                    format!("{error:#}"),
                    0,
                );
                // A relay that could not *read* is a degraded trial and the
                // rollout's own answer still stands. A relay whose history is
                // wrong is not: the trial fails with the integrity reason, and
                // the rollout is drained only so it does not leak.
                if integrity {
                    if settled.is_none() {
                        let _ = (&mut rollout).await;
                    }
                    return (Err(error), outcome);
                }
                if let Some(result) = settled {
                    return (result, outcome);
                }
                let result = (&mut rollout).await;
                return (result, outcome);
            }
        }

        if outcome.journal_closed && settled.is_some() {
            break;
        }
        if outcome.relayed_events >= ctx.settings.event_stream.max_events_per_rollout {
            outcome.note(
                "event_cap_reached",
                format!(
                    "stopped relaying at event_stream.max_events_per_rollout = {}",
                    ctx.settings.event_stream.max_events_per_rollout
                ),
                0,
            );
            let result = match settled {
                Some(result) => result,
                None => (&mut rollout).await,
            };
            return (result, outcome);
        }

        match settled {
            None => {
                tokio::select! {
                    biased;
                    result = &mut rollout => {
                        settled = Some(result);
                        settled_at = Some(Instant::now());
                    }
                    _ = tokio::time::sleep(poll_interval) => {}
                    _ = cancel.changed() => {}
                }
            }
            Some(_) => {
                // The rollout has answered but the journal has not closed. Keep
                // draining briefly: `capture.closed` is appended after the
                // response is composed, so the last few events routinely land
                // here.
                if settled_at.is_some_and(|at| at.elapsed() >= JOURNAL_DRAIN_GRACE) {
                    outcome.note(
                        "journal_not_closed",
                        format!(
                            "the rollout settled but its event journal did not close within {}s",
                            JOURNAL_DRAIN_GRACE.as_secs()
                        ),
                        0,
                    );
                    break;
                }
                if idle_drains >= SETTLED_IDLE_DRAINS {
                    if declares_cursor {
                        // The producer does report closure and has not. Say so:
                        // a viewer that trusts `journalClosed` must not be told
                        // a stream ended when the producer never said it did.
                        outcome.note(
                            "journal_not_closed",
                            "the rollout settled and its journal went quiet without reporting closed",
                            0,
                        );
                    }
                    break;
                }
                tokio::time::sleep(poll_interval).await;
            }
        }
    }

    let result = match settled {
        Some(result) => result,
        None => (&mut rollout).await,
    };
    (result, outcome)
}

/// What one full drain of the journal observed.
#[derive(Default)]
struct DrainSummary {
    /// The producer said its journal is closed. Authoritative when present.
    closed: bool,
    /// Semantic events relayed by this drain.
    relayed: usize,
    /// The page carried a `cursor` block at all. A producer without one cannot
    /// report closure, so its silence is a contract gap rather than a stall.
    declares_cursor: bool,
}

/// Read every page available at `cursor`, relaying each semantic event.
async fn drain(
    ctx: &RelayContext<'_>,
    cursor: &mut u64,
    outcome: &mut RelayOutcome,
) -> Result<DrainSummary> {
    let mut summary = DrainSummary::default();
    loop {
        let page = fetch_page(ctx, *cursor).await?;
        let events = poll_event_list(&page);
        let mut drafts = Vec::new();
        for event in events {
            // Control records carry no sequence and are transport bookkeeping
            // (`stream.subscribed`, heartbeats). They are not evidence.
            let Some(sequence) = event.get("sequence").and_then(Value::as_u64) else {
                continue;
            };
            if event.get("kind").and_then(Value::as_str) == Some(STREAM_SUBSCRIBED_KIND) {
                continue;
            }
            if sequence <= *cursor {
                // A retried page. The idempotency key would collapse it anyway;
                // skipping keeps the batch honest about what it appended.
                continue;
            }
            if sequence != *cursor + 1 {
                // Fail visibly. A gap means the producer's journal and this
                // cursor disagree about history, and a viewer folded from a
                // gapped stream shows a trajectory that never happened.
                return Err(anyhow::Error::new(RelayIntegrityError {
                    detail: format!(
                        "event sequence gap on {}: expected {}, received {}",
                        ctx.rollout_id,
                        *cursor + 1,
                        sequence
                    ),
                }));
            }
            let draft = relay_event(ctx, event, sequence, outcome).await?;
            drafts.push(draft);
            *cursor = sequence;
            outcome.relayed_events += 1;
            summary.relayed += 1;
            if outcome.relayed_events >= ctx.settings.event_stream.max_events_per_rollout {
                break;
            }
        }
        if !drafts.is_empty() {
            ctx.service
                .append_event_payloads(ctx.run_id.to_string(), drafts)
                .await
                .context("append relayed container events")?;
        }
        let cursor_block = page.get("cursor").filter(|value| value.is_object());
        summary.declares_cursor |= cursor_block.is_some();
        if let Some(high) = cursor_block
            .and_then(|value| value.get("high_water"))
            .and_then(Value::as_u64)
        {
            outcome.high_water = outcome.high_water.max(high);
        }
        if cursor_block
            .and_then(|value| value.get("closed"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            summary.closed = true;
        }
        let has_more = cursor_block
            .and_then(|value| value.get("has_more"))
            .and_then(Value::as_bool)
            == Some(true);
        if !has_more || outcome.relayed_events >= ctx.settings.event_stream.max_events_per_rollout {
            return Ok(summary);
        }
    }
}

async fn fetch_page(ctx: &RelayContext<'_>, after: u64) -> Result<Value> {
    let response = ctx
        .client
        .get(ctx.poll_url)
        .query(&[
            ("after", after.to_string()),
            ("limit", ctx.settings.event_stream.page_limit.to_string()),
        ])
        .send()
        .await
        .context("GET declared rollout event page")?;
    let status = response.status();
    if !status.is_success() {
        bail!("rollout event page returned {status}");
    }
    response
        .json::<Value>()
        .await
        .context("decode rollout event page")
}

/// Turn one producer envelope into one optimizer event.
///
/// The envelope is preserved, not flattened: `kind` keeps the producer's own
/// vocabulary (`frame`, `observation`, `span.policy.data`, `action_applied`,
/// `achievement_unlocked`, …) so the projector can read the real event stream
/// rather than a Workshop-invented one.
async fn relay_event(
    ctx: &RelayContext<'_>,
    event: &Value,
    sequence: u64,
    outcome: &mut RelayOutcome,
) -> Result<OptimizerEventDraft> {
    let kind = event
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut payload = event.get("payload").cloned().unwrap_or(json!({}));

    if kind == "frame" {
        outcome.frames_declared += 1;
        match retain_frame(ctx, &payload, outcome).await {
            Ok(Some(media)) => {
                if let Some(object) = payload.as_object_mut() {
                    object.insert("media".into(), media.to_json());
                }
                outcome.frames_retained += 1;
                outcome.unique_frame_blobs.insert(media.cas_digest.clone());
                outcome.frame_bytes += media.byte_size;
            }
            Ok(None) => {}
            Err(error) => {
                // A refused frame is named and counted. The trial keeps its
                // frame *event* — the step happened — and says the bytes did
                // not survive validation.
                outcome.note("frame_refused", format!("{error:#}"), 1);
                if let Some(object) = payload.as_object_mut() {
                    object.insert(
                        "mediaError".into(),
                        json!({ "reason": "refused", "detail": format!("{error:#}") }),
                    );
                }
            }
        }
    }

    let container_event = json!({
        "rollout_id": ctx.rollout_id,
        "sequence": sequence,
        "kind": kind,
        "occurred_at": event.get("ts").cloned().unwrap_or(Value::Null),
        "digest": event.get("digest").cloned().unwrap_or(Value::Null),
        "payload": payload,
    });

    let delta = Map::from_iter([
        ("trial_id".into(), json!(ctx.trial_id)),
        ("seed".into(), json!(ctx.seed)),
        ("pool".into(), json!(ctx.pool)),
        ("scenario".into(), json!(ctx.scenario)),
        ("message".into(), json!(kind)),
        ("containerEvent".into(), container_event.clone()),
    ]);

    Ok(
        OptimizerEventDraft::new("eval.trial.event", EVAL_ALGORITHM_ID)
            // One relay of one producer sequence. A retried page, a resumed
            // worker, and a restarted Workshop all re-offer the same fact.
            .idempotency_key(format!("eval:event:{}:{sequence}", ctx.rollout_id))
            .level("debug")
            .occurred_at_opt(event.get("ts").and_then(Value::as_str))
            .delta(delta)
            .raw(json!({
                "source": "container_eval",
                "trial_id": ctx.trial_id,
                "container_event": container_event,
            })),
    )
}

/// Fetch, validate, and store one native frame.
///
/// `Ok(None)` means the frame carried no retainable media (an ASCII-only
/// frame, or retention turned off) — not a failure. `Err` means the producer
/// offered PNG media this refused to trust.
async fn retain_frame(
    ctx: &RelayContext<'_>,
    payload: &Value,
    outcome: &mut RelayOutcome,
) -> Result<Option<FrameMedia>> {
    if ctx.settings.media.frame_retention == FrameRetention::None {
        return Ok(None);
    }
    if payload.get("format").and_then(Value::as_str) != Some("png") {
        return Ok(None);
    }
    let Some(url) = payload.get("url").and_then(Value::as_str) else {
        return Ok(None);
    };
    let step = payload
        .get("step")
        .and_then(Value::as_i64)
        .context("png frame event omitted its step")?;

    if outcome.frames_retained >= ctx.settings.media.max_frames_per_rollout {
        outcome.note(
            "frame_count_cap_reached",
            format!(
                "media.max_frames_per_rollout = {} reached; later frames keep their event and lose their bytes",
                ctx.settings.media.max_frames_per_rollout
            ),
            1,
        );
        return Ok(None);
    }
    if outcome.frame_bytes >= ctx.settings.media.max_total_frame_bytes {
        outcome.note(
            "frame_bytes_cap_reached",
            format!(
                "media.max_total_frame_bytes = {} reached; later frames keep their event and lose their bytes",
                ctx.settings.media.max_total_frame_bytes
            ),
            1,
        );
        return Ok(None);
    }

    let resolved = resolve_frame_url(ctx.base, ctx.rollout_id, step, url)?;
    let bytes = fetch_frame_bytes(ctx, &resolved).await?;
    let (width, height) = decode_png_dimensions(&bytes)?;
    let cas_digest = ctx
        .service
        .content()
        .put_bytes("eval_frames", &bytes)
        .context("store native frame in the content store")?;
    let media = FrameMedia {
        cas_digest,
        media_type: "image/png",
        width,
        height,
        byte_size: bytes.len() as u64,
        producer_digest: payload
            .get("digest")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    // Index it against the run before the event that references it is
    // appended. A visual that could see the reference but not be granted the
    // bytes would render a hole and blame the store.
    ctx.service
        .record_run_media(
            ctx.run_id,
            &super::service::RunMediaRow {
                cas_digest: media.cas_digest.clone(),
                kind: "eval_frames",
                media_type: media.media_type,
                byte_size: media.byte_size,
                width: Some(media.width),
                height: Some(media.height),
                rollout_id: Some(ctx.rollout_id.to_string()),
                trial_id: Some(ctx.trial_id.to_string()),
                step: Some(step),
                producer_digest: media.producer_digest.clone(),
            },
        )
        .await
        .context("index the retained frame against its run")?;
    Ok(Some(media))
}

/// Resolve a declared frame URL against the registered container's own origin,
/// and refuse anything that is not *this* rollout's frame for *this* step.
///
/// A container is a local process Workshop launched, but its event payloads are
/// still input. Without this, a `url` field is an arbitrary fetch from inside
/// the desktop app's network position.
pub(crate) fn resolve_frame_url(
    base: &str,
    rollout_id: &str,
    step: i64,
    declared: &str,
) -> Result<reqwest::Url> {
    let origin = reqwest::Url::parse(base).context("container base URL")?;
    if origin.scheme() != "http"
        || !matches!(
            origin.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("::1") | Some("[::1]")
        )
    {
        bail!("frame media is limited to registered loopback HTTP containers");
    }
    let resolved = origin.join(declared).context("declared frame URL")?;
    if resolved.scheme() != origin.scheme()
        || resolved.host_str() != origin.host_str()
        || resolved.port_or_known_default() != origin.port_or_known_default()
    {
        bail!("frame URL {resolved} does not resolve to the registered container origin {origin}");
    }
    if resolved.query().is_some() {
        bail!("frame URL {resolved} carries a query string");
    }
    let expected = format!("/rollouts/{rollout_id}/frames/{step}.png");
    if resolved.path() != expected {
        bail!(
            "frame URL path {} is not this rollout's step {step} ({expected})",
            resolved.path()
        );
    }
    Ok(resolved)
}

async fn fetch_frame_bytes(ctx: &RelayContext<'_>, url: &reqwest::Url) -> Result<Vec<u8>> {
    let response = tokio::time::timeout(
        FRAME_FETCH_TIMEOUT,
        ctx.media_client.get(url.clone()).send(),
    )
    .await
    .context("frame fetch timed out")?
    .context("GET native frame")?;
    let status = response.status();
    if status.is_redirection() {
        bail!("frame fetch was redirected ({status}); redirects are refused");
    }
    if !status.is_success() {
        bail!("frame fetch returned {status}");
    }
    let media_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if media_type != "image/png" {
        bail!("frame content type is {media_type:?}, not image/png");
    }
    if let Some(declared) = response.content_length() {
        if declared > ctx.settings.media.max_frame_bytes {
            bail!(
                "frame declares {declared} bytes, over media.max_frame_bytes = {}",
                ctx.settings.media.max_frame_bytes
            );
        }
    }
    let bytes = response.bytes().await.context("read native frame body")?;
    if bytes.len() as u64 > ctx.settings.media.max_frame_bytes {
        bail!(
            "frame is {} bytes, over media.max_frame_bytes = {}",
            bytes.len(),
            ctx.settings.media.max_frame_bytes
        );
    }
    Ok(bytes.to_vec())
}

/// Prove the bytes are a decodable PNG within bounds, and report its size.
///
/// A signature check alone accepts a truncated file with a valid header, which
/// then renders as a broken image in the pane with no evidence of why. The full
/// decode is the check that a frame is actually showable.
pub(crate) fn decode_png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_MAGIC) {
        bail!("frame body is not a PNG (signature mismatch)");
    }
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().context("decode PNG header")?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 {
        bail!("PNG declares a zero dimension ({width}x{height})");
    }
    if width > MAX_FRAME_DIMENSION || height > MAX_FRAME_DIMENSION {
        bail!("PNG is {width}x{height}, over the {MAX_FRAME_DIMENSION}px per-side ceiling");
    }
    if u64::from(width) * u64::from(height) > MAX_FRAME_PIXELS {
        bail!("PNG is {width}x{height}, over the {MAX_FRAME_PIXELS} pixel ceiling");
    }
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    reader
        .next_frame(&mut buffer)
        .context("decode PNG image data")?;
    Ok((width, height))
}

/// Client used for frame bodies: never follows a redirect, so a container
/// cannot point Workshop's fetch anywhere but at itself.
pub(crate) fn frame_media_client() -> Result<reqwest::Client> {
    crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(FRAME_FETCH_TIMEOUT)
        .build()
        .context("build frame media client")
}

/// Announce that a trial has begun, before any of its container events.
pub(crate) async fn append_trial_started(
    service: &OptimizerService,
    run_id: &str,
    work_item_id: &str,
    trial_id: &str,
    rollout_id: &str,
    seed: i64,
    pool: &str,
    scenario: &str,
    candidate_id: &str,
) -> Result<()> {
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![
                OptimizerEventDraft::new("eval.trial.started", EVAL_ALGORITHM_ID)
                    .idempotency_key(format!("eval:started:{work_item_id}"))
                    .delta(Map::from_iter([
                        ("workItemId".into(), json!(work_item_id)),
                        ("trial_id".into(), json!(trial_id)),
                        ("rollout_id".into(), json!(rollout_id)),
                        ("candidate_id".into(), json!(candidate_id)),
                        ("seed".into(), json!(seed)),
                        ("pool".into(), json!(pool)),
                        ("scenario".into(), json!(scenario)),
                        ("stage".into(), json!("screen")),
                    ]))
                    .raw(json!({ "source": "container_eval" })),
            ],
        )
        .await?;
    Ok(())
}

/// Record every bound this trial hit, as its own durable receipt.
pub(crate) async fn append_degradations(
    service: &OptimizerService,
    run_id: &str,
    trial_id: &str,
    outcome: &RelayOutcome,
) -> Result<()> {
    if outcome.degradations.is_empty() {
        return Ok(());
    }
    let dropped: usize = outcome.degradations.iter().map(|item| item.dropped).sum();
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![
                OptimizerEventDraft::new("eval.trial.degraded", EVAL_ALGORITHM_ID)
                    .idempotency_key(format!("eval:degraded:{trial_id}"))
                    .level("warn")
                    .delta(Map::from_iter([
                        ("trial_id".into(), json!(trial_id)),
                        ("dropped".into(), json!(dropped)),
                        (
                            "message".into(),
                            json!(format!(
                                "{} of {} native frames retained; {} bound(s) reached",
                                outcome.frames_retained,
                                outcome.frames_declared,
                                outcome.degradations.len()
                            )),
                        ),
                        ("relay".into(), outcome.to_json()),
                    ]))
                    .raw(json!({ "source": "container_eval" })),
            ],
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![0u8; (width * height * 4) as usize])
                .unwrap();
        }
        bytes
    }

    #[test]
    fn a_valid_png_reports_its_dimensions() {
        assert_eq!(decode_png_dimensions(&png(768, 768)).unwrap(), (768, 768));
    }

    #[test]
    fn a_truncated_png_is_refused_even_though_its_header_is_valid() {
        let full = png(64, 64);
        let truncated = &full[..full.len() - 20];
        let error = decode_png_dimensions(truncated).unwrap_err().to_string();
        assert!(error.contains("image data"), "{error}");
    }

    #[test]
    fn a_non_png_body_is_refused_on_signature() {
        let error = decode_png_dimensions(b"<html>nope</html>")
            .unwrap_err()
            .to_string();
        assert!(error.contains("signature"), "{error}");
    }

    #[test]
    fn frame_urls_must_name_this_rollout_and_step() {
        let base = "http://127.0.0.1:9110";
        assert!(resolve_frame_url(base, "roll_a", 7, "/rollouts/roll_a/frames/7.png").is_ok());
        for hostile in [
            "/rollouts/roll_b/frames/7.png",
            "/rollouts/roll_a/frames/8.png",
            "http://example.com/rollouts/roll_a/frames/7.png",
            "http://127.0.0.1:9999/rollouts/roll_a/frames/7.png",
            "/rollouts/roll_a/frames/7.png?x=1",
            "/etc/passwd",
        ] {
            assert!(
                resolve_frame_url(base, "roll_a", 7, hostile).is_err(),
                "accepted {hostile}"
            );
        }
    }

    #[test]
    fn a_non_loopback_container_cannot_serve_frames() {
        assert!(resolve_frame_url(
            "https://frames.example.com",
            "roll_a",
            0,
            "/rollouts/roll_a/frames/0.png"
        )
        .is_err());
    }

    #[test]
    fn retention_decides_whether_the_container_renders_frames_at_all() {
        let mut settings = RelaySettings::default();
        assert_eq!(settings.telemetry_frame()["enabled"], json!(true));
        settings.media.frame_retention = FrameRetention::None;
        assert_eq!(settings.telemetry_frame()["enabled"], json!(false));
    }

    #[test]
    fn page_limits_are_clamped_to_the_container_contract() {
        let settings = EventStreamSettings {
            page_limit: 50_000,
            ..EventStreamSettings::default()
        }
        .normalized();
        assert_eq!(settings.page_limit, 10_000);
    }
}
