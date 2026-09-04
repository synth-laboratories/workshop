use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const SUBJECT_KINDS: &[&str] = &[
    "visual_revision",
    "file_snapshot",
    "dataset_row",
    "image",
    "video",
    "rollout",
    "trace_v5",
    "trace_span",
    "model_call",
    "optimizer_checkpoint",
    "comparison",
];
const QUESTION_KINDS: &[&str] = &[
    "yes_no",
    "single_select",
    "multi_select",
    "preference",
    "ranking",
    "likert",
    "numeric",
    "rubric_score",
    "short_text",
    "long_text",
    "quiz_single",
    "quiz_multi",
    "quiz_order",
    "target_selection",
    "media_response",
    "composite",
];
const SELECTOR_KINDS: &[&str] = &[
    "visual_target",
    "text_quote",
    "image_region",
    "time_range",
    "trace_range",
    "dataset_field",
    "coordinate",
];

pub fn validate_task(task: &Value) -> Result<()> {
    let object = task.as_object().context("task must be a JSON object")?;
    let schema = string(object.get("schemaVersion"), "schemaVersion")?;
    if schema != super::models::TASK_SCHEMA {
        bail!("unsupported task schema `{schema}`");
    }
    let title = string(object.get("title"), "title")?;
    if title.trim().is_empty() || title.len() > 200 {
        bail!("title must be 1..=200 characters");
    }
    string(object.get("instructions"), "instructions")?;
    let subject = object
        .get("subject")
        .and_then(Value::as_object)
        .context("subject required")?;
    let kind = string(subject.get("kind"), "subject.kind")?;
    if !SUBJECT_KINDS.contains(&kind) {
        bail!("unsupported subject kind `{kind}`");
    }
    nonempty(subject.get("id"), "subject.id")?;
    digest(subject.get("digest"), "subject.digest")?;
    if let Some(evidence) = object.get("evidence") {
        let items = evidence.as_array().context("evidence must be an array")?;
        for item in items {
            let item = item
                .as_object()
                .context("evidence item must be an object")?;
            let kind = nonempty(item.get("kind"), "evidence.kind")?;
            if !matches!(
                kind,
                "visual"
                    | "file"
                    | "dataset_row"
                    | "image"
                    | "video"
                    | "rollout"
                    | "trace_v5"
                    | "trace_span"
                    | "model_call"
                    | "optimizer_checkpoint"
                    | "comparison"
            ) {
                bail!("unsupported evidence kind `{kind}`");
            }
            if item.get("url").is_some() {
                bail!("remote URLs must be registered as evidence artifacts before review");
            }
            if item
                .get("id")
                .or_else(|| item.get("artifactId"))
                .and_then(Value::as_str)
                .is_none()
            {
                bail!("evidence needs a registered id or artifactId");
            }
            digest(item.get("digest"), "evidence.digest")?;
            validate_evidence_payload(item)?;
        }
    }
    let rubric = object
        .get("rubric")
        .and_then(Value::as_object)
        .context("rubric required")?;
    nonempty(rubric.get("id"), "rubric.id")?;
    digest(rubric.get("digest"), "rubric.digest")?;
    let questions = object
        .get("questions")
        .and_then(Value::as_array)
        .context("questions required")?;
    if questions.is_empty() {
        bail!("at least one question is required");
    }
    let mut ids = BTreeSet::new();
    let mut positions = BTreeMap::new();
    let mut dependencies: BTreeMap<String, String> = BTreeMap::new();
    for (position, question) in questions.iter().enumerate() {
        let q = question.as_object().context("question must be an object")?;
        let id = nonempty(q.get("id"), "question.id")?.to_owned();
        if !ids.insert(id.clone()) {
            bail!("duplicate question id `{id}`");
        }
        positions.insert(id.clone(), position);
        let qkind = string(q.get("type"), "question.type")?;
        if !QUESTION_KINDS.contains(&qkind) {
            bail!("unsupported question type `{qkind}`");
        }
        validate_question_contract(q, &id, qkind)?;
        if matches!(
            qkind,
            "single_select"
                | "multi_select"
                | "preference"
                | "ranking"
                | "likert"
                | "quiz_single"
                | "quiz_multi"
                | "quiz_order"
        ) {
            let options = q.get("options").and_then(Value::as_array).context(
                "select, preference, ranking, likert, and quiz questions require options",
            )?;
            if options.is_empty() {
                bail!("question `{id}` requires at least one option");
            }
            let mut option_ids = BTreeSet::new();
            for option in options {
                let option_id = nonempty(option.get("id"), "question.options[].id")?;
                if !option_ids.insert(option_id) {
                    bail!("question `{id}` has duplicate option id `{option_id}`");
                }
            }
        }
        if qkind.starts_with("quiz_") && q.get("answerKey").is_some() {
            bail!("quiz answer keys must be sealed backend references, never inline task content");
        }
        if let Some(visibility) = q.get("visibility").and_then(Value::as_object) {
            if let Some(when) = visibility.get("when").and_then(Value::as_object) {
                let operator = string(when.get("operator"), "visibility.when.operator")?;
                if !matches!(
                    operator,
                    "equals"
                        | "not_equals"
                        | "contains"
                        | "contains_any"
                        | "answered"
                        | "not_answered"
                ) {
                    bail!("unsupported visibility operator `{operator}`");
                }
                dependencies.insert(
                    id,
                    nonempty(when.get("questionId"), "visibility.when.questionId")?.to_owned(),
                );
            }
        }
    }
    for (id, dependency) in &dependencies {
        if !ids.contains(dependency) {
            bail!("question `{id}` references unknown question `{dependency}`");
        }
        if positions.get(dependency) >= positions.get(id) {
            bail!("question `{id}` may only branch on an earlier question");
        }
        let mut cursor = dependency.as_str();
        let mut seen = BTreeSet::from([id.as_str()]);
        while let Some(next) = dependencies.get(cursor) {
            if !seen.insert(cursor) || next == id {
                bail!("question visibility contains a cycle at `{id}`");
            }
            cursor = next;
        }
    }
    Ok(())
}

