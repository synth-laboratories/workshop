import { MANDER_PARTS, type ManderPart, type ManderPose, type ManderState, type PartPose } from "./Mander.types";

export const IDENTITY_PART: PartPose = {
	x: 0,
	y: 0,
	scaleX: 1,
	scaleY: 1,
	rotation: 0,
	opacity: 1
};

export function part(overrides: Partial<PartPose> = {}): PartPose {
	return { ...IDENTITY_PART, ...overrides };
}

export function pose(overrides: Partial<Record<ManderPart, Partial<PartPose>>> = {}): ManderPose {
	const next = {
		body: part(),
		leftEye: part(),
		rightEye: part(),
		leftFeature: part(),
		rightFeature: part(),
		forelimb: part(),
		thought: part({ opacity: 0 }),
		streaks: part({ opacity: 0 }),
		sparkle: part({ opacity: 0 })
	};
	for (const key of MANDER_PARTS) {
		if (overrides[key]) next[key] = part({ ...next[key], ...overrides[key] });
	}
	return next;
}

const closedEyes = { scaleX: 1.4, scaleY: 0.35, y: 0 } as const;

export const idleRest = pose({
	body: { y: 0 }
});

export const thinkingRest = pose({
	body: { y: -1 },
	leftEye: { x: 1, y: 1 },
	rightEye: { x: 1, y: 1 },
	leftFeature: { x: -1 },
	rightFeature: { x: 1 },
	forelimb: { x: -6, y: -7 },
	thought: { opacity: 1 }
});

export const workingRest = pose({
	body: { x: -2 },
	leftEye: { x: -1 },
	rightEye: { x: -1 },
	leftFeature: { x: 1, y: 1 },
	rightFeature: { x: 2 },
	forelimb: { y: 1 },
	streaks: { opacity: 1 }
});

export const successRest = pose({
	body: { y: -1 },
	leftEye: closedEyes,
	rightEye: closedEyes,
	forelimb: { x: 1, y: 1 },
	sparkle: { opacity: 1 }
});

const REST: Record<ManderState, ManderPose> = {
	idle: idleRest,
	thinking: thinkingRest,
	working: workingRest,
	success: successRest
};

export function restPose(state: ManderState): ManderPose {
	return REST[state];
}

export const idleInhale = pose({
	body: { y: -1 }
});

export const idleBlink = pose({
	leftEye: { scaleY: 0.5 },
	rightEye: { scaleY: 0.5 }
});

export const thinkingGlance = pose({
	body: { y: -1 },
	leftEye: { x: 1, y: 0 },
	rightEye: { x: 1, y: 0 },
	leftFeature: { x: -1 },
	rightFeature: { x: 1 },
	forelimb: { x: -6, y: -8 },
	thought: { y: -1, opacity: 1 }
});

export const thinkingSway = pose({
	body: { y: -1 },
	leftEye: { x: 1, y: 1 },
	rightEye: { x: 1, y: 1 },
	leftFeature: { x: -1 },
	rightFeature: { x: 1 },
	forelimb: { x: -5, y: -6 },
	thought: { y: 1, opacity: 1 }
});

export const engageEyes = pose({
	leftEye: { x: 1, y: 1 },
	rightEye: { x: 1, y: 1 },
	thought: { y: 2, opacity: 0.4 }
});

export const engageCommit = pose({
	body: { y: -1 },
	leftEye: { x: 1, y: 1 },
	rightEye: { x: 1, y: 1 },
	leftFeature: { x: -1 },
	rightFeature: { x: 1 },
	forelimb: { x: -7, y: -8 },
	thought: { y: -1, opacity: 1 }
});

export const settleEyes = pose({
	body: { y: -1 },
	leftFeature: { x: -1 },
	rightFeature: { x: 1 },
	forelimb: { x: -6, y: -7 },
	thought: { y: 1, opacity: 0.25 }
});

export const settleFollow = pose({
	body: { y: 0 },
	forelimb: { x: -2, y: -3 },
	thought: { y: 2, opacity: 0 }
});

