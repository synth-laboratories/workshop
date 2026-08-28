//! Admission pipeline behaviour, driven by the NanoHorizon acceptance request.
//!
//! The fixtures describe the NanoHorizon acceptance request: container
//! `nanohorizon-craftax` speaking `synth.container.live-eval.v1`, model
//! `z-ai/glm-5.3-flash` through OpenRouter, policy `nanohorizon/glm-5.3-flash`,
//! seeds 780000–780004, five rollouts, ten model calls and two thousand steps
//! per rollout, and a $2.45 hard ceiling routed through the Workshop secrets
//! proxy. Each test then removes exactly one fact and asserts the specific
//! refusal, so a regression cannot pass by failing for a different reason.

use super::canonical::{digest_bytes, CanonicalJson};
use super::error::AdmissionErrorCode;
use super::ids::*;
use super::pipeline::*;
use super::spec::*;
use super::state::*;
use serde_json::json;
use std::num::NonZeroU64;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn container_id() -> ContainerId {
    ContainerId::new("nanohorizon-craftax").unwrap()
}

fn provider() -> ProviderId {
    ProviderId::new("openrouter").unwrap()
}

fn model_id() -> ModelId {
    ModelId::new("z-ai/glm-5.3-flash").unwrap()
}

fn nanohorizon_seeds() -> Vec<Seed> {
    (780_000..780_005).map(Seed).collect()
}

