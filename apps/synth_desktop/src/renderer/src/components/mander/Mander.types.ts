export const MANDER_STATES = ["idle", "thinking", "working", "success"] as const;
export type ManderState = (typeof MANDER_STATES)[number];
export type ManderMotion = "auto" | "full" | "reduced" | "still";
export type ResolvedManderMotion = Exclude<ManderMotion, "auto">;

export type ManderProps = {
	state: ManderState;
	size?: number;
	motion?: ManderMotion;
	className?: string;
	label?: string;
};

export type PartPose = {
	x: number;
	y: number;
	scaleX: number;
	scaleY: number;
	rotation: number;
	opacity: number;
};

export type ManderPose = {
	body: PartPose;
	leftEye: PartPose;
	rightEye: PartPose;
	leftFeature: PartPose;
	rightFeature: PartPose;
	forelimb: PartPose;
	thought: PartPose;
	streaks: PartPose;
	sparkle: PartPose;
};

export const MANDER_PARTS = [
	"body",
	"leftEye",
	"rightEye",
	"leftFeature",
	"rightFeature",
	"forelimb",
	"thought",
	"streaks",
	"sparkle"
] as const;
export type ManderPart = (typeof MANDER_PARTS)[number];

export type TransitionKey = `${ManderState}->${ManderState}`;

export type EaseName = "linear" | "easeOut" | "easeInOut" | "easeOutBack";

export type PoseKeyframe = {
	at: number;
	pose: ManderPose;
	ease?: EaseName;
};

export type TransitionRecipe = {
	durationMs: number;
	loop: boolean;
	kind: "directed" | "loop";
	keyframes: PoseKeyframe[];
};

export type MotionClock = {
	now: () => number;
	requestFrame: (callback: FrameRequestCallback) => number;
	cancelFrame: (handle: number) => void;
};

export type MotionSnapshot = {
	pose: ManderPose;
	state: ManderState;
	motion: ResolvedManderMotion;
	recipeKey: TransitionKey;
	elapsedMs: number;
	running: boolean;
	frameCount: number;
};
