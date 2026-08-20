import type { SavedLoraCheckpoint } from "../bridge/types";

/** Laguna Composer loads Laguna XS 2.1. Qwen Optimizers SFT adapters stay off this picker. */
export function isLagunaCompatibleAdapter(checkpoint: SavedLoraCheckpoint): boolean {
	const base = checkpoint.baseModel.toLowerCase();
	return checkpoint.placement === "this_mac"
		&& checkpoint.checkpointKind === "inference"
		&& checkpoint.status === "ready"
		&& (base.includes("laguna") || base.includes("poolside"));
}

export type LagunaAdapterOption = {
	checkpointId: string;
	name: string;
};