fn declaration() -> EvalDeclaration {
    EvalDeclaration {
        protocol: Some(LIVE_EVAL_PROTOCOL_V1.to_string()),
        declaration_digest: Some(DeclarationDigest::new("sha256:declaration-v1").unwrap()),
        evaluator: Some(DeclaredEvaluator {
            evaluator_id: EvaluatorId::new("craftax.achievements").unwrap(),
            evaluator_version: "1.4.2".into(),
            scoring_digest: digest_bytes(b"craftax-scoring-v1"),
        }),
        supported_models: vec![ModelPin {
            provider: provider(),
            model_id: model_id(),
        }],
        supports_seed_control: Some(true),
        maximum_rollouts: Some(64),
        maximum_model_calls_per_rollout: Some(32),
        maximum_steps_per_rollout: Some(4_000),
        operations: [
            "rollouts.prepare",
            "rollouts.start_prepared",
            "rollouts.get",
            "rollouts.poll",
            "reward.get",
            "trace_v5.capture",
            "usage.get",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    }
}

fn candidate() -> ContainerCandidate {
    ContainerCandidate {
        container_id: container_id(),
        registration_id: ContainerRegistrationId::new("registration-7").unwrap(),
        source_revision: SourceRevision::new("sha256:image-91af").unwrap(),
        health: "ready".into(),
        family: Some("craftax".into()),
        declaration: declaration(),
    }
}

fn policy_resolution() -> PolicyResolution {
    PolicyResolution {
        namespace: "nanohorizon".into(),
        name: "glm-5.3-flash".into(),
        revision: Some(PolicyRevision::new("rev-2026-08-26-a1").unwrap()),
        declared_configuration: CanonicalJson::new(json!({
            "api": "chat_completions",
            "effort": "medium",
            "max_calls": 10,
            "max_steps": 2000,
            "thinking_budget": 2048,
        }))
        .unwrap(),
        source_code: None,
        material: None,
    }
}

fn context() -> DiscoveryContext {
    DiscoveryContext {
        containers: vec![candidate()],
        policy: Some(policy_resolution()),
        credential_route_available: true,
        credential_route_detail: None,
        credential_capability_scope: Some(CredentialCapabilityScope::new(
            ["chat.completions".to_string()],
            900,
        )),
        catalog: Vec::new(),
    }
}

/// The NanoHorizon request, exactly as the handoff states it.
fn request() -> InlineRequest {
    InlineRequest {
        container_id: Some(container_id()),
        family: Some("craftax".into()),
        policy_namespace: Some("nanohorizon".into()),
        policy_name: Some("glm-5.3-flash".into()),
        policy_source_path: None,
        policy_overrides: None,
        provider: Some(provider()),
        model_id: Some(model_id()),
        seeds: nanohorizon_seeds(),
        maximum_rollouts: Some(5),
        maximum_model_calls_per_rollout: Some(10),
        maximum_steps_per_rollout: Some(2_000),
        hard_total_cost_usd: Some(2.45),
        evaluator: None,
    }
}

fn admit(
    request: &InlineRequest,
    context: &DiscoveryContext,
) -> Result<AdmissibleExecutionSpec, super::error::AdmissionError> {
    let recipe = draft_inline(request, context)?;
    materialize(RecipeSource::Inline(recipe), context)?
        .validate()?
        .admit()
}

fn binding_for(admissible: &AdmissibleExecutionSpec) -> ApprovalBinding {
    let recipe = admissible.spec().recipe.clone();
    ApprovalBinding {
        receipt_id: ApprovalReceiptId::new("receipt-1").unwrap(),
        execution_spec_digest: admissible.digest().clone(),
        container_declaration_digest: recipe.container.declaration_digest,
        policy_revision: recipe.policy.revision.clone(),
        policy_configuration_digest: recipe.policy.configuration_digest,
        approved_cost_micros: recipe.resource_limits.hard_total_cost_micros,
        approved_rollouts: recipe.rollout_plan.maximum_rollouts,
    }
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

#[test]
fn nanohorizon_completes_the_admission_path_without_a_catalog_recipe() {
    let context = context();
    assert!(
        context.catalog.is_empty(),
        "the acceptance request must succeed with an empty catalog"
    );

    let admissible = admit(&request(), &context).expect("NanoHorizon must be admissible inline");
    let disclosure = admissible.approval_disclosure();

    // Provenance is inline, and no catalog id was invented for it.
    assert_eq!(disclosure["sourceKind"], json!("inline"));
    assert!(disclosure["catalogRecipeId"].is_null());

    // Every fact the approval must display and bind.
    assert_eq!(
        disclosure["container"]["containerId"],
        json!("nanohorizon-craftax")
    );
    assert_eq!(
        disclosure["container"]["declarationDigest"],
        json!("sha256:declaration-v1")
    );
    assert_eq!(disclosure["protocol"], json!(LIVE_EVAL_PROTOCOL_V1));
    assert_eq!(disclosure["evaluator"]["kind"], json!("container_declared"));
    assert_eq!(
        disclosure["evaluator"]["evaluatorId"],
        json!("craftax.achievements")
    );
    assert_eq!(disclosure["policy"]["revision"], json!("rev-2026-08-26-a1"));
    assert_eq!(disclosure["model"]["modelId"], json!("z-ai/glm-5.3-flash"));
    assert_eq!(
        disclosure["seeds"],
        json!([780_000, 780_001, 780_002, 780_003, 780_004])
    );
    assert_eq!(disclosure["rolloutCount"], json!(5));
    assert_eq!(disclosure["maximumModelCallsPerRollout"], json!(10));
    assert_eq!(disclosure["maximumStepsPerRollout"], json!(2_000));
    assert_eq!(disclosure["hardTotalCostMicros"], json!(2_450_000));
    assert_eq!(disclosure["hardTotalCostDisplay"], json!("$2.45"));
    assert_eq!(
        disclosure["credentialRoute"]["kind"],
        json!("workshop_secrets_proxy")
    );
    assert!(disclosure["executionSpecDigest"]
        .as_str()
        .is_some_and(|digest| digest.starts_with("sha256:")));

    // The approval binds that exact digest, and execution accepts it.
    let approved = admissible
        .clone()
        .approve(binding_for(&admissible))
        .expect("a receipt describing this specification must be accepted");
    assert_eq!(approved.digest(), admissible.digest());
    assert_eq!(approved.recipe().rollout_plan.declared_rollouts(), 5);
}

#[tokio::test]
async fn paid_approval_receipt_survives_creation_settlement_and_reload() {
    let admissible = admit(&request(), &context()).expect("NanoHorizon must be admissible inline");
    let binding = binding_for(&admissible);
    let receipt_id = binding.receipt_id.as_str().to_string();
    let approved = admissible
        .approve(binding)
        .expect("the exact admitted specification must accept its receipt");
    let (service, _dir, _) = crate::optimizers::service::tests::service().await;
    let create = serde_json::from_value(json!({
        "algorithmId": "eval",
        "id": "approval_receipt_terminal",
        "openVisual": false
    }))
    .unwrap();
    let (run, _) = service
        .create_admitted_eval(create, approved, 5)
        .await
        .unwrap();

    assert_eq!(
        run.usage.extra["paidComputeApproval"]["approvalId"],
        json!(receipt_id)
    );
    assert_eq!(
        run.usage.extra["paidComputeApproval"]["cap"],
        json!({"maxCostUsdMicros": 2_450_000, "maxRollouts": 5})
    );

    service
        .settle_run(
            run.id.clone(),
            crate::optimizers::kernel::SettleCause::Failed {
                detail: "typed pre-dispatch refusal".into(),
            },
            Some(json!({"message": "typed pre-dispatch refusal"})),
        )
        .await
        .unwrap();

    let reloaded = service.get(run.id.clone()).await.unwrap();
    assert_eq!(
        reloaded.usage.extra["paidComputeApproval"]["approvalId"],
        json!(receipt_id)
    );
    let manifest = service
        .terminal_manifest(run.id.clone())
        .await
        .unwrap()
        .expect("settlement must seal a terminal manifest");
    assert_eq!(
        manifest["paidComputeApproval"]["approvalId"],
        json!(receipt_id)
    );

    let authorization_json: String = service
        .database()
        .clone()
        .run(move |conn| {
            conn.query_row(
                "SELECT authorization_json FROM optimizer_run_specs WHERE optimizer_run_id=?1",
                [run.id],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
    let authorization: serde_json::Value = serde_json::from_str(&authorization_json).unwrap();
    assert_eq!(authorization["authorizationRef"], json!(receipt_id));
}

#[test]
fn the_acceptance_run_starts_exactly_five_rollouts_and_settles_truthfully() {
    let admissible = admit(&request(), &context()).unwrap();
    let approved = admissible
        .clone()
        .approve(binding_for(&admissible))
        .unwrap();

    let mut progress = RunProgress::plan(approved.recipe().rollout_plan.declared_rollouts());
    assert_eq!(progress.declared_rollouts(), 5);
    for next in [
        RunState::Validating,
        RunState::ReadyForApproval,
        RunState::AwaitingApproval,
        RunState::Admitted,
        RunState::Starting,
        RunState::Running,
    ] {
        progress.transition_run(next).unwrap();
    }

    let requirements = EvidenceRequirements {
        requires_reward: true,
        requires_trace: true,
        requires_usage: true,
    };
    for index in 0..5u32 {
        for next in [
            RolloutState::Queued,
            RolloutState::Starting,
            RolloutState::Running,
            RolloutState::Completed,
        ] {
            progress.transition_rollout(index, next).unwrap();
        }
        progress.record_evidence(
            index,
            RolloutRecord {
                rollout_id: Some(RolloutId::new(format!("rollout-{index}")).unwrap()),
                reward: Some(0.4),
                trace_ref: Some(format!("trace-{index}")),
                cost_micros: Some(120_000),
                total_tokens: Some(4_096),
                ..RolloutRecord::default()
            },
        );
    }
    progress.credential_revocation_confirmed = true;
    assert_eq!(progress.settle(requirements).unwrap(), RunState::Completed);

    let projected = progress.project(requirements);
    assert_eq!(projected["rolloutStateCounts"]["completed"], json!(5));
    assert_eq!(projected["totalCostMicros"], json!(600_000));
    // Reported spend must stay inside the ceiling that was approved.
    assert!(
        projected["totalCostMicros"].as_u64().unwrap()
            <= approved
                .recipe()
                .resource_limits
                .hard_total_cost_micros
                .as_micros()
    );
}

// ---------------------------------------------------------------------------
// Source selection
// ---------------------------------------------------------------------------

#[test]
fn inline_is_the_default_source_and_never_consults_the_catalog() {
    let context = context();
    let recipe = draft_inline(&request(), &context).unwrap();
    let draft = materialize(RecipeSource::Inline(recipe), &context).unwrap();
    assert_eq!(draft.source_kind(), RecipeSourceKind::Inline);
    assert!(draft.spec().catalog_recipe_id.is_none());
}

#[test]
fn a_missing_catalog_entry_does_not_block_inline_execution() {
    // The catalog is empty and contains nothing resembling this request.
    let context = context();
    assert!(context.catalog.is_empty());
    let admissible = admit(&request(), &context);
    assert!(
        admissible.is_ok(),
        "inline admission must not depend on a catalog entry existing"
    );
}

#[test]
fn an_explicit_catalog_request_stays_fail_closed() {
    let context = context();
    let missing = RecipeId::new("nanohorizon.craftax.glm-5.3-flash.eval.v1").unwrap();
    let error = materialize(
        RecipeSource::Catalog(CatalogRecipeRef {
            recipe_id: missing.clone(),
            expected_digest: None,
        }),
        &context,
    )
    .unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::CatalogRecipeNotFound);
    assert_eq!(error.context["recipeId"], json!(missing.as_str()));
    // It must not have quietly become an inline run.
    assert!(error.remediation.contains("inline"));
}

#[test]
fn catalog_and_inline_sources_produce_the_same_execution_model() {
    let mut context = context();
    let recipe = draft_inline(&request(), &context).unwrap();
    let entry_digest = digest_bytes(b"catalog-entry-1");
    let recipe_id = RecipeId::new("nanohorizon.craftax.v1").unwrap();
    context.catalog.push(CatalogEntry {
        recipe_id: recipe_id.clone(),
        digest: entry_digest.clone(),
        recipe: recipe.clone(),
    });

    let inline = materialize(RecipeSource::Inline(recipe.clone()), &context)
        .unwrap()
        .validate()
        .unwrap()
        .admit()
        .unwrap();
    let catalog = materialize(
        RecipeSource::Catalog(CatalogRecipeRef {
            recipe_id: recipe_id.clone(),
            expected_digest: Some(RecipeDigest::new(entry_digest.as_str()).unwrap()),
        }),
        &context,
    )
    .unwrap()
    .validate()
    .unwrap()
    .admit()
    .unwrap();

    // Identical executable content, and both went through the same validation.
    assert_eq!(inline.spec().recipe, catalog.spec().recipe);
    // Provenance is still recorded truthfully rather than erased.
    assert_eq!(catalog.spec().source_kind, RecipeSourceKind::Catalog);
    assert_eq!(catalog.spec().catalog_recipe_id.as_ref(), Some(&recipe_id));
    assert_ne!(inline.digest(), catalog.digest());
}

#[test]
fn a_catalog_recipe_pinned_to_a_stale_digest_is_refused() {
    let mut context = context();
    let recipe = draft_inline(&request(), &context).unwrap();
    let recipe_id = RecipeId::new("nanohorizon.craftax.v1").unwrap();
    context.catalog.push(CatalogEntry {
        recipe_id: recipe_id.clone(),
        digest: digest_bytes(b"current"),
        recipe,
    });
    let error = materialize(
        RecipeSource::Catalog(CatalogRecipeRef {
            recipe_id,
            expected_digest: Some(RecipeDigest::new(digest_bytes(b"stale").as_str()).unwrap()),
        }),
        &context,
    )
    .unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ExecutionSpecDigestMismatch);
}

// ---------------------------------------------------------------------------
// Construction rules
// ---------------------------------------------------------------------------

#[test]
fn an_undeclared_evaluator_returns_the_precise_error() {
    let mut context = context();
    context.containers[0].declaration.evaluator = None;
    let error = draft_inline(&request(), &context).unwrap_err();
    // Specifically not `catalog_recipe_not_found`: authoring a recipe would
    // not supply the missing declaration.
    assert_eq!(error.code, AdmissionErrorCode::EvaluatorNotDeclared);
    assert_eq!(error.context["containerId"], json!("nanohorizon-craftax"));
}

#[test]
fn a_declared_evaluator_without_a_version_is_a_scoring_contract_failure() {
    let mut context = context();
    if let Some(evaluator) = context.containers[0].declaration.evaluator.as_mut() {
        evaluator.evaluator_version = "  ".into();
    }
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ScoringContractInvalid);
}

#[test]
fn ambiguous_container_discovery_refuses_rather_than_taking_the_first() {
    let mut context = context();
    let mut second = candidate();
    second.container_id = ContainerId::new("nanohorizon-craftax-b").unwrap();
    second.registration_id = ContainerRegistrationId::new("registration-8").unwrap();
    context.containers.push(second);

    let mut ambiguous = request();
    ambiguous.container_id = None; // only a family hint
    let error = draft_inline(&ambiguous, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ContainerSelectionAmbiguous);
    assert_eq!(
        error.context["candidates"],
        json!(["nanohorizon-craftax", "nanohorizon-craftax-b"])
    );
}

#[test]
fn naming_the_exact_container_resolves_an_otherwise_ambiguous_discovery() {
    let mut context = context();
    let mut second = candidate();
    second.container_id = ContainerId::new("nanohorizon-craftax-b").unwrap();
    context.containers.push(second);
    // The request already names the container, so ambiguity does not arise.
    assert!(draft_inline(&request(), &context).is_ok());
}

#[test]
fn an_unhealthy_container_is_refused_before_any_spend() {
    let mut context = context();
    context.containers[0].health = "unhealthy".into();
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ContainerUnhealthy);
    assert_eq!(error.context["observedHealth"], json!("unhealthy"));
}

#[test]
fn an_unknown_health_string_fails_closed_rather_than_reading_as_ready() {
    let mut context = context();
    context.containers[0].health = "degraded-but-probably-fine".into();
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ContainerUnhealthy);
}