fn validate_question_contract(
    question: &serde_json::Map<String, Value>,
    id: &str,
    kind: &str,
) -> Result<()> {
    let prompt = nonempty(question.get("prompt"), "question.prompt")?;
    if prompt.chars().count() < 24 {
        bail!("question `{id}` prompt is too short to be concrete");
    }
    if prompt.chars().count() > 140 {
        bail!("question `{id}` prompt is too long; keep it under 140 characters");
    }
    let normalized = prompt.to_ascii_lowercase();
    const MECHANICAL_QUESTIONS: &[&str] = &[
        "does the header",
        "does the visual say",
        "does the visual show",
        "does the visual state",
        "how many",
        "is the field present",
    ];
    if let Some(term) = MECHANICAL_QUESTIONS
        .iter()
        .find(|term| normalized.contains(**term))
    {
        bail!("question `{id}` asks a mechanical check (`{term}`); compute it in software and ask for human judgment instead");
    }
    const SUBJECTIVE_SHORTCUTS: &[&str] = &[
        "is this good",
        "is this clear",
        "is this useful",
        "is this helpful",
        "is this visual good",
        "is this visual clear",
        "what do you think",
        "give feedback",
    ];
    if let Some(term) = SUBJECTIVE_SHORTCUTS
        .iter()
        .find(|term| normalized.contains(**term))
    {
        bail!(
            "question `{id}` uses ambiguous term `{}`; ask about a directly observable condition",
            term.trim()
        );
    }
    let criteria = question
        .get("decisionCriteria")
        .and_then(Value::as_object)
        .with_context(|| format!("question `{id}` requires decisionCriteria"))?;
    if criteria
        .get("requiresHumanJudgment")
        .and_then(Value::as_bool)
        != Some(true)
    {
        bail!("question `{id}` must set decisionCriteria.requiresHumanJudgment=true");
    }
    concrete_text(
        criteria.get("evidence"),
        &format!("question `{id}` decisionCriteria.evidence"),
        100,
    )?;
    concrete_text(
        criteria.get("answerRule"),
        &format!("question `{id}` decisionCriteria.answerRule"),
        120,
    )?;

    if kind == "yes_no" {
        let guidance = question
            .get("answerGuidance")
            .and_then(Value::as_object)
            .with_context(|| format!("question `{id}` requires answerGuidance"))?;
        concrete_text(
            guidance.get("yes"),
            &format!("question `{id}` answerGuidance.yes"),
            100,
        )?;
        concrete_text(
            guidance.get("no"),
            &format!("question `{id}` answerGuidance.no"),
            100,
        )?;
        if question.get("allowAbstain").and_then(Value::as_bool) == Some(true) {
            concrete_text(
                guidance.get("not_enough_evidence"),
                &format!("question `{id}` answerGuidance.not_enough_evidence"),
                100,
            )?;
        }
    }
    Ok(())
}

