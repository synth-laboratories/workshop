//! Evaluation campaigns: the contract that makes "an evaluation" a count.
//!
//! Five Workshop chats were each asked for "one complete end-to-end Craftax
//! evaluation" and each produced exactly one rollout, so the requested
//! 50-rollout study came back as five hand-picked seeds. Nothing was broken —
//! "evaluation" was a word in a prompt, and every agent was free to read it as
//! one rollout.
//!
//! A campaign is the missing noun. It is planned before anything runs, with a
//! fixed number of rollouts, its own non-overlapping seeds, and stable rollout
//! identities. Its result is computed here rather than reconstructed by the
//! agent afterwards, and it cannot report success while any of its rollouts is
//! missing.
//!
//! What this module deliberately does *not* own is execution. Rollouts still
//! start through the same prepared-rollout contract as before, with the same
//! stream subscription and visual gating. A campaign says which rollouts must
//! exist and judges whether they do.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Ceiling on one campaign's size. Ten is the Craftax study's shape; the cap is
/// there so a typo cannot enqueue a thousand paid rollouts.
pub const MAX_CAMPAIGN_ROLLOUTS: i64 = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignRolloutPlan {
    pub rollout_id: String,
    pub ordinal: i64,
    pub seed: i64,
    pub task_instance_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Campaign {
    pub id: String,
    pub session_id: Option<String>,
    pub container_id: String,
    pub title: String,
    pub expected_rollouts: i64,
    pub max_concurrency: i64,
    pub policy_ref: Value,
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub settled_at: Option<String>,
    pub rollouts: Vec<CampaignRolloutPlan>,
}

pub struct CampaignCreate {
    pub id: String,
    pub session_id: Option<String>,
    pub container_id: String,
    pub title: String,
    pub expected_rollouts: i64,
    pub max_concurrency: i64,
    pub policy_ref: Value,
    pub seeds: Vec<i64>,
    pub task_instance_template: String,
    pub created_at: String,
}

/// Seeds an agent may not have thought about are still the campaign's problem.
///
/// A caller either names its seeds or asks for a contiguous block from a start;
/// either way the plan is fixed before anything runs, so "which ten did we
/// sample" is answerable without reading a transcript.
pub fn resolve_seeds(
    seeds: Option<&Vec<Value>>,
    seed_start: Option<i64>,
    expected: i64,
) -> Result<Vec<i64>> {
    if let Some(values) = seeds {
        let resolved: Vec<i64> = values.iter().filter_map(Value::as_i64).collect();
        if resolved.len() != values.len() {
            bail!("campaign seeds must all be integers");
        }
        if resolved.len() as i64 != expected {
            bail!(
                "campaign expects {expected} rollouts but {} seeds were supplied",
                resolved.len()
            );
        }
        let mut sorted = resolved.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != resolved.len() {
            bail!("campaign seeds must be distinct; a repeated seed is not a second sample");
        }
        return Ok(resolved);
    }
    let start = seed_start.context("campaign requires seeds or seed_start")?;
    Ok((0..expected).map(|offset| start + offset).collect())
}

pub fn create(conn: &Connection, request: CampaignCreate) -> Result<Campaign> {
    if request.expected_rollouts < 1 || request.expected_rollouts > MAX_CAMPAIGN_ROLLOUTS {
        bail!(
            "a campaign runs between 1 and {MAX_CAMPAIGN_ROLLOUTS} rollouts, not {}",
            request.expected_rollouts
        );
    }
    if request.max_concurrency < 1 {
        bail!("campaign concurrency must be at least 1");
    }
    if request.seeds.len() as i64 != request.expected_rollouts {
        bail!("campaign seed count does not match its expected rollout count");
    }
    // Overlap with a campaign that is still open is the failure this catches:
    // five concurrent chats drawing from the same seed block would report five
    // independent samples of one thing.
    for seed in &request.seeds {
        let conflict: Option<(String, String)> = conn
            .query_row(
                "SELECT r.campaign_id, c.title
                   FROM eval_campaign_rollouts r
                   JOIN eval_campaigns c ON c.id = r.campaign_id
                  WHERE r.seed = ?1 AND c.status IN ('planned','running')
                  LIMIT 1",
                params![seed],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((campaign_id, title)) = conflict {
            bail!(
                "seed {seed} already belongs to open campaign {campaign_id} ({title}); \
                 concurrent campaigns must use non-overlapping seeds"
            );
        }
    }

    let plan: Vec<CampaignRolloutPlan> = request
        .seeds
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            let ordinal = index as i64 + 1;
            CampaignRolloutPlan {
                // Stable identity before execution: a rollout that never starts
                // is still nameable, which is what makes a missing one visible.
                rollout_id: format!("{}_r{ordinal:02}", request.id),
                ordinal,
                seed: *seed,
                task_instance_id: request
                    .task_instance_template
                    .replace("{seed}", &seed.to_string()),
                status: "planned".into(),
                terminal: None,
                started_at: None,
                settled_at: None,
            }
        })
        .collect();

    conn.execute(
        "INSERT INTO eval_campaigns(id,session_id,container_id,title,expected_rollouts,max_concurrency,policy_ref_json,plan_json,status,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'planned',?9)",
        params![
            &request.id,
            &request.session_id,
            &request.container_id,
            &request.title,
            request.expected_rollouts,
            request.max_concurrency,
            serde_json::to_string(&request.policy_ref)?,
            serde_json::to_string(&json!({"taskInstanceTemplate": request.task_instance_template}))?,
            &request.created_at,
        ],
    )?;
    for rollout in &plan {
        conn.execute(
            "INSERT INTO eval_campaign_rollouts(campaign_id,rollout_id,ordinal,seed,task_instance_id,status)
             VALUES(?1,?2,?3,?4,?5,'planned')",
            params![
                &request.id,
                &rollout.rollout_id,
                rollout.ordinal,
                rollout.seed,
                &rollout.task_instance_id
            ],
        )?;
    }
    let campaign = load(conn, &request.id)?;
    if let Some(session_id) = campaign.session_id.as_deref() {
        crate::experiments::attach(
            conn,
            session_id,
            crate::experiments::MEMBER_CAMPAIGN,
            &campaign.id,
            &campaign.created_at,
            &campaign.title,
        )?;
    }
    Ok(campaign)
}