export const reducedThinking = pose({
	leftEye: { x: 1, y: 1, opacity: 1 },
	rightEye: { x: 1, y: 1, opacity: 1 },
	forelimb: { x: -6, y: -7 },
	thought: { opacity: 1 }
});

export const workingStride = pose({
	body: { x: -3, y: -1 },
	leftEye: { x: -1 },
	rightEye: { x: -1 },
	leftFeature: { x: 2, y: 1 },
	rightFeature: { x: 3 },
	forelimb: { x: -1, y: -2 },
	streaks: { x: 2, opacity: 1 }
});

export const workingPush = pose({
	body: { x: -1, y: 1 },
	leftEye: { x: -1 },
	rightEye: { x: -1 },
	leftFeature: { x: 1 },
	rightFeature: { x: 1 },
	forelimb: { y: 2 },
	streaks: { x: -1, opacity: 0.65 }
});

export const workingLean = pose({
	body: { x: -2 },
	leftEye: { x: -1 },
	rightEye: { x: -1 },
	leftFeature: { x: 1 },
	rightFeature: { x: 1 },
	forelimb: { y: 1 }
});

export const workingBurst = pose({
	body: { x: -3, y: -1 },
	leftEye: { x: -1 },
	rightEye: { x: -1 },
	leftFeature: { x: 2, y: 1 },
	rightFeature: { x: 2 },
	forelimb: { y: -1 },
	streaks: { x: 1, opacity: 1 }
});

export const dropProps = pose({
	body: { y: -1 },
	forelimb: { x: -2, y: -3 },
	thought: { y: 2, opacity: 0 },
	streaks: { opacity: 0 },
	sparkle: { opacity: 0 }
});

export const successHop = pose({
	body: { y: -3 },
	leftEye: closedEyes,
	rightEye: closedEyes,
	forelimb: { x: 1, y: 0 },
	sparkle: { y: -1, opacity: 1 }
});

export const successTwinkle = pose({
	body: { y: -4 },
	leftEye: closedEyes,
	rightEye: closedEyes,
	forelimb: { x: 2, y: -2 },
	sparkle: { x: 2, y: -3, opacity: 1 }
});

export const successBounce = pose({
	body: { y: 1 },
	leftEye: closedEyes,
	rightEye: closedEyes,
	forelimb: { x: 1, y: 2 },
	sparkle: { x: -2, y: 2, opacity: 0.85 }
});

export const successLand = pose({
	body: { y: 0 },
	leftEye: closedEyes,
	rightEye: closedEyes,
	sparkle: { y: 1, opacity: 0.9 }
});

export const openEyes = pose({
	body: { y: -1 },
	sparkle: { y: 2, opacity: 0.3 }
});

export function flattenPose(value: ManderPose): number[] {
	const out: number[] = [];
	for (const key of MANDER_PARTS) {
		const item = value[key];
		out.push(item.x, item.y, item.scaleX, item.scaleY, item.rotation, item.opacity);
	}
	return out;
}

export function unflattenPose(values: number[]): ManderPose {
	if (values.length !== MANDER_PARTS.length * 6) {
		throw new Error(`Mander pose vector length ${values.length} does not match ${MANDER_PARTS.length * 6}`);
	}
	const next = pose();
	MANDER_PARTS.forEach((key, index) => {
		const offset = index * 6;
		next[key] = {
			x: values[offset]!,
			y: values[offset + 1]!,
			scaleX: values[offset + 2]!,
			scaleY: values[offset + 3]!,
			rotation: values[offset + 4]!,
			opacity: values[offset + 5]!
		};
	});
	return next;
}

export function lerpPose(from: ManderPose, to: ManderPose, t: number): ManderPose {
	const a = flattenPose(from);
	const b = flattenPose(to);
	return unflattenPose(a.map((value, index) => value + (b[index]! - value) * t));
}

export function poseDistance(from: ManderPose, to: ManderPose): number {
	const a = flattenPose(from);
	const b = flattenPose(to);
	return Math.hypot(...a.map((value, index) => value - b[index]!));
}

export function clonePose(value: ManderPose): ManderPose {
	return unflattenPose(flattenPose(value));
}