#[test]
fn a_container_on_another_protocol_is_not_adapted() {
    let mut context = context();
    context.containers[0].declaration.protocol = Some("synth.container.live-eval.v2".into());
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ContainerProtocolUnsupported);
    assert_eq!(
        error.context["advertisedProtocol"],
        json!("synth.container.live-eval.v2")
    );
}

#[test]
fn a_policy_without_a_resolved_revision_cannot_be_pinned() {
    let mut context = context();
    if let Some(policy) = context.policy.as_mut() {
        policy.revision = None;
    }
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::PolicyRevisionUnresolved);
}

#[test]
fn a_policy_the_session_cannot_resolve_is_not_found() {
    let mut context = context();
    context.policy = None;
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::PolicyNotFound);
}

#[test]
fn an_override_of_a_key_the_policy_does_not_declare_is_refused() {
    let mut context = context();
    let mut with_override = request();
    with_override.policy_overrides = Some(CanonicalJson::new(json!({"temperature": 0.9})).unwrap());
    let error = draft_inline(&with_override, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::PolicyConfigurationInvalid);
    assert!(error.context["detail"]
        .as_str()
        .unwrap()
        .contains("temperature"));
    // An override of a declared key is fine and changes the pinned digest.
    let mut legal = request();
    legal.policy_overrides = Some(CanonicalJson::new(json!({"effort": "high"})).unwrap());
    let overridden = draft_inline(&legal, &context).unwrap();
    let baseline = draft_inline(&request(), &context).unwrap();
    assert_ne!(
        overridden.policy.configuration_digest,
        baseline.policy.configuration_digest
    );
    context.policy = None; // silence unused-mut lint intent
}

