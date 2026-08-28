import { useEffect, useRef, useState } from "react";
import { ManderMotionEngine } from "./Mander.motion";
import { restPose } from "./Mander.poses";
import type { ManderMotion, ManderPose, ManderState, ResolvedManderMotion } from "./Mander.types";

function prefersReducedMotion(): boolean {
	return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function resolveManderMotion(motion: ManderMotion, reducedPreference: boolean): ResolvedManderMotion {
	if (motion === "auto") return reducedPreference ? "reduced" : "full";
	return motion;
}

export function useManderMotion(state: ManderState, motion: ManderMotion = "auto"): {
	pose: ManderPose;
	resolvedMotion: ResolvedManderMotion;
	running: boolean;
} {
	const [reducedPreference, setReducedPreference] = useState(prefersReducedMotion);
	const resolvedMotion = resolveManderMotion(motion, reducedPreference);
	const [pose, setPose] = useState(() => restPose(state));
	const [running, setRunning] = useState(false);
	const engineRef = useRef<ManderMotionEngine | null>(null);
	const stateRef = useRef(state);

	useEffect(() => {
		const media = window.matchMedia("(prefers-reduced-motion: reduce)");
		const sync = () => setReducedPreference(media.matches);
		sync();
		media.addEventListener("change", sync);
		return () => media.removeEventListener("change", sync);
	}, []);

	useEffect(() => {
		const engine = new ManderMotionEngine({
			state: stateRef.current,
			motion: resolvedMotion,
			onFrame: (snapshot) => {
				setPose(snapshot.pose);
				setRunning(snapshot.running);
			}
		});
		engineRef.current = engine;
		engine.start();
		return () => {
			engine.stop();
			engineRef.current = null;
		};
	}, [resolvedMotion]);

	useEffect(() => {
		stateRef.current = state;
		engineRef.current?.setState(state);
	}, [state]);

	return { pose, resolvedMotion, running };
}