fn concrete_text<'a>(value: Option<&'a Value>, field: &str, max_chars: usize) -> Result<&'a str> {
    let text = nonempty(value, field)?;
    if text.chars().count() < 16 {
        bail!("{field} must state a concrete observable or decision rule");
    }
    if text.chars().count() > max_chars {
        bail!("{field} is too long; keep it under {max_chars} characters");
    }
    Ok(text)
}

pub fn validate_selector(selector: &Value) -> Result<()> {
    let object = selector.as_object().context("selector must be an object")?;
    let kind = string(object.get("kind"), "selector.kind")?;
    if !SELECTOR_KINDS.contains(&kind) {
        bail!("unsupported selector kind `{kind}`");
    }
    match kind {
        "visual_target" => {
            nonempty(object.get("targetId"), "selector.targetId")?;
        }
        "text_quote" => {
            nonempty(object.get("quote"), "selector.quote")?;
            ordered_i64(object, "lineStart", "lineEnd")?;
        }
        "image_region" => {
            let x = unit_number(object.get("x"), "selector.x")?;
            let y = unit_number(object.get("y"), "selector.y")?;
            let width = unit_number(object.get("width"), "selector.width")?;
            let height = unit_number(object.get("height"), "selector.height")?;
            if width <= 0.0 || height <= 0.0 || x + width > 1.0 || y + height > 1.0 {
                bail!("image region must be a positive normalized rectangle inside the evidence");
            }
        }
        "time_range" => ordered_i64(object, "startMs", "endMs")?,
        "trace_range" => {
            if object.get("spanId").and_then(Value::as_str).is_none()
                && object.get("modelCallId").and_then(Value::as_str).is_none()
                && object
                    .get("startSequence")
                    .and_then(Value::as_i64)
                    .is_none()
            {
                bail!("trace selector needs a span, model call, or sequence range");
            }
            if object.get("startSequence").is_some() || object.get("endSequence").is_some() {
                ordered_i64(object, "startSequence", "endSequence")?;
            }
        }
        "dataset_field" => {
            nonempty(object.get("field"), "selector.field")?;
        }
        "coordinate" => {
            unit_number(object.get("x"), "selector.x")?;
            unit_number(object.get("y"), "selector.y")?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn validate_selector_for_evidence(selector: &Value, evidence: &Value) -> Result<()> {
    validate_selector(selector)?;
    let selector_kind = selector["kind"].as_str().unwrap_or_default();
    let evidence_kind = evidence["kind"].as_str().unwrap_or_default();
    let allowed = match evidence_kind {
        "visual" | "visual_revision" => matches!(selector_kind, "visual_target" | "coordinate"),
        "file" | "file_snapshot" => selector_kind == "text_quote",
        "dataset_row" => selector_kind == "dataset_field",
        "image" => matches!(selector_kind, "image_region" | "coordinate"),
        "video" => matches!(selector_kind, "time_range" | "image_region"),
        "rollout" | "trace_v5" | "trace_span" | "model_call" => selector_kind == "trace_range",
        "optimizer_checkpoint" | "comparison" => selector_kind == "visual_target",
        _ => false,
    };
    if !allowed {
        bail!("selector kind `{selector_kind}` is not valid for evidence kind `{evidence_kind}`");
    }
    Ok(())
}

pub fn evidence_preview_warnings(task: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    for item in task
        .get("evidence")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = item["kind"].as_str().unwrap_or("unknown");
        let renderable = match kind {
            "visual" => true,
            "file" => item
                .pointer("/snapshot/text")
                .and_then(Value::as_str)
                .is_some(),
            "dataset_row" => item.get("row").is_some() || item.pointer("/snapshot/row").is_some(),
            "image" | "video" => item
                .pointer("/snapshot/dataUrl")
                .and_then(Value::as_str)
                .is_some(),
            "rollout" | "trace_v5" | "trace_span" | "model_call" => item
                .pointer("/snapshot/events")
                .and_then(Value::as_array)
                .is_some(),
            "optimizer_checkpoint" | "comparison" => item.get("summary").is_some(),
            _ => false,
        };
        if !renderable {
            warnings.push(format!(
                "{} has no immutable inline preview; reviewers will see provenance only",
                item.get("label")
                    .or_else(|| item.get("artifactId"))
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or(kind)
            ));
        }
    }
    warnings
}

fn validate_evidence_payload(item: &serde_json::Map<String, Value>) -> Result<()> {
    let snapshot = item.get("snapshot").and_then(Value::as_object);
    if let Some(text) = snapshot
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
    {
        if text.len() > 2 * 1024 * 1024 {
            bail!("file preview exceeds the 2 MiB review limit");
        }
    }
    if let Some(data_url) = snapshot
        .and_then(|value| value.get("dataUrl"))
        .and_then(Value::as_str)
    {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or_default();
        let prefix = if kind == "image" {
            "data:image/"
        } else {
            "data:video/"
        };
        if !data_url.starts_with(prefix) || data_url.len() > 16 * 1024 * 1024 {
            bail!("media preview must be a bounded inline {prefix} data URL");
        }
    }
    if snapshot
        .and_then(|value| value.get("events"))
        .and_then(Value::as_array)
        .is_some_and(|events| events.len() > 10_000)
    {
        bail!("trace preview exceeds 10,000 events");
    }
    Ok(())
}

fn ordered_i64(object: &serde_json::Map<String, Value>, start: &str, end: &str) -> Result<()> {
    let start_value = object
        .get(start)
        .and_then(Value::as_i64)
        .with_context(|| format!("selector.{start} must be an integer"))?;
    let end_value = object
        .get(end)
        .and_then(Value::as_i64)
        .with_context(|| format!("selector.{end} must be an integer"))?;
    if start_value < 0 || end_value < start_value {
        bail!("selector.{end} must be greater than or equal to selector.{start}");
    }
    Ok(())
}

fn unit_number(value: Option<&Value>, field: &str) -> Result<f64> {
    let number = value
        .and_then(Value::as_f64)
        .with_context(|| format!("{field} must be numeric"))?;
    if !(0.0..=1.0).contains(&number) {
        bail!("{field} must be between 0 and 1");
    }
    Ok(number)
}

pub fn validate_answer(question: &Value, answer: &Value) -> Result<()> {
    let q = question.as_object().context("question must be an object")?;
    let kind = string(q.get("type"), "question.type")?;
    let value = answer.get("value").unwrap_or(answer);
    match kind {
        "yes_no" => {
            let candidate = value.as_str().context("answer.value must be a string")?;
            if candidate == "not_enough_evidence"
                && q.get("allowAbstain").and_then(Value::as_bool) != Some(true)
            {
                bail!("abstention is not allowed for this question");
            }
            one_of(value, &["yes", "no", "not_enough_evidence"])
        }
        "single_select" | "quiz_single" | "likert" => option_id(q, value),
        "multi_select" | "quiz_multi" | "ranking" | "quiz_order" => {
            let values = value.as_array().context("answer.value must be an array")?;
            let min = q
                .get("validation")
                .and_then(|v| v.get("min"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let max = q
                .get("validation")
                .and_then(|v| v.get("max"))
                .and_then(Value::as_u64)
                .unwrap_or(usize::MAX as u64) as usize;
            if values.len() < min || values.len() > max {
                bail!("answer selection count must be between {min} and {max}");
            }
            let mut seen = BTreeSet::new();
            for value in values {
                let id = value.as_str().context("selection IDs must be strings")?;
                if !seen.insert(id) {
                    bail!("selection IDs must be unique");
                }
                ensure_option(q, id)?;
            }
            Ok(())
        }
        "preference" => {
            let choice = value
                .as_object()
                .and_then(|v| v.get("optionId"))
                .unwrap_or(value)
                .as_str()
                .context("preference must contain an optionId")?;
            if choice == "tie" {
                if q.get("allowTie").and_then(Value::as_bool) == Some(true) {
                    return Ok(());
                }
                bail!("tie is not allowed for this preference");
            }
            if choice == "neither" {
                if q.get("allowNeither").and_then(Value::as_bool) == Some(true) {
                    return Ok(());
                }
                bail!("neither is not allowed for this preference");
            }
            if choice == "not_enough_evidence" {
                if q.get("allowAbstain").and_then(Value::as_bool) == Some(true) {
                    return Ok(());
                }
                bail!("abstention is not allowed for this preference");
            }
            ensure_option(q, choice)
        }
        "numeric" | "rubric_score" => {
            let number = value.as_f64().context("answer.value must be numeric")?;
            let min = q
                .get("validation")
                .and_then(|v| v.get("min"))
                .and_then(Value::as_f64)
                .unwrap_or(f64::NEG_INFINITY);
            let max = q
                .get("validation")
                .and_then(|v| v.get("max"))
                .and_then(Value::as_f64)
                .unwrap_or(f64::INFINITY);
            if number < min || number > max {
                bail!("answer must be between {min} and {max}");
            }
            Ok(())
        }
        "short_text" | "long_text" => {
            let text = string(Some(value), "answer.value")?;
            let max = q
                .get("validation")
                .and_then(|v| v.get("maxLength"))
                .and_then(Value::as_u64)
                .unwrap_or(if kind == "short_text" { 500 } else { 20_000 })
                as usize;
            if text.chars().count() > max {
                bail!("answer text exceeds {max} characters");
            }
            Ok(())
        }
        "target_selection" => validate_selector(value),
        "media_response" | "composite" => {
            value
                .as_object()
                .context("answer.value must be an object")?;
            Ok(())
        }
        _ => bail!("unsupported question type `{kind}`"),
    }
}

fn option_id(question: &serde_json::Map<String, Value>, value: &Value) -> Result<()> {
    let id = nonempty(Some(value), "answer.value")?;
    ensure_option(question, id)
}
fn ensure_option(question: &serde_json::Map<String, Value>, id: &str) -> Result<()> {
    if question
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .any(|option| option.get("id").and_then(Value::as_str) == Some(id))
        })
        .unwrap_or(false)
    {
        Ok(())
    } else {
        bail!("unknown option id `{id}`")
    }
}

fn one_of(value: &Value, allowed: &[&str]) -> Result<()> {
    let candidate = value.as_str().context("answer.value must be a string")?;
    if !allowed.contains(&candidate) {
        bail!("unsupported answer `{candidate}`");
    }
    Ok(())
}
fn string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str> {
    value
        .and_then(Value::as_str)
        .with_context(|| format!("{field} must be a string"))
}
fn nonempty<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str> {
    let value = string(value, field)?;
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(value)
}
fn digest(value: Option<&Value>, field: &str) -> Result<()> {
    let value = nonempty(value, field)?
        .strip_prefix("sha256:")
        .unwrap_or(nonempty(value, field)?);
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("{field} must be a sha256 digest");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(questions: Value) -> Value {
        json!({
            "schemaVersion":"synth.human-annotation-task.v1",
            "title":"Review a visual",
            "instructions":"Judge the visible evidence.",
            "subject":{"kind":"visual_revision","id":"vis_1","digest":format!("sha256:{}","a".repeat(64))},
            "rubric":{"id":"visualsbench.human_reference.v1","digest":format!("sha256:{}","b".repeat(64))},
            "questions":questions
        })
    }

    fn concrete(id: &str, kind: &str, prompt: &str) -> Value {
        let mut question = json!({
            "id":id,
            "type":kind,
            "prompt":prompt,
            "decisionCriteria":{
                "requiresHumanJudgment":true,
                "evidence":"Inspect the digest-bound subject shown in the evidence stage.",
                "answerRule":"Choose the response whose stated condition matches the visible evidence."
            }
        });
        if kind == "yes_no" {
            question["answerGuidance"] = json!({
                "yes":"The stated observable condition is present.",
                "no":"The stated observable condition is absent."
            });
        }
        question
    }

    #[test]
    fn accepts_structured_questions_and_rejects_inline_quiz_keys() {
        validate_task(&task(json!([concrete(
            "truth",
            "yes_no",
            "Does the subject display the required value?"
        )])))
        .unwrap();
        let error = validate_task(&task(
            json!([{
                "id":"quiz","type":"quiz_single","prompt":"Which visible option matches the retained evidence?",
                "decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Inspect the retained evidence value shown above.","answerRule":"Choose the one option whose value exactly matches it."},
                "options":[{"id":"a"}],"answerKey":"a"
            }]),
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("answer keys"), "{error}");
    }

    #[test]
    fn rejects_visibility_cycles_and_preserves_insufficient_evidence() {
        let error = validate_task(&task(json!([
            {"id":"a","type":"yes_no","prompt":"Does the first retained field contain a value?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Inspect the first retained field in the subject.","answerRule":"Answer yes only when a nonempty value is visible."},"answerGuidance":{"yes":"A nonempty field value is visibly present.","no":"The field is empty or visibly absent."},"visibility":{"when":{"questionId":"b","operator":"answered"}}},
            {"id":"b","type":"yes_no","prompt":"Does the second retained field contain a value?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Inspect the second retained field in the subject.","answerRule":"Answer yes only when a nonempty value is visible."},"answerGuidance":{"yes":"A nonempty field value is visibly present.","no":"The field is empty or visibly absent."},"visibility":{"when":{"questionId":"a","operator":"answered"}}}
        ])))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("earlier question") || error.contains("cycle"),
            "{error}"
        );
        validate_answer(
            &json!({"type":"yes_no","allowAbstain":true}),
            &json!({"value":"not_enough_evidence"}),
        )
        .unwrap();
    }

    #[test]
    fn rejects_ambiguous_questions_and_requires_explicit_yes_no_semantics() {
        let vague = validate_task(&task(json!([{
            "id":"vague","type":"yes_no","prompt":"Is this visual clear and useful?",
            "decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Inspect the complete visual shown in the evidence stage.","answerRule":"Choose the response that best matches your impression."},
            "answerGuidance":{"yes":"The visual seems sufficiently understandable.","no":"The visual seems insufficiently understandable."}
        }]))).unwrap_err().to_string();
        assert!(vague.contains("ambiguous term"), "{vague}");

        let missing = validate_task(&task(json!([{
            "id":"missing","type":"yes_no","prompt":"Would you rely on this comparison to choose a run?",
            "decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Consider the comparison and its stated limits.","answerRule":"Answer yes only if it supports a confident choice."}
        }]))).unwrap_err().to_string();
        assert!(missing.contains("answerGuidance"), "{missing}");
    }

    #[test]
    fn validates_selection_and_numeric_bounds() {
        assert!(validate_answer(
            &json!({"type":"multi_select","validation":{"min":1,"max":2}}),
            &json!({"value":[]})
        )
        .is_err());
        assert!(validate_answer(
            &json!({"type":"rubric_score","validation":{"min":1,"max":5}}),
            &json!({"value":0})
        )
        .is_err());
        validate_answer(
            &json!({"type":"rubric_score","validation":{"min":1,"max":5}}),
            &json!({"value":5}),
        )
        .unwrap();
    }
}