#[test]
fn a_model_the_container_does_not_advertise_is_never_substituted() {
    let mut context = context();
    let mut other = request();
    other.model_id = Some(ModelId::new("z-ai/glm-5.3-pro").unwrap());
    let error = draft_inline(&other, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ModelUnsupported);
    assert_eq!(error.context["modelId"], json!("z-ai/glm-5.3-pro"));

    // A container that advertises no models at all cannot be verified, so it
    // also fails rather than accepting anything.
    context.containers[0].declaration.supported_models.clear();
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ModelUnsupported);
}

#[test]
fn a_container_that_cannot_take_seeds_refuses_rather_than_choosing_its_own() {
    let mut context = context();
    context.containers[0].declaration.supports_seed_control = Some(false);
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::SeedControlUnsupported);

    // "Never said" is also not permission.
    context.containers[0].declaration.supports_seed_control = None;
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::SeedControlUnsupported);
}

#[test]
fn unsupported_limits_fail_rather_than_clamp() {
    let context = context();
    for (mutate, limit, requested, ceiling) in [
        (
            Box::new(|request: &mut InlineRequest| {
                request.maximum_model_calls_per_rollout = Some(64)
            }) as Box<dyn Fn(&mut InlineRequest)>,
            "maximum_model_calls_per_rollout",
            64u64,
            32u64,
        ),
        (
            Box::new(|request: &mut InlineRequest| request.maximum_steps_per_rollout = Some(9_000)),
            "maximum_steps_per_rollout",
            9_000,
            4_000,
        ),
        (
            Box::new(|request: &mut InlineRequest| request.maximum_rollouts = Some(128)),
            "maximum_rollouts",
            128,
            64,
        ),
    ] {
        let mut over = request();
        mutate(&mut over);
        let error = draft_inline(&over, &context).unwrap_err();
        assert_eq!(
            error.code,
            AdmissionErrorCode::RequestedLimitUnsupported,
            "{limit} above the declared ceiling must fail"
        );
        assert_eq!(error.context["limit"], json!(limit));
        assert_eq!(error.context["requested"], json!(requested));
        assert_eq!(error.context["maximumSupported"], json!(ceiling));
    }
}

