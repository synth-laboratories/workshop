export type WorkshopStarter = {
	id: "craftax-first-eval" | "nanohorizon-craftax";
	title: string;
	description: string;
	recipeId: string;
	flow: readonly string[];
	maxCostUsd: number;
	prompt: string;
};

/** Product-owned starter identities. Runtime recipe admission remains authoritative. */
export const WORKSHOP_STARTERS: readonly WorkshopStarter[] = [
	{
		id: "craftax-first-eval",
		title: "Craftax first evaluation",
		description: "Stage a small policy comparison, run a bounded local evaluation, and inspect its scorecard and trace.",
		recipeId: "eval.craftax.code-policy.smoke.v1",
		flow: ["Preflight", "Evaluate", "Inspect"],
		maxCostUsd: 0.3,
		prompt: "Set up the Workshop recipe eval.craftax.code-policy.smoke.v1. Do not start compute yet. First show its live admission checks, pinned container digest, candidate paths, trials, models, and maximum cost. Wait for my explicit approval, then stage exactly one baseline and one candidate, run the recipe, and open the result visual and evidence directory."
	},
	{
		id: "nanohorizon-craftax",
		title: "Explore NanoHorizon",
		description: "Reproduce the five-seed Craftax evaluation with its pinned policy, rules, cost ceiling, traces, and visual evidence.",
		recipeId: "nanohorizon.craftax.glm-5.3-flash.eval.v1",
		flow: ["Verify kit", "Run 5 seeds", "Compare"],
		maxCostUsd: 2.45,
		prompt: "Set up the exact NanoHorizon recipe nanohorizon.craftax.glm-5.3-flash.eval.v1. Do not start compute yet. If NanoHorizon is absent, clone only the public https://github.com/synth-laboratories/nanohorizon repository into this workspace, then inspect and pin its declared recipe and source revision. Verify the declared container, GLM 5.3 Flash policy, five seeds, 180 second timeout, five-rollout limit, and $2.45 maximum cost. Show every failed preflight check and stop for my explicit approval before provider use or any run. Never substitute another Craftax or GEPA recipe."
	}
] as const;

export function workshopStarter(id: string | null | undefined): WorkshopStarter | null {
	return WORKSHOP_STARTERS.find((starter) => starter.id === id) ?? null;
}

/** Recipe identity is the only authority that can bind a run to a starter. */
export function workshopStarterForRecipe(recipeId: string | null | undefined): WorkshopStarter | null {
	return WORKSHOP_STARTERS.find((starter) => starter.recipeId === recipeId) ?? null;
}

/** A referral can recommend setup copy only for the exact selected recipe. */
export function starterPromptForRecipe(
	starter: WorkshopStarter | null,
	recipeId: string,
	fallback: string
): string {
	return starter?.recipeId === recipeId ? starter.prompt : fallback;
}