pub fn load(conn: &Connection, id: &str) -> Result<Campaign> {
    let mut campaign = conn
        .query_row(
            "SELECT id,session_id,container_id,title,expected_rollouts,max_concurrency,policy_ref_json,status,created_at,started_at,settled_at
               FROM eval_campaigns WHERE id=?1",
            params![id],
            |row| {
                Ok(Campaign {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    container_id: row.get(2)?,
                    title: row.get(3)?,
                    expected_rollouts: row.get(4)?,
                    max_concurrency: row.get(5)?,
                    policy_ref: serde_json::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(Value::Null),
                    status: row.get(7)?,
                    created_at: row.get(8)?,
                    started_at: row.get(9)?,
                    settled_at: row.get(10)?,
                    rollouts: Vec::new(),
                })
            },
        )
        .optional()?
        .with_context(|| format!("unknown campaign {id}"))?;
    let mut statement = conn.prepare(
        "SELECT rollout_id,ordinal,seed,task_instance_id,status,terminal_json,started_at,settled_at
           FROM eval_campaign_rollouts WHERE campaign_id=?1 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![id], |row| {
        Ok(CampaignRolloutPlan {
            rollout_id: row.get(0)?,
            ordinal: row.get(1)?,
            seed: row.get(2)?,
            task_instance_id: row.get(3)?,
            status: row.get(4)?,
            terminal: row
                .get::<_, Option<String>>(5)?
                .and_then(|value| serde_json::from_str(&value).ok()),
            started_at: row.get(6)?,
            settled_at: row.get(7)?,
        })
    })?;
    for row in rows {
        campaign.rollouts.push(row?);
    }
    Ok(campaign)
}

pub fn campaign_for_rollout(conn: &Connection, rollout_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT campaign_id FROM eval_campaign_rollouts WHERE rollout_id=?1",
            params![rollout_id],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn record_started(conn: &Connection, rollout_id: &str, at: &str) -> Result<()> {
    conn.execute(
        "UPDATE eval_campaign_rollouts SET status='started', started_at=COALESCE(started_at,?2)
          WHERE rollout_id=?1 AND status='planned'",
        params![rollout_id, at],
    )?;
    conn.execute(
        "UPDATE eval_campaigns SET status='running', started_at=COALESCE(started_at,?2)
          WHERE id=(SELECT campaign_id FROM eval_campaign_rollouts WHERE rollout_id=?1)
            AND status='planned'",
        params![rollout_id, at],
    )?;
    Ok(())
}

/// Record one rollout's authoritative terminal state.
///
/// `terminal` is the container's own record, kept verbatim: the campaign result
/// is derived from what the container said, not from what an agent summarized.
pub fn record_terminal(
    conn: &Connection,
    rollout_id: &str,
    terminal: &Value,
    at: &str,
) -> Result<()> {
    let failed = terminal
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "failed" | "cancelled" | "error"));
    conn.execute(
        "UPDATE eval_campaign_rollouts
            SET status=?2, terminal_json=?3, settled_at=COALESCE(settled_at,?4)
          WHERE rollout_id=?1",
        params![
            rollout_id,
            if failed { "failed" } else { "terminal" },
            serde_json::to_string(terminal)?,
            at
        ],
    )?;
    Ok(())
}

