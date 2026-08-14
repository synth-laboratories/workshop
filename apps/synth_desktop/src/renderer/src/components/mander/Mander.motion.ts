import { clonePose, lerpPose, restPose } from "./Mander.poses";
import { recipeFor, transitionKey } from "./Mander.transitions";
import type {
	EaseName,
	ManderPose,
	ManderState,
	MotionClock,
	MotionSnapshot,
	PoseKeyframe,
	ResolvedManderMotion,
	TransitionKey,
	TransitionRecipe
} from "./Mander.types";

const defaultClock: MotionClock = {
	now: () => (typeof performance === "undefined" ? Date.now() : performance.now()),
	requestFrame: (callback) => requestAnimationFrame(callback),
	cancelFrame: (handle) => cancelAnimationFrame(handle)
};

function clamp(value: number, min: number, max: number): number {
	return Math.min(max, Math.max(min, value));
}

export function ease(name: EaseName, t: number): number {
	const x = clamp(t, 0, 1);
	if (name === "linear") return x;
	if (name === "easeInOut") return x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2;
	if (name === "easeOutBack") {
		const c1 = 1.2504;
		const c3 = c1 + 1;
		return 1 + c3 * Math.pow(x - 1, 3) + c1 * Math.pow(x - 1, 2);
	}
	return 1 - Math.pow(1 - x, 3);
}

export function sampleKeyframes(from: ManderPose, frames: PoseKeyframe[], t: number): ManderPose {
	const points: PoseKeyframe[] = [{ at: 0, pose: from, ease: "linear" }];
	for (const frame of frames) {
		if (frame.at > 0) points.push(frame);
	}
	const last = points[points.length - 1];
	if (!last || last.at < 1) {
		const terminal = frames[frames.length - 1]?.pose ?? from;
		points.push({ at: 1, pose: terminal, ease: last?.ease ?? "easeOut" });
	}
	const progress = clamp(t, 0, 1);
	let index = 0;
	while (index < points.length - 2 && points[index + 1]!.at <= progress) index += 1;
	const a = points[index]!;
	const b = points[index + 1]!;
	const span = Math.max(b.at - a.at, 1e-6);
	const local = ease(b.ease ?? "easeOut", (progress - a.at) / span);
	return lerpPose(a.pose, b.pose, local);
}

export function sampleLoop(frames: PoseKeyframe[], t: number): ManderPose {
	const start = frames[0]?.pose;
	if (!start) throw new Error("Loop recipe is missing keyframes");
	return sampleKeyframes(start, frames, t);
}

export type ManderMotionOptions = {
	state: ManderState;
	motion: ResolvedManderMotion;
	clock?: MotionClock;
	onFrame?: (snapshot: MotionSnapshot) => void;
};

export class ManderMotionEngine {
	pose: ManderPose;
	state: ManderState;
	motion: ResolvedManderMotion;
	recipeKey: TransitionKey;
	elapsedMs = 0;
	frameCount = 0;
	running = false;
	private from: ManderPose;
	private recipe: TransitionRecipe;
	private clock: MotionClock;
	private handle: number | null = null;
	private lastNow: number | null = null;
	private inFrame = false;
	private onFrame?: (snapshot: MotionSnapshot) => void;

	constructor(options: ManderMotionOptions) {
		this.state = options.state;
		this.motion = options.motion;
		this.clock = options.clock ?? defaultClock;
		this.onFrame = options.onFrame;
		this.pose = restPose(options.state);
		this.from = clonePose(this.pose);
		this.recipeKey = transitionKey(options.state, options.state);
		this.recipe = recipeFor(this.recipeKey, options.motion === "reduced");
	}

	snapshot(): MotionSnapshot {
		return {
			pose: clonePose(this.pose),
			state: this.state,
			motion: this.motion,
			recipeKey: this.recipeKey,
			elapsedMs: this.elapsedMs,
			running: this.running,
			frameCount: this.frameCount
		};
	}

	start(): void {
		if (this.motion === "still") {
			this.pose = restPose(this.state);
			this.running = false;
			this.emit();
			return;
		}
		this.running = true;
		if (this.handle != null || this.inFrame) {
			this.emit();
			return;
		}
		this.lastNow = this.clock.now();
		this.handle = this.clock.requestFrame(this.loop);
		this.emit();
	}

	stop(): void {
		if (this.handle != null) {
			this.clock.cancelFrame(this.handle);
			this.handle = null;
		}
		this.running = false;
		this.lastNow = null;
	}

	setState(next: ManderState): void {
		const previous = this.state;
		this.state = next;
		if (this.motion === "still") {
			this.pose = restPose(next);
			this.from = clonePose(this.pose);
			this.recipeKey = transitionKey(next, next);
			this.recipe = recipeFor(this.recipeKey, false);
			this.elapsedMs = 0;
			this.stop();
			this.emit();
			return;
		}
		this.begin(transitionKey(previous, next));
	}

	setMotion(next: ResolvedManderMotion): void {
		if (next === this.motion) return;
		this.motion = next;
		if (next === "still") {
			this.pose = restPose(this.state);
			this.from = clonePose(this.pose);
			this.elapsedMs = 0;
			this.stop();
			this.emit();
			return;
		}
		this.begin(this.recipeKey);
	}

	tick(now: number): void {
		const last = this.lastNow ?? now;
		const dt = Math.min(64, Math.max(0, now - last));
		this.lastNow = now;
		this.elapsedMs += dt;
		this.frameCount += 1;
		this.pose = this.sample(this.elapsedMs, dt);
		if (!this.recipe.loop && this.elapsedMs >= this.recipe.durationMs) {
			this.pose = this.sample(this.recipe.durationMs, dt);
			if (this.motion === "full") {
				this.begin(transitionKey(this.state, this.state));
			} else {
				this.stop();
				this.emit();
				return;
			}
		}
		this.emit();
	}

	private begin(key: TransitionKey): void {
		this.recipeKey = key;
		this.recipe = recipeFor(key, this.motion === "reduced");
		this.from = clonePose(this.pose);
		this.elapsedMs = 0;
		this.start();
	}

	private sample(elapsedMs: number, dt: number): ManderPose {
		const duration = Math.max(this.recipe.durationMs, 1);
		if (this.recipe.kind === "loop") {
			const desired = sampleLoop(this.recipe.keyframes, (elapsedMs % duration) / duration);
			const follow = 1 - Math.exp(-dt / 26);
			return lerpPose(this.pose, desired, follow);
		}
		return sampleKeyframes(this.from, this.recipe.keyframes, elapsedMs / duration);
	}

	private loop = (now: number): void => {
		this.handle = null;
		this.inFrame = true;
		this.tick(now);
		this.inFrame = false;
		if (!this.running) return;
		if (this.handle == null) this.handle = this.clock.requestFrame(this.loop);
	};

	private emit(): void {
		this.onFrame?.(this.snapshot());
	}
}

export function createManderMotion(options: ManderMotionOptions): ManderMotionEngine {
	return new ManderMotionEngine(options);
}
