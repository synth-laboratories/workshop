import assert from "node:assert/strict";
import test from "node:test";
import {
	hostedLifecycleLabel,
	hostedCooldownLabel,
	parseHostedInferenceLifecycle
} from "../src/renderer/src/runtime/hostedInferenceLifecycle.ts";

test("parses Shoal v3 lifecycle and cooldown without inventing fields", () => {
	assert.deepEqual(parseHostedInferenceLifecycle({
		inference_lifecycle: {
			protocol_version: "synth.inference.lifecycle.v3",
			phase: "warming",
			reason: "live provider observation",
			observed_at: 42,
			warm_operation_id: "warm-1",
			cold_start_deadline_seconds: 90,
			cooldown: {
				policy: "idle_scale_to_zero",
				idle_timeout_seconds: 300,
				last_activity_at: 100,
				warm_until: 400
			}
		}
	}), {
		protocolVersion: "synth.inference.lifecycle.v3",
		phase: "warming",
		reason: "live provider observation",
		observedAt: 42,
		warmOperationId: "warm-1",
		coldStartDeadlineSeconds: 90,
		cooldown: {
			policy: "idle_scale_to_zero",
			idleTimeoutSeconds: 300,
			lastActivityAt: 100,
			warmUntil: 400
		}
	});
});

test("renders an authoritative hosted cooldown without pretending provider-managed timing", () => {
	const lifecycle = parseHostedInferenceLifecycle({ inference_lifecycle: {
		protocol_version: "synth.inference.lifecycle.v3",
		phase: "ready",
		cooldown: { policy: "idle_scale_to_zero", warm_until: 400 }
	} });
	assert.equal(hostedCooldownLabel(lifecycle, 100_000), "Hosted model warm · scales down in 5m");
	assert.equal(hostedCooldownLabel({ ...lifecycle, cooldown: { ...lifecycle.cooldown, policy: "provider_managed", warmUntil: null } }), "Hosted model ready");
});

test("maps hosted phases to specific live copy", () => {
	assert.equal(hostedLifecycleLabel("queued"), "Waiting for capacity…");
	assert.equal(hostedLifecycleLabel("provisioning"), "Starting cloud GPU…");
	assert.equal(hostedLifecycleLabel("warming"), "Warming model…");
	assert.equal(hostedLifecycleLabel("running"), "Generating…");
	assert.equal(hostedLifecycleLabel("mystery"), null);
});
