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
    #[specta(type = specta_typescript::Number)]
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
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub artifact_digest: Option<String>,
    #[serde(default)]
    pub evaluator: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
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
        let id = checkpoint_id.clone().unwrap_or_else(|| {
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
            delta: number(&["delta"]),
            checkpoint_id,
            artifact_digest: string(&["artifact_digest", "artifactDigest", "digest"]),
            evaluator: string(&["evaluator"]),
            sample_count: integer(&["sample_count", "sampleCount", "n"]),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn point(step: u64) -> TrainingMetricPoint {
        TrainingMetricPoint {
            step,
            loss: Some(1.0 / (step as f64 + 1.0)),
            sequence: step + 1,
            ..TrainingMetricPoint::default()
        }
    }

    #[test]
    fn series_stays_under_the_ceiling_and_keeps_the_newest_point() {
        let mut series = MetricSeries::default();
        for step in 0..100_000u64 {
            series.push(point(step));
        }
        assert!(series.points.len() <= METRIC_SERIES_CEILING);
        assert_eq!(series.latest().map(|p| p.step), Some(99_999));
        assert_eq!(series.observed, 100_000);
        assert!(series.stride > 1);
        let steps: Vec<u64> = series.points.iter().map(|p| p.step).collect();
        let mut sorted = steps.clone();
        sorted.sort_unstable();
        assert_eq!(steps, sorted, "decimation preserves order");
    }

    #[test]
    fn decimation_is_a_function_of_input_order_only() {
        let mut once = MetricSeries::default();
        let mut twice = MetricSeries::default();
        for step in 0..9_000u64 {
            once.push(point(step));
        }
        for step in 0..9_000u64 {
            twice.push(point(step));
        }
        assert_eq!(once, twice);
        assert_eq!(
            serde_json::to_string(&once).unwrap(),
            serde_json::to_string(&twice).unwrap()
        );
    }

    #[test]
    fn evaluation_summary_decodes_the_nested_evaluation_object() {
        let summary = TrainingEvaluationSummary::from_payload(
            &json!({
                "optimizerRunId": "eval_child_1",
                "evaluation": {
                    "phase": "checkpoint", "step": 40, "score": 0.81, "loss": 0.42,
                    "checkpoint_id": "ckpt-40", "evaluator": "banking77", "sample_count": 200,
                    "status": "completed"
                }
            }),
            77,
        )
        .unwrap();
        assert_eq!(summary.id, "ckpt-40");
        assert_eq!(summary.metric.as_deref(), None);
        assert_eq!(summary.step, Some(40));
        assert_eq!(summary.score, Some(0.81));
        assert_eq!(summary.child_run_id.as_deref(), Some("eval_child_1"));
        assert_eq!(summary.sequence, 77);
        assert!(TrainingEvaluationSummary::from_payload(&json!({}), 1).is_none());
    }

    #[test]
    fn evaluation_summary_normalizes_fixture_accuracy() {
        let checkpoint = TrainingEvaluationSummary::from_payload(
            &json!({
                "kind": "sft.checkpoint_eval.completed",
                "evaluation": {
                    "checkpoint_id": "ckpt-10",
                    "step": 10,
                    "calibration_accuracy": 0.0,
                    "n": 1
                }
            }),
            14,
        )
        .unwrap();
        assert_eq!(checkpoint.phase.as_deref(), Some("checkpoint"));
        assert_eq!(checkpoint.metric.as_deref(), Some("calibration_accuracy"));
        assert_eq!(checkpoint.score, Some(0.0));
        assert_eq!(checkpoint.sample_count, Some(1));

        let heldout = TrainingEvaluationSummary::from_payload(
            &json!({
                "kind": "sft.heldout_eval.completed",
                "evaluation": {"accuracy": 0.5, "n": 2}
            }),
            40,
        )
        .unwrap();
        assert_eq!(heldout.phase.as_deref(), Some("heldout"));
        assert_eq!(heldout.metric.as_deref(), Some("accuracy"));
        assert_eq!(heldout.score, Some(0.5));
        assert_eq!(heldout.sample_count, Some(2));
    }
}