#[test]
fn a_rollout_cap_below_the_seed_count_is_refused_not_silently_truncated() {
    let context = context();
    let mut narrowed = request();
    narrowed.maximum_rollouts = Some(3); // five seeds were supplied
    let error = draft_inline(&narrowed, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::RequestedLimitUnsupported);
}

#[test]
fn paid_execution_without_a_cost_ceiling_is_refused() {
    let context = context();
    let mut unbounded = request();
    unbounded.hard_total_cost_usd = None;
    let error = draft_inline(&unbounded, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::CostCeilingRequired);

    // A zero ceiling is not a ceiling either.
    let mut free = request();
    free.hard_total_cost_usd = Some(0.0);
    let error = draft_inline(&free, &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::CostCeilingRequired);
}

#[test]
fn an_unavailable_credential_route_is_refused_before_admission() {
    let mut context = context();
    context.credential_route_available = false;
    context.credential_route_detail = Some("the secrets proxy is not running".into());
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::CredentialRouteUnavailable);
    assert_eq!(error.context["providerId"], json!("openrouter"));
}

#[test]
fn a_container_missing_a_required_evidence_operation_is_refused() {
    let mut context = context();
    context.containers[0]
        .declaration
        .operations
        .retain(|operation| operation != "reward.get");
    let error = draft_inline(&request(), &context).unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::OutputContractUnsupported);
    assert_eq!(error.context["missingOperations"], json!(["reward.get"]));
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn a_repeated_seed_is_refused_rather_than_deduplicated() {
    let context = context();
    let mut recipe = draft_inline(&request(), &context).unwrap();
    recipe.rollout_plan.seeds[4] = recipe.rollout_plan.seeds[0];
    let error = materialize(RecipeSource::Inline(recipe), &context)
        .unwrap()
        .validate()
        .unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::ExecutionSpecInvalid);
    assert!(error.context["detail"].as_str().unwrap().contains("seed"));
}

