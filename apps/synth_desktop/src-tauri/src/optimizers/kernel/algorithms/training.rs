//! Durable, bounded training series shared by the SFT and CISPO reducers.
//!
//! Before this module the training projections carried only the *latest*
//! loss and step. Every surface that wanted a curve or a checkpoint scorecard
//! — `TrainingWorkspace` most visibly — re-read the whole event prefix on a
//! timer and rebuilt the series in the renderer. The series now lives in the
//! projection, where it is written once per fold and read by page.
//!
//! Both series are bounded by construction:
//!
//!   · evaluations are bounded by the number of checkpoint evaluations a run
//!     performs, which is a configuration fact rather than a step count;
//!   · metric points are decimated once they pass a fixed ceiling, doubling the
//!     stride each time, so a 100,000-step run keeps a deterministic
//!     downsampled curve rather than 100,000 rows in the primary projection.
//!     The most recent point is always retained so "latest" is never lost.
//!
//! Decimation is a function of the event order alone, so replaying the journal
//! reproduces the same series byte for byte.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Ceiling on retained metric points before decimation halves the series.
pub const METRIC_SERIES_CEILING: usize = 2_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingEvaluationSummary {
    /// Stable identity: the checkpoint id when reported, else the phase+step.
    pub id: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub step: Option<u64>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub loss: Option<f64>,
    #[serde(default)]
    pub delta: Option<f64>,
    #[serde(default)]
    pub macro_f1: Option<f64>,
    #[serde(default)]
    pub ci_low: Option<f64>,
    #[serde(default)]
    pub ci_high: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub paired_n: Option<u64>,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub claim_ready: Option<bool>,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub artifact_digest: Option<String>,
    #[serde(default)]
    pub evaluator: Option<String>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub sample_count: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    /// Child eval run when the evaluation ran as its own optimizer run.
    #[serde(default)]
    pub child_run_id: Option<String>,
    /// Sequence of the event that reported it; the durable evidence pointer.
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
}