/// Settle a campaign and compute its result.
///
/// The one rule that matters: a campaign that owes ten terminal rollouts and has
/// nine is `partial`, and says which one is missing. It is never `complete` with
/// a smaller sample, and the caller never has to count for itself.
pub fn settle(conn: &Connection, id: &str, at: &str) -> Result<Value> {
    let campaign = load(conn, id)?;
    let terminal: Vec<&CampaignRolloutPlan> = campaign
        .rollouts
        .iter()
        .filter(|rollout| rollout.status == "terminal")
        .collect();
    let missing: Vec<Value> = campaign
        .rollouts
        .iter()
        .filter(|rollout| rollout.status != "terminal")
        .map(|rollout| {
            json!({
                "rolloutId": rollout.rollout_id,
                "seed": rollout.seed,
                "status": rollout.status,
            })
        })
        .collect();
    let status = if terminal.len() as i64 == campaign.expected_rollouts {
        "complete"
    } else if terminal.is_empty() {
        "failed"
    } else {
        "partial"
    };
    conn.execute(
        "UPDATE eval_campaigns SET status=?2, settled_at=?3 WHERE id=?1",
        params![id, status, at],
    )?;
    let aggregate = aggregate(&terminal);
    let result = json!({
        "campaignId": campaign.id,
        "title": campaign.title,
        "containerId": campaign.container_id,
        "status": status,
        "expectedRollouts": campaign.expected_rollouts,
        "terminalRollouts": terminal.len(),
        "missing": missing,
        "seeds": campaign.rollouts.iter().map(|rollout| rollout.seed).collect::<Vec<_>>(),
        "aggregate": aggregate,
        "rollouts": campaign.rollouts,
        "settledAt": at,
    });
    let trace_refs = terminal
        .iter()
        .filter_map(|rollout| {
            rollout
                .terminal
                .as_ref()
                .and_then(|value| {
                    value
                        .pointer("/trace/url")
                        .or_else(|| value.pointer("/trace/bundle_url"))
                })
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    let model = campaign
        .policy_ref
        .get("model")
        .or_else(|| campaign.policy_ref.get("config"))
        .and_then(Value::as_str);
    crate::experiments::settle_member(
        conn,
        crate::experiments::MEMBER_CAMPAIGN,
        &campaign.id,
        status,
        &campaign.title,
        model,
        &result["aggregate"],
        &trace_refs,
        at,
    )?;
    for rollout in terminal {
        let Some(state) = rollout.terminal.as_ref() else {
            continue;
        };
        let trace = state.get("trace").unwrap_or(&Value::Null);
        crate::experiments::attach_member_evidence(
            conn,
            crate::experiments::MEMBER_CAMPAIGN,
            &campaign.id,
            crate::experiments::ExperimentEvidenceAttachRequest {
                experiment_id: String::new(),
                session_id: None,
                node_id: None,
                evidence_id: format!("trace:{}:{}", campaign.container_id, rollout.rollout_id),
                kind: "trace".into(),
                label: format!("Seed {} trace", rollout.seed),
                digest: trace
                    .get("content_digest")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                container_id: Some(campaign.container_id.clone()),
                rollout_id: Some(rollout.rollout_id.clone()),
                trace_id: trace
                    .get("trace_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                visual_id: None,
                artifact_uri: None,
                metadata: Some(crate::contract::specta::OpaqueJson(serde_json::json!({
                    "announcedKind": trace.get("kind"),
                    "inspectable": trace.get("inspectable"),
                    "eventCount": trace.get("event_count"),
                }))),
                attached_at: at.to_owned(),
            },
        )?;
    }
    Ok(result)
}

fn number(terminal: &Value, pointers: &[&str]) -> Option<f64> {
    pointers
        .iter()
        .find_map(|pointer| terminal.pointer(pointer).and_then(Value::as_f64))
}

/// The distribution, computed here.
///
/// The previous study reported a mean of five hand-picked seeds reconstructed by
/// hand. Reward spread, achievement rates, why each episode ended, and how much
/// of the usage was actually reported are all campaign-level facts, and every
/// one of them is a `null` rather than a zero when it was never reported.
fn aggregate(terminal: &[&CampaignRolloutPlan]) -> Value {
    let mut rewards: Vec<f64> = Vec::new();
    let mut latencies: Vec<f64> = Vec::new();
    let mut achievements: std::collections::BTreeMap<String, i64> = Default::default();
    let mut termination: std::collections::BTreeMap<String, i64> = Default::default();
    let mut calls_total = 0.0;
    let mut calls_reported = 0;
    let mut usage_reported = 0;

    for rollout in terminal {
        let Some(record) = rollout.terminal.as_ref() else {
            continue;
        };
        if let Some(reward) = number(record, &["/reward", "/summary/reward", "/metrics/reward"]) {
            rewards.push(reward);
        }
        if let Some(latency) = number(record, &["/duration_ms", "/summary/duration_ms"]) {
            latencies.push(latency);
        } else if let (Some(started), Some(settled)) = (&rollout.started_at, &rollout.settled_at) {
            if let (Ok(started), Ok(settled)) = (
                chrono::DateTime::parse_from_rfc3339(started),
                chrono::DateTime::parse_from_rfc3339(settled),
            ) {
                latencies.push((settled - started).num_milliseconds().max(0) as f64);
            }
        }
        if let Some(calls) = number(
            record,
            &[
                "/model_calls",
                "/summary/model_calls",
                "/usage/requests",
                "/usage/calls",
                "/usage/llm_calls",
            ],
        ) {
            calls_total += calls;
            calls_reported += 1;
        }
        let usage = record.pointer("/usage").filter(|value| value.is_object());
        if usage.is_some_and(|usage| {
            ["prompt_tokens", "completion_tokens", "total_tokens"]
                .iter()
                .any(|field| usage.get(field).and_then(Value::as_f64).is_some())
        }) {
            usage_reported += 1;
        }
        let reason = record
            .pointer("/stopped_on")
            .or_else(|| record.pointer("/summary/stopped_on"))
            .or_else(|| record.pointer("/status"))
            .and_then(Value::as_str)
            .unwrap_or("unreported");
        *termination.entry(reason.to_owned()).or_insert(0) += 1;
        let names = record
            .pointer("/achievements")
            .or_else(|| record.pointer("/summary/achievements"))
            .and_then(Value::as_array);
        for name in names.into_iter().flatten() {
            let label = name
                .as_str()
                .map(str::to_owned)
                .or_else(|| name.get("name").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| "unnamed".into());
            *achievements.entry(label).or_insert(0) += 1;
        }
    }

    let sample = terminal.len() as f64;
    let achievement_rates: serde_json::Map<String, Value> = achievements
        .into_iter()
        .map(|(name, count)| {
            (
                name,
                json!({"count": count, "rate": if sample > 0.0 { count as f64 / sample } else { 0.0 }}),
            )
        })
        .collect();

    json!({
        "sampleSize": terminal.len(),
        "reward": distribution(&rewards),
        "latencyMs": distribution(&latencies),
        "achievementRates": Value::Object(achievement_rates),
        "terminationReasons": termination.into_iter().map(|(k, v)| (k, json!(v))).collect::<serde_json::Map<_, _>>(),
        "modelCalls": if calls_reported > 0 { json!(calls_total) } else { Value::Null },
        // Coverage, not a fabricated denominator: how many of the terminal
        // rollouts actually reported usage at all.
        "coverage": {
            "callCountReportedBy": calls_reported,
            "usageReportedBy": usage_reported,
            "of": terminal.len(),
        },
    })
}

fn distribution(values: &[f64]) -> Value {
    if values.is_empty() {
        // No sample is not a zero sample.
        return json!({"n": 0, "mean": Value::Null, "median": Value::Null, "min": Value::Null, "max": Value::Null, "values": []});
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    json!({
        "n": sorted.len(),
        "mean": mean,
        "median": median,
        "min": sorted[0],
        "max": sorted[sorted.len() - 1],
        "values": values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::migrations::apply_migrations(&conn).unwrap();
        conn
    }

    fn request(id: &str, seeds: Vec<i64>) -> CampaignCreate {
        CampaignCreate {
            id: id.into(),
            session_id: Some("session_1".into()),
            container_id: "ctr_1".into(),
            title: "Craftax ten-seed".into(),
            expected_rollouts: seeds.len() as i64,
            max_concurrency: 4,
            policy_ref: json!({"harness": "react", "config": "luna_low"}),
            seeds,
            task_instance_template: "seed:{seed}".into(),
            created_at: "2026-08-17T00:00:00Z".into(),
        }
    }

    fn terminal_record(reward: f64, achievements: &[&str], stopped_on: &str) -> Value {
        json!({
            "status": "completed",
            "reward": reward,
            "achievements": achievements,
            "stopped_on": stopped_on,
            "duration_ms": 66_000,
            "model_calls": 3,
            "usage": {"prompt_tokens": 1200, "completion_tokens": 300},
        })
    }

    #[test]
    fn a_plan_fixes_ten_identities_and_ten_seeds_before_anything_runs() {
        let conn = database();
        let campaign = create(&conn, request("camp_1", (201..=210).collect())).unwrap();
        assert_eq!(campaign.expected_rollouts, 10);
        assert_eq!(campaign.rollouts.len(), 10);
        assert_eq!(campaign.rollouts[0].rollout_id, "camp_1_r01");
        assert_eq!(campaign.rollouts[9].seed, 210);
        assert_eq!(campaign.rollouts[9].task_instance_id, "seed:210");
        assert!(campaign.rollouts.iter().all(|r| r.status == "planned"));
    }

    /// The defect this whole contract exists for: a chat asked for ten
    /// rollouts, ran one, and reported success.
    #[test]
    fn one_terminal_rollout_out_of_ten_is_partial_and_names_the_missing_nine() {
        let conn = database();
        create(&conn, request("camp_1", (201..=210).collect())).unwrap();
        record_started(&conn, "camp_1_r01", "2026-08-17T00:01:00Z").unwrap();
        record_terminal(
            &conn,
            "camp_1_r01",
            &terminal_record(0.6, &["collect_wood"], "max_steps"),
            "2026-08-17T00:02:00Z",
        )
        .unwrap();
        let result = settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["terminalRollouts"], 1);
        assert_eq!(result["expectedRollouts"], 10);
        assert_eq!(result["missing"].as_array().unwrap().len(), 9);
        assert_eq!(result["missing"][0]["rolloutId"], "camp_1_r02");
    }

    #[test]
    fn ten_terminal_rollouts_are_complete_and_carry_a_distribution() {
        let conn = database();
        create(&conn, request("camp_1", (201..=210).collect())).unwrap();
        for ordinal in 1..=10 {
            let rollout = format!("camp_1_r{ordinal:02}");
            record_started(&conn, &rollout, "2026-08-17T00:01:00Z").unwrap();
            record_terminal(
                &conn,
                &rollout,
                &terminal_record(ordinal as f64 / 10.0, &["collect_wood"], "max_steps"),
                "2026-08-17T00:02:00Z",
            )
            .unwrap();
        }
        let result = settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["aggregate"]["sampleSize"], 10);
        assert_eq!(result["aggregate"]["reward"]["n"], 10);
        assert_eq!(result["aggregate"]["reward"]["min"], 0.1);
        assert_eq!(result["aggregate"]["reward"]["max"], 1.0);
        assert_eq!(
            result["aggregate"]["achievementRates"]["collect_wood"]["count"],
            10
        );
        assert_eq!(result["aggregate"]["terminationReasons"]["max_steps"], 10);
        assert_eq!(result["aggregate"]["coverage"]["usageReportedBy"], 10);
    }

    #[test]
    fn container_usage_calls_and_rollout_timestamps_are_preserved() {
        let conn = database();
        create(&conn, request("camp_1", vec![201])).unwrap();
        record_started(&conn, "camp_1_r01", "2026-08-17T00:01:00Z").unwrap();
        record_terminal(
            &conn,
            "camp_1_r01",
            &json!({"status":"completed","reward":0.0,"usage":{"calls":2}}),
            "2026-08-17T00:01:04Z",
        )
        .unwrap();
        let result = settle(&conn, "camp_1", "2026-08-17T00:01:05Z").unwrap();
        assert_eq!(result["aggregate"]["reward"]["mean"], 0.0);
        assert_eq!(result["aggregate"]["modelCalls"], 2.0);
        assert_eq!(result["aggregate"]["coverage"]["callCountReportedBy"], 1);
        assert_eq!(result["aggregate"]["latencyMs"]["mean"], 4000.0);
    }

    #[test]
    fn a_failed_rollout_does_not_pass_as_terminal_evidence() {
        let conn = database();
        create(&conn, request("camp_1", vec![201, 202])).unwrap();
        record_terminal(
            &conn,
            "camp_1_r01",
            &terminal_record(1.0, &["collect_wood"], "max_steps"),
            "2026-08-17T00:02:00Z",
        )
        .unwrap();
        record_terminal(
            &conn,
            "camp_1_r02",
            &json!({"status": "failed", "error": "engine crash"}),
            "2026-08-17T00:02:00Z",
        )
        .unwrap();
        let result = settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["missing"][0]["status"], "failed");
    }

    /// Five chats drawing from one seed block would report five samples of the
    /// same thing and call it between-campaign variance.
    #[test]
    fn open_campaigns_may_not_share_a_seed() {
        let conn = database();
        create(&conn, request("camp_1", (201..=210).collect())).unwrap();
        let overlap = create(&conn, request("camp_2", (205..=214).collect()));
        let error = overlap.unwrap_err().to_string();
        assert!(error.contains("seed 205"), "{error}");
        assert!(error.contains("non-overlapping"), "{error}");
    }

    #[test]
    fn a_settled_campaign_releases_its_seeds() {
        let conn = database();
        create(&conn, request("camp_1", (201..=210).collect())).unwrap();
        settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        // A finished experiment does not own seed 205 forever; a later study may
        // sample it again.
        create(&conn, request("camp_2", (205..=214).collect())).unwrap();
    }

    #[test]
    fn crash_restart_preserves_campaign_terminal_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("campaigns.db");
        {
            let conn = Connection::open(&path).unwrap();
            crate::storage::migrations::apply_migrations(&conn).unwrap();
            create(&conn, request("camp_1", (201..=210).collect())).unwrap();
            record_started(&conn, "camp_1_r01", "2026-08-17T00:01:00Z").unwrap();
            record_terminal(
                &conn,
                "camp_1_r01",
                &terminal_record(0.6, &["collect_wood"], "max_steps"),
                "2026-08-17T00:02:00Z",
            )
            .unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        crate::storage::migrations::apply_migrations(&conn).unwrap();
        let campaign = load(&conn, "camp_1").unwrap();
        assert_eq!(campaign.expected_rollouts, 10);
        assert_eq!(
            campaign
                .rollouts
                .iter()
                .filter(|rollout| rollout.status == "terminal")
                .count(),
            1
        );
        let result = settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["terminalRollouts"], 1);
        assert_eq!(result["missing"].as_array().unwrap().len(), 9);
    }

    #[test]
    fn campaign_create_attaches_the_session_experiment_group() {
        let conn = database();
        create(&conn, request("camp_1", vec![201, 202])).unwrap();
        let group = crate::experiments::load_for_session(&conn, "session_1")
            .unwrap()
            .expect("campaign create owns an experiment group");
        assert_eq!(group.members.len(), 1);
        assert_eq!(group.members[0].member_id, "camp_1");
        assert_eq!(
            group.members[0].member_kind,
            crate::experiments::MEMBER_CAMPAIGN
        );
    }

    #[test]
    fn seeds_must_be_distinct_and_match_the_expected_count() {
        assert!(resolve_seeds(Some(&vec![json!(1), json!(1)]), None, 2)
            .unwrap_err()
            .to_string()
            .contains("distinct"));
        assert!(resolve_seeds(Some(&vec![json!(1)]), None, 2)
            .unwrap_err()
            .to_string()
            .contains("2 rollouts"));
        assert_eq!(
            resolve_seeds(None, Some(201), 3).unwrap(),
            vec![201, 202, 203]
        );
    }

    #[test]
    fn an_empty_sample_reports_null_rather_than_zero() {
        let conn = database();
        create(&conn, request("camp_1", vec![201])).unwrap();
        let result = settle(&conn, "camp_1", "2026-08-17T00:03:00Z").unwrap();
        assert_eq!(result["status"], "failed");
        assert!(result["aggregate"]["reward"]["mean"].is_null());
        assert!(result["aggregate"]["modelCalls"].is_null());
    }
}