#[test]
fn a_policy_configuration_rewritten_after_pinning_fails_validation() {
    let context = context();
    let mut recipe = draft_inline(&request(), &context).unwrap();
    // Rewrite the configuration but leave the recorded digest alone.
    recipe.policy.configuration = CanonicalJson::new(json!({"max_calls": 99})).unwrap();
    let error = materialize(RecipeSource::Inline(recipe), &context)
        .unwrap()
        .validate()
        .unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::PolicyConfigurationInvalid);
}

#[test]
fn a_credential_route_for_another_provider_fails_validation() {
    let context = context();
    let mut recipe = draft_inline(&request(), &context).unwrap();
    recipe.credential_route = CredentialRoute::WorkshopSecretsProxy {
        provider: ProviderId::new("openai").unwrap(),
        capability_scope: CredentialCapabilityScope::new(["chat.completions".to_string()], 900),
    };
    let error = materialize(RecipeSource::Inline(recipe), &context)
        .unwrap()
        .validate()
        .unwrap_err();
    assert_eq!(error.code, AdmissionErrorCode::CredentialRouteUnavailable);
}

// ---------------------------------------------------------------------------
// Approval binding and drift
// ---------------------------------------------------------------------------

#[test]
fn an_approval_receipt_rejects_a_changed_specification() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let binding = binding_for(&admissible);

    // A different specification, approved with the first one's receipt.
    let mut altered = request();
    altered.seeds = (790_000..790_005).map(Seed).collect();
    let other = admit(&altered, &context).unwrap();
    let error = other.approve(binding).unwrap_err();
    assert_eq!(error.code, DriftCode::ApprovedSpecDigestMismatch);
}