impl TrainingEvaluationSummary {
    /// Decode the `evaluation` object a `training.evaluation.completed` event
    /// carries. Absent or non-object evaluations produce `None`; the reducer
    /// then records the completion on the work item without inventing a score.
    pub fn from_payload(payload: &Value, sequence: u64) -> Option<Self> {
        let evaluation = payload.get("evaluation")?.as_object()?;
        let get = |key: &str| evaluation.get(key);
        let string = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| get(key).and_then(Value::as_str))
                .map(str::to_string)
        };
        let number = |keys: &[&str]| keys.iter().find_map(|key| get(key).and_then(Value::as_f64));
        let integer = |keys: &[&str]| keys.iter().find_map(|key| get(key).and_then(Value::as_u64));
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let phase = string(&["phase"]).or_else(|| {
            if kind.contains("checkpoint") {
                Some("checkpoint".into())
            } else if kind.contains("heldout") || kind.contains("final") {
                Some("heldout".into())
            } else {
                None
            }
        });
        let step = integer(&["step"]);
        let checkpoint_id = string(&["checkpoint_id", "checkpointId"]);
        let child_run_id = payload
            .get("optimizerRunId")
            .or_else(|| payload.get("childEvalRunId"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let sample_count = integer(&["sample_count", "sampleCount", "n"]).or_else(|| {
            evaluation
                .get("per_intent")
                .and_then(Value::as_object)
                .map(|intents| {
                    intents
                        .values()
                        .filter_map(|row| row.get("n").and_then(Value::as_u64))
                        .sum()
                })
        });
        let paired = evaluation.get("paired_uplift").and_then(Value::as_object);
        let paired_number = |key: &str| {
            paired
                .and_then(|value| value.get(key))
                .and_then(Value::as_f64)
        };
        let paired_integer = |key: &str| {
            paired
                .and_then(|value| value.get(key))
                .and_then(Value::as_u64)
        };
        let paired_string = |key: &str| {
            paired
                .and_then(|value| value.get(key))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let id = string(&["evaluation_id", "evaluationId"])
            .or_else(|| checkpoint_id.clone())
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    phase.as_deref().unwrap_or("checkpoint"),
                    step.map(|value| value.to_string())
                        .unwrap_or_else(|| format!("seq{sequence}"))
                )
            });
        Some(Self {
            id,
            phase,
            step,
            score: number(&["score", "calibration_accuracy", "accuracy"]),
            metric: string(&["metric"]).or_else(|| {
                if get("calibration_accuracy").is_some() {
                    Some("calibration_accuracy".into())
                } else if get("accuracy").is_some() {
                    Some("accuracy".into())
                } else {
                    None
                }
            }),
            loss: number(&["loss"]),
            delta: number(&["delta", "uplift"]).or_else(|| paired_number("uplift")),
            macro_f1: number(&["macro_f1", "macroF1"]),
            ci_low: number(&["ci_low", "ciLow"]).or_else(|| paired_number("ci_low")),
            ci_high: number(&["ci_high", "ciHigh"]).or_else(|| paired_number("ci_high")),
            confidence: number(&["confidence"]).or_else(|| paired_number("confidence")),
            paired_n: integer(&["paired_n", "pairedN"]).or_else(|| paired_integer("paired_n")),
            verdict: string(&["verdict"]).or_else(|| paired_string("verdict")),
            claim_ready: evaluation
                .get("claim_ready")
                .or_else(|| evaluation.get("claimReady"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    paired
                        .and_then(|value| value.get("claim_ready"))
                        .and_then(Value::as_bool)
                }),
            checkpoint_id,
            artifact_digest: string(&["artifact_digest", "artifactDigest", "digest"]),
            evaluator: string(&["evaluator"]),
            sample_count,
            status: string(&["status"]),
            child_run_id,
            sequence,
        })
    }

    /// Decode a directly reported per-seed/per-rollout measurement used by
    /// SFT baseline and paired-heldout phases.
    pub fn from_direct_payload(payload: &Value, phase: &str, sequence: u64) -> Option<Self> {
        let string = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| payload.get(key).and_then(Value::as_str))
                .map(str::to_string)
        };
        let number = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| payload.get(key).and_then(Value::as_f64))
        };
        let integer = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| payload.get(key).and_then(Value::as_u64))
        };
        let seed = payload.get("seed").and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|value| value.to_string()))
        });
        let identity = string(&["rolloutId", "rollout_id", "id"])
            .or_else(|| seed.as_ref().map(|seed| format!("seed:{seed}")))?;
        let id = format!("{phase}:{identity}");
        Some(Self {
            id,
            phase: Some(phase.into()),
            step: integer(&["step", "steps", "step_count"]),
            score: number(&["score", "reward", "total_reward"]),
            metric: string(&["metric"]),
            loss: number(&["loss"]),
            delta: number(&["delta", "lift"]),
            macro_f1: number(&["macro_f1", "macroF1"]),
            ci_low: number(&["ci_low", "ciLow"]),
            ci_high: number(&["ci_high", "ciHigh"]),
            confidence: number(&["confidence"]),
            paired_n: integer(&["paired_n", "pairedN"]),
            verdict: string(&["verdict"]),
            claim_ready: payload
                .get("claim_ready")
                .or_else(|| payload.get("claimReady"))
                .and_then(Value::as_bool),
            checkpoint_id: string(&["checkpointId", "checkpoint_id"]),
            artifact_digest: string(&[
                "traceDigest",
                "trace_digest",
                "trace_v5_digest",
                "artifactDigest",
                "artifact_digest",
            ]),
            evaluator: string(&["evaluator"]),
            sample_count: Some(1),
            status: string(&["status"]).or_else(|| Some("completed".into())),
            child_run_id: string(&["optimizerRunId", "childEvalRunId"]),
            sequence,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingMetricPoint {
    #[specta(type = specta_typescript::Number)]
    pub step: u64,
    #[serde(default)]
    pub loss: Option<f64>,
    #[serde(default)]
    pub learning_rate: Option<f64>,
    #[serde(default)]
    pub reward: Option<f64>,
    #[serde(default)]
    pub advantage: Option<f64>,
    #[serde(default)]
    pub advantage_std: Option<f64>,
    #[serde(default)]
    pub reward_variance: Option<f64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub group_size: Option<u64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub optimizer_step: Option<u64>,
    #[serde(default)]
    pub tokens_per_second: Option<f64>,
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
}

impl TrainingMetricPoint {
    pub fn from_payload(payload: &Value, sequence: u64) -> Option<Self> {
        let number = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| payload.get(key).and_then(Value::as_f64))
        };
        let step = payload.get("step").and_then(Value::as_u64)?;
        Some(Self {
            step,
            loss: number(&["trainLoss", "train_loss", "loss"]),
            learning_rate: number(&["learningRate", "learning_rate", "lr"]),
            reward: number(&["meanReward", "mean_reward", "reward"]),
            advantage: number(&["meanAdvantage", "mean_advantage", "advantage"]),
            advantage_std: number(&["advantageStd", "advantage_std", "advantage_sd"]),
            reward_variance: number(&["rewardVariance", "reward_variance"]),
            group_size: payload
                .get("groupSize")
                .or_else(|| payload.get("group_size"))
                .and_then(Value::as_u64),
            optimizer_step: payload
                .get("optimizerStep")
                .or_else(|| payload.get("optimizer_step"))
                .and_then(Value::as_u64),
            tokens_per_second: number(&["tokensPerSecond", "tokens_per_second"]),
            sequence,
        })
    }
}

/// A bounded, deterministically downsampled metric series.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MetricSeries {
    pub points: Vec<TrainingMetricPoint>,
    /// Current decimation stride. 1 until the ceiling is first reached.
    #[serde(default = "default_stride")]
    #[specta(type = specta_typescript::Number)]
    pub stride: u64,
    /// Every point ever offered, including the ones decimation dropped.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub observed: u64,
}

fn default_stride() -> u64 {
    1
}

impl MetricSeries {
    pub fn push(&mut self, point: TrainingMetricPoint) {
        if self.stride == 0 {
            self.stride = 1;
        }
        self.observed += 1;
        // The newest point always lands so `latest()` is exact; a point that
        // does not sit on the stride is replaced by the next one rather than
        // accumulating.
        if let Some(last) = self.points.last() {
            if last.step % self.stride != 0 && last.step != point.step {
                self.points.pop();
            }
        }
        self.points.push(point);
        if self.points.len() > METRIC_SERIES_CEILING {
            self.stride *= 2;
            let stride = self.stride;
            let last = self.points.len() - 1;
            let mut index = 0;
            self.points.retain(|point| {
                let keep = index == last || point.step % stride == 0;
                index += 1;
                keep
            });
        }
    }

    pub fn latest(&self) -> Option<&TrainingMetricPoint> {
        self.points.last()
    }
}

