import {
	dropProps,
	engageCommit,
	engageEyes,
	idleBlink,
	idleInhale,
	idleRest,
	openEyes,
	reducedThinking,
	restPose,
	settleEyes,
	settleFollow,
	successBounce,
	successHop,
	successLand,
	successRest,
	successTwinkle,
	thinkingGlance,
	thinkingRest,
	thinkingSway,
	workingBurst,
	workingLean,
	workingPush,
	workingRest,
	workingStride
} from "./Mander.poses";
import { MANDER_STATES, type ManderState, type TransitionKey, type TransitionRecipe } from "./Mander.types";

function loop(durationMs: number, keyframes: TransitionRecipe["keyframes"]): TransitionRecipe {
	return { durationMs, loop: true, kind: "loop", keyframes };
}

function directed(durationMs: number, keyframes: TransitionRecipe["keyframes"]): TransitionRecipe {
	return { durationMs, loop: false, kind: "directed", keyframes };
}

function hold(pose: ReturnType<typeof restPose>): TransitionRecipe {
	return directed(1, [{ at: 1, pose, ease: "linear" }]);
}

function snap(pose: ReturnType<typeof restPose>, durationMs = 160): TransitionRecipe {
	return directed(durationMs, [{ at: 1, pose, ease: "easeOut" }]);
}

const idleLoop = loop(4200, [
	{ at: 0, pose: idleRest, ease: "easeInOut" },
	{ at: 0.38, pose: idleInhale, ease: "easeInOut" },
	{ at: 0.72, pose: idleRest, ease: "easeInOut" },
	{ at: 0.84, pose: idleRest, ease: "linear" },
	{ at: 0.875, pose: idleBlink, ease: "easeInOut" },
	{ at: 0.91, pose: idleRest, ease: "easeOut" },
	{ at: 1, pose: idleRest, ease: "linear" }
]);

const thinkingLoop = loop(1800, [
	{ at: 0, pose: thinkingRest, ease: "easeInOut" },
	{ at: 0.28, pose: thinkingGlance, ease: "easeInOut" },
	{ at: 0.58, pose: thinkingSway, ease: "easeInOut" },
	{ at: 1, pose: thinkingRest, ease: "easeInOut" }
]);

const workingLoop = loop(720, [
	{ at: 0, pose: workingRest, ease: "easeInOut" },
	{ at: 0.32, pose: workingStride, ease: "easeInOut" },
	{ at: 0.68, pose: workingPush, ease: "easeInOut" },
	{ at: 1, pose: workingRest, ease: "easeInOut" }
]);

const successLoop = loop(1400, [
	{ at: 0, pose: successRest, ease: "easeInOut" },
	{ at: 0.34, pose: successTwinkle, ease: "easeInOut" },
	{ at: 0.72, pose: successBounce, ease: "easeInOut" },
	{ at: 1, pose: successRest, ease: "easeInOut" }
]);

const idleToThinking = directed(440, [
	{ at: 0.28, pose: engageEyes, ease: "easeOut" },
	{ at: 0.72, pose: engageCommit, ease: "easeOutBack" },
	{ at: 1, pose: thinkingRest, ease: "easeOut" }
]);

const thinkingToIdle = directed(380, [
	{ at: 0.24, pose: settleEyes, ease: "easeOut" },
	{ at: 0.7, pose: settleFollow, ease: "easeInOut" },
	{ at: 1, pose: idleRest, ease: "easeOut" }
]);

const idleToWorking = directed(360, [
	{ at: 0.35, pose: workingLean, ease: "easeOut" },
	{ at: 0.78, pose: workingBurst, ease: "easeOut" },
	{ at: 1, pose: workingRest, ease: "easeOut" }
]);

const thinkingToWorking = directed(420, [
	{ at: 0.28, pose: dropProps, ease: "easeOut" },
	{ at: 0.62, pose: workingLean, ease: "easeOut" },
	{ at: 1, pose: workingRest, ease: "easeOut" }
]);

const workingToIdle = directed(340, [
	{ at: 0.4, pose: workingLean, ease: "easeOut" },
	{ at: 1, pose: idleRest, ease: "easeOut" }
]);

const workingToThinking = directed(400, [
	{ at: 0.3, pose: workingLean, ease: "easeOut" },
	{ at: 0.68, pose: engageCommit, ease: "easeOut" },
	{ at: 1, pose: thinkingRest, ease: "easeOut" }
]);

const idleToSuccess = directed(420, [
	{ at: 0.38, pose: successHop, ease: "easeOut" },
	{ at: 0.72, pose: successLand, ease: "easeInOut" },
	{ at: 1, pose: successRest, ease: "easeOut" }
]);

const thinkingToSuccess = directed(440, [
	{ at: 0.24, pose: dropProps, ease: "easeOut" },
	{ at: 0.58, pose: successHop, ease: "easeOut" },
	{ at: 1, pose: successRest, ease: "easeOut" }
]);

const workingToSuccess = directed(420, [
	{ at: 0.28, pose: workingLean, ease: "easeOut" },
	{ at: 0.62, pose: successHop, ease: "easeOut" },
	{ at: 1, pose: successRest, ease: "easeOut" }
]);

const successToIdle = directed(360, [
	{ at: 0.32, pose: openEyes, ease: "easeOut" },
	{ at: 0.7, pose: successLand, ease: "easeInOut" },
	{ at: 1, pose: idleRest, ease: "easeOut" }
]);

const successToThinking = directed(400, [
	{ at: 0.28, pose: openEyes, ease: "easeOut" },
	{ at: 0.68, pose: engageCommit, ease: "easeOut" },
	{ at: 1, pose: thinkingRest, ease: "easeOut" }
]);

const successToWorking = directed(380, [
	{ at: 0.3, pose: openEyes, ease: "easeOut" },
	{ at: 0.68, pose: workingBurst, ease: "easeOut" },
	{ at: 1, pose: workingRest, ease: "easeOut" }
]);

export const transitions: Record<TransitionKey, TransitionRecipe> = {
	"idle->idle": idleLoop,
	"idle->thinking": idleToThinking,
	"idle->working": idleToWorking,
	"idle->success": idleToSuccess,
	"thinking->idle": thinkingToIdle,
	"thinking->thinking": thinkingLoop,
	"thinking->working": thinkingToWorking,
	"thinking->success": thinkingToSuccess,
	"working->idle": workingToIdle,
	"working->thinking": workingToThinking,
	"working->working": workingLoop,
	"working->success": workingToSuccess,
	"success->idle": successToIdle,
	"success->thinking": successToThinking,
	"success->working": successToWorking,
	"success->success": successLoop
};

export const reducedTransitions: Record<TransitionKey, TransitionRecipe> = Object.fromEntries(
	MANDER_STATES.flatMap((from) =>
		MANDER_STATES.map((to) => {
			const key = `${from}->${to}` as TransitionKey;
			if (from === to) return [key, hold(to === "thinking" ? reducedThinking : restPose(to))];
			return [key, snap(to === "thinking" ? reducedThinking : restPose(to), from === to ? 1 : 160)];
		})
	)
) as Record<TransitionKey, TransitionRecipe>;

export function transitionKey(from: ManderState, to: ManderState): TransitionKey {
	return `${from}->${to}`;
}

export function recipeFor(key: TransitionKey, reduced: boolean): TransitionRecipe {
	return reduced ? reducedTransitions[key] : transitions[key];
}