#[test]
fn an_approval_receipt_rejects_a_looser_cost_ceiling() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let mut binding = binding_for(&admissible);
    // The operator consented to less than the specification wants to spend.
    binding.approved_cost_micros = CostMicros(NonZeroU64::new(1_000_000).unwrap());
    binding.execution_spec_digest = admissible.digest().clone();
    let error = admissible.approve(binding).unwrap_err();
    assert_eq!(error.code, DriftCode::ApprovalBoundsExceeded);
    assert_eq!(error.context["bound"], json!("hard_total_cost_micros"));
}

#[test]
fn an_approval_receipt_rejects_more_rollouts_than_were_approved() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let mut binding = binding_for(&admissible);
    binding.approved_rollouts = RolloutCount(non_zero_u32(2, "maximum_rollouts").unwrap());
    let error = admissible.approve(binding).unwrap_err();
    assert_eq!(error.code, DriftCode::ApprovalBoundsExceeded);
    assert_eq!(error.context["bound"], json!("maximum_rollouts"));
}

#[test]
fn an_approval_receipt_rejects_a_changed_container_declaration() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let mut binding = binding_for(&admissible);
    binding.container_declaration_digest = DeclarationDigest::new("sha256:declaration-v2").unwrap();
    let error = admissible.approve(binding).unwrap_err();
    assert_eq!(error.code, DriftCode::ContainerDeclarationChanged);
}

#[test]
fn an_approval_receipt_rejects_a_changed_policy_revision() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let mut binding = binding_for(&admissible);
    binding.policy_revision = PolicyRevision::new("rev-2026-08-27-b2").unwrap();
    let error = admissible.approve(binding).unwrap_err();
    assert_eq!(error.code, DriftCode::PolicyRevisionChanged);
}

#[test]
fn drift_detected_at_dispatch_demands_a_new_approval_rather_than_a_patch() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let approved = admissible
        .clone()
        .approve(binding_for(&admissible))
        .unwrap();

    let source_error = approved
        .reverify(
            &SourceRevision::new("harbor-runtime-v2").unwrap(),
            &approved.recipe().container.declaration_digest,
            &approved.recipe().policy.revision,
        )
        .unwrap_err();
    assert_eq!(source_error.code, DriftCode::ContainerSourceChanged);

    // The declaration moved between approval and dispatch.
    let error = approved
        .reverify(
            &approved.recipe().container.source_revision,
            &DeclarationDigest::new("sha256:declaration-v2").unwrap(),
            &approved.recipe().policy.revision,
        )
        .unwrap_err();
    assert_eq!(error.code, DriftCode::ContainerDeclarationChanged);
    let rendered = error.to_json();
    assert!(rendered["remediation"]
        .as_str()
        .unwrap()
        .contains("new paid-compute approval"));

    // Unchanged inputs re-verify cleanly.
    approved
        .reverify(
            &approved.recipe().container.source_revision.clone(),
            &approved.recipe().container.declaration_digest.clone(),
            &approved.recipe().policy.revision.clone(),
        )
        .expect("unchanged inputs must still dispatch");
}

#[test]
fn a_receipt_is_never_reusable_across_two_admissions_of_the_same_request() {
    let context = context();
    let first = admit(&request(), &context).unwrap();
    let second = admit(&request(), &context).unwrap();
    // The same request admits to the same digest, which is what makes the
    // specification reproducible...
    assert_eq!(first.digest(), second.digest());
    // ...but each run still takes its own receipt; nothing here mints one, and
    // the executor only ever holds the receipt it was approved with.
    let binding = binding_for(&first);
    let approved = second.approve(binding.clone()).unwrap();
    assert_eq!(approved.receipt_id(), &binding.receipt_id);
}

// ---------------------------------------------------------------------------
// Reuse
// ---------------------------------------------------------------------------

