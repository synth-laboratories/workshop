import { bridges } from "./desktopBridge";

export type MlxReadiness = {
	platform: "apple_silicon" | "unknown";
	compatibility: "compatible" | "unknown";
	runtimeHealth: "ready" | "missing" | "unhealthy";
	runtimeVersion: string | null;
	availableMemoryBytes: number | null;
	availableDiskBytes: number | null;
	failureClass: "runtime" | null;
};

export type ModelInstallPlan = {
	modelId: string;
	title: string;
	source: string;
	revision: string;
	digest: string | null;
	license: string;
	downloadBytes: number;
	minimumFreeDiskBytes: number;
	alreadyPresent: boolean;
	compatible: boolean;
};

const MODEL_ID = "Qwen/Qwen3.5-2B";
const MODEL_REVISION = "15852e8c16360a2fea060d615a32b45270f8a8fc";

export async function inspectMlxReadiness(): Promise<MlxReadiness> {
	const appleSilicon = /Mac/.test(navigator.platform) && navigator.maxTouchPoints === 0;
	if (!bridges.trainingModels) {
		return { platform: appleSilicon ? "apple_silicon" : "unknown", compatibility: appleSilicon ? "compatible" : "unknown", runtimeHealth: "missing", runtimeVersion: null, availableMemoryBytes: null, availableDiskBytes: null, failureClass: "runtime" };
	}
	try {
		const runtime = await bridges.trainingModels.runtimeStatus();
		if (!runtime.installed) {
			return { platform: appleSilicon ? "apple_silicon" : "unknown", compatibility: appleSilicon ? "compatible" : "unknown", runtimeHealth: "missing", runtimeVersion: runtime.version, availableMemoryBytes: null, availableDiskBytes: null, failureClass: "runtime" };
		}
		await bridges.trainingModels.listModels();
		return {
			platform: appleSilicon ? "apple_silicon" : "unknown",
			compatibility: appleSilicon ? "compatible" : "unknown",
			runtimeHealth: "ready",
			runtimeVersion: runtime.version,
			availableMemoryBytes: null,
			availableDiskBytes: null,
			failureClass: null
		};
	} catch {
		return { platform: appleSilicon ? "apple_silicon" : "unknown", compatibility: appleSilicon ? "compatible" : "unknown", runtimeHealth: "unhealthy", runtimeVersion: null, availableMemoryBytes: null, availableDiskBytes: null, failureClass: "runtime" };
	}
}

export async function planModelInstall(): Promise<ModelInstallPlan> {
	const installed = await bridges.trainingModels?.listModels() ?? [];
	return {
		modelId: MODEL_ID,
		title: "Qwen 3.5 2B (MLX training)",
		source: MODEL_ID,
		revision: MODEL_REVISION,
		digest: null,
		license: "Apache-2.0",
		downloadBytes: 4_500_000_000,
		minimumFreeDiskBytes: 7 * 1024 ** 3,
		alreadyPresent: installed.some((hit) => hit.modelId === MODEL_ID && hit.revision === MODEL_REVISION),
		compatible: true
	};
}

export const trainingArtifacts = {
	async list() {
		return await bridges.trainingArtifacts?.list() ?? [];
	},
	async inspect(id: string) {
		if (!bridges.trainingArtifacts) throw new Error("Training artifact storage is unavailable");
		return bridges.trainingArtifacts.get(id);
	},
	async launchInference(id: string) {
		if (!bridges.trainingArtifacts) throw new Error("Training artifact inference is unavailable");
		return bridges.trainingArtifacts.launchInference({ id, confirm: true });
	},
	async launchEval(id: string, recipeId: string) {
		if (!bridges.optimizers) throw new Error("Native artifact Eval is unavailable; no run was started");
		await this.inspect(id);
		return bridges.optimizers.startRecipe({ recipeId, trainingArtifactId: id, openVisual: true });
	},
	async export(id: string, destination: string) {
		if (!bridges.trainingArtifacts?.export) throw new Error("Training artifact export is unavailable");
		return bridges.trainingArtifacts.export({ id, destination, confirm: true });
	},
	async delete(id: string) {
		if (!bridges.trainingArtifacts?.delete) throw new Error("Training artifact deletion is unavailable; no artifact was changed");
		return bridges.trainingArtifacts.delete({ id, confirm: true });
	}
};
