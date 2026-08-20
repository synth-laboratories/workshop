import type { MlxReadiness, ModelInstallPlan, SavedLoraCheckpoint, TrainingArtifact, TrainingArtifactsBridge } from "../bridge";
import { bridges } from "./desktopBridge";

const MODEL_ID = "Qwen/Qwen3.5-0.8B";
const MODEL_REVISION = "2fc06364715b967f1860aea9cf38778875588b17";

function metadataString(checkpoint: SavedLoraCheckpoint, key: string): string | null {
	const value = checkpoint.metadata[key];
	return typeof value === "string" && value.length > 0 ? value : null;
}

function asArtifact(checkpoint: SavedLoraCheckpoint): TrainingArtifact | null {
	const algorithm = checkpoint.lineage.optimizerAlgorithm ?? checkpoint.optimizerAlgorithm;
	if (algorithm !== "sft" && algorithm !== "cispo") return null;
	const runId = checkpoint.lineage.runId ?? checkpoint.runId;
	if (!runId) return null;
	return {
		id: checkpoint.checkpointId,
		kind: checkpoint.provider === "imported" || checkpoint.storage.backend === "minio" ? "mlx-lora.v1" : checkpoint.checkpointKind === "training" ? "training-checkpoint.v1" : "hosted-lora.v1",
		algorithm,
		baseModel: { id: checkpoint.baseModel, revision: metadataString(checkpoint, "baseModelRevision") },
		producingRunId: runId,
		datasetDigest: metadataString(checkpoint, "datasetDigest"),
		configDigest: metadataString(checkpoint, "configDigest"),
		sha256: checkpoint.storage.sha256 ?? null,
		sizeBytes: checkpoint.storage.sizeBytes ?? null,
		integrity: checkpoint.status === "ready" && checkpoint.storage.sha256 ? "verified" : checkpoint.status === "failed" ? "failed" : checkpoint.status === "uploading" ? "pending" : "unknown",
		compatibleBackends: checkpoint.checkpointKind === "inference" ? ["MLX inference", "Local Eval"] : ["MLX training"],
		parentArtifactId: checkpoint.lineage.sourceCheckpointId ?? checkpoint.sourceCheckpointId ?? null
	};
}

export async function inspectMlxReadiness(): Promise<MlxReadiness> {
	const appleSilicon = /Mac/.test(navigator.platform) && navigator.maxTouchPoints === 0;
	return {
		platform: appleSilicon ? "apple_silicon" : "unknown",
		compatibility: appleSilicon ? "compatible" : "unknown",
		runtimeHealth: bridges.trainingModels ? "ready" : "missing",
		runtimeVersion: null,
		availableMemoryBytes: null,
		availableDiskBytes: null,
		failureClass: bridges.trainingModels ? null : "runtime"
	};
}

export async function planModelInstall(): Promise<ModelInstallPlan> {
	const installed = await bridges.trainingModels?.listModels() ?? [];
	return {
		modelId: MODEL_ID,
		title: "Qwen 3.5 0.8B (MLX training)",
		source: MODEL_ID,
		revision: MODEL_REVISION,
		digest: null,
		license: "Apache-2.0",
		downloadBytes: 1_750_000_000,
		minimumFreeDiskBytes: 3 * 1024 ** 3,
		alreadyPresent: installed.some((hit) => hit.modelId === MODEL_ID && hit.revision === MODEL_REVISION),
		compatible: true
	};
}

export const trainingArtifacts: TrainingArtifactsBridge = {
	async list() {
		if (!bridges.optimizers) return [];
		const page = await bridges.optimizers.searchSavedLoras({ scope: "all", status: "ready", limit: 200 });
		return page.items.map(asArtifact).filter((item): item is TrainingArtifact => item !== null);
	},
	async inspect(id) {
		const artifact = (await this.list()).find((item) => item.id === id);
		if (!artifact) throw new Error(`Training artifact ${id} is unavailable`);
		return artifact;
	},
	async launchInference(id) {
		await this.inspect(id);
		return { artifactId: id, status: "planned" };
	},
	async launchEval(id, recipeId) {
		await this.inspect(id);
		return { artifactId: id, recipeId, status: "planned" };
	},
	async delete(id) {
		if (!bridges.optimizers) throw new Error("Training artifact deletion requires Synth Desktop");
		await bridges.optimizers.archiveSavedLora(id);
	}
};