#[test]
fn a_persisted_specification_reopens_and_readmits_without_its_old_receipt() {
    let context = context();
    let admissible = admit(&request(), &context).unwrap();
    let stored = serde_json::to_string(admissible.spec()).unwrap();

    // Reopening is a plain deserialization of the canonical specification.
    let reopened: ExecutionSpec = serde_json::from_str(&stored).unwrap();
    assert_eq!(&reopened, admissible.spec());

    // Reuse re-enters admission from the top and produces a fresh admissible
    // specification that must be approved again.
    let readmitted = materialize(RecipeSource::Inline(reopened.recipe.clone()), &context)
        .unwrap()
        .validate()
        .unwrap()
        .admit()
        .unwrap();
    assert_eq!(readmitted.digest(), admissible.digest());
}

#[test]
fn unknown_cost_fields_fail_at_schema_validation() {
    let error = serde_json::from_value::<InlineRequest>(json!({
        "policyNamespace": "nanohorizon",
        "policyName": "glm-5.3-flash",
        "provider": "openrouter",
        "modelId": "z-ai/glm-5.3-flash",
        "seeds": [780005],
        "maximumRollouts": 1,
        "maximumModelCallsPerRollout": 10,
        "maximumStepsPerRollout": 2000,
        "costCeilingUsd": 2.45
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `costCeilingUsd`"), "{error}");
}

#[test]
fn host_session_fields_are_stripped_before_inline_request_parse() {
    let request = InlineRequest::from_tool_arguments(json!({
        "policyNamespace": "nanohorizon",
        "policyName": "glm-5.3-flash",
        "provider": "openrouter",
        "modelId": "z-ai/glm-5.3-flash",
        "seeds": [780005],
        "maximumRollouts": 1,
        "maximumModelCallsPerRollout": 10,
        "maximumStepsPerRollout": 2000,
        "hardTotalCostUsd": 2.45,
        "sessionRef": "330175fd-not-a-spec-field",
        "openVisual": true
    }))
    .unwrap();
    assert_eq!(request.hard_total_cost_usd, Some(2.45));
    assert_eq!(request.maximum_model_calls_per_rollout, Some(10));
    assert_eq!(request.maximum_steps_per_rollout, Some(2_000));
}

#[test]
fn policy_source_bytes_do_not_fork_the_canonical_digest() {
    let bytes = "fn act(): pass";
    let material = PolicyMaterialRef {
        source_root: "/GitHub/nanohorizon".into(),
        repository_relative_path: "src/challenge/policy.py".into(),
        tracked_revision: "rev-2026-08-26-a1".into(),
        content_digest: digest_bytes(bytes.as_bytes()),
    };
    let mut with_bytes = context();
    with_bytes.policy.as_mut().unwrap().source_code = Some(bytes.into());
    with_bytes.policy.as_mut().unwrap().material = Some(material.clone());
    let mut without_bytes = context();
    without_bytes.policy.as_mut().unwrap().material = Some(material);
    let without = admit(&request(), &without_bytes).unwrap();
    let with = admit(&request(), &with_bytes).unwrap();
    assert_eq!(
        without.digest(),
        with.digest(),
        "in-memory policy bytes must not change the approved digest"
    );
    assert_eq!(
        with.spec()
            .recipe
            .policy
            .material
            .as_ref()
            .unwrap()
            .content_digest,
        digest_bytes(bytes.as_bytes())
    );
}

#[test]
fn approved_limits_remain_non_null_through_admission() {
    let admissible = admit(&request(), &context()).unwrap();
    let limits = &admissible.spec().recipe.resource_limits;
    assert_eq!(limits.maximum_model_calls_per_rollout.0.get(), 10);
    assert_eq!(limits.maximum_steps_per_rollout.0.get(), 2_000);
    assert_eq!(limits.hard_total_cost_micros.as_micros(), 2_450_000);
    let approved = admissible
        .clone()
        .approve(binding_for(&admissible))
        .unwrap();
    assert_eq!(
        approved
            .recipe()
            .resource_limits
            .maximum_model_calls_per_rollout
            .0
            .get(),
        10
    );
    assert_eq!(approved.digest(), admissible.digest());
}
