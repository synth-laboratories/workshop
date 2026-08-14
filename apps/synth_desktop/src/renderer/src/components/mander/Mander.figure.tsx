import {
	BODY_ANCHOR,
	BODY_PIXELS,
	FORELIMB_ANCHOR,
	FORELIMB_PIXELS,
	LEFT_EYE_ANCHOR,
	LEFT_FEATURE_ANCHOR,
	LEFT_GILL_PIXELS,
	MANDER_VIEWBOX,
	RIGHT_EYE_ANCHOR,
	RIGHT_FEATURE_ANCHOR,
	RIGHT_GILL_PIXELS,
	SPARKLE_ANCHOR,
	SPARKLE_PIXELS,
	STREAK_ANCHOR,
	STREAK_PIXELS,
	THOUGHT_ANCHOR,
	THOUGHT_PIXELS,
	partTransform,
	type PixelRun
} from "./Mander.geometry";
import type { ManderPose } from "./Mander.types";

const TONE_CLASS = {
	fill: "mander-fill",
	shade: "mander-fill-shade",
	mouth: "mander-mouth"
} as const;

function Pixels({ runs }: { runs: PixelRun[] }) {
	return (
		<>
			{runs.map((run) => (
				<rect
					key={`${run.tone ?? "fill"}-${run.x}-${run.y}-${run.w}-${run.h ?? 1}`}
					className={TONE_CLASS[run.tone ?? "fill"]}
					x={run.x}
					y={run.y}
					width={run.w}
					height={run.h ?? 1}
				/>
			))}
		</>
	);
}

export function ManderFigure({ pose }: { pose: ManderPose }) {
	return (
		<>
			<rect className="mander-shadow" x="12" y="46" width="36" height="1" />
			<g
				data-mander-part="body"
				aria-hidden="true"
				transform={partTransform(BODY_ANCHOR.x, BODY_ANCHOR.y, pose.body)}
				opacity={pose.body.opacity}
			>
				<g
					data-mander-part="streaks"
					transform={partTransform(STREAK_ANCHOR.x, STREAK_ANCHOR.y, pose.streaks)}
					opacity={pose.streaks.opacity}
				>
					<Pixels runs={STREAK_PIXELS} />
				</g>
				<g
					data-mander-part="right-feature"
					transform={partTransform(RIGHT_FEATURE_ANCHOR.x, RIGHT_FEATURE_ANCHOR.y, pose.rightFeature)}
					opacity={pose.rightFeature.opacity}
				>
					<Pixels runs={RIGHT_GILL_PIXELS} />
				</g>
				<Pixels runs={BODY_PIXELS} />
				<g
					data-mander-part="left-feature"
					transform={partTransform(LEFT_FEATURE_ANCHOR.x, LEFT_FEATURE_ANCHOR.y, pose.leftFeature)}
					opacity={pose.leftFeature.opacity}
				>
					<Pixels runs={LEFT_GILL_PIXELS} />
				</g>
				<g
					data-mander-part="forelimb"
					transform={partTransform(FORELIMB_ANCHOR.x, FORELIMB_ANCHOR.y, pose.forelimb)}
					opacity={pose.forelimb.opacity}
				>
					<Pixels runs={FORELIMB_PIXELS} />
				</g>
				<g
					data-mander-part="left-eye"
					transform={partTransform(LEFT_EYE_ANCHOR.x, LEFT_EYE_ANCHOR.y, pose.leftEye)}
					opacity={pose.leftEye.opacity}
				>
					<rect className="mander-eye" x={-1} y={-1} width={2} height={2} />
				</g>
				<g
					data-mander-part="right-eye"
					transform={partTransform(RIGHT_EYE_ANCHOR.x, RIGHT_EYE_ANCHOR.y, pose.rightEye)}
					opacity={pose.rightEye.opacity}
				>
					<rect className="mander-eye" x={-1} y={-1} width={2} height={2} />
				</g>
				<g
					data-mander-part="thought"
					transform={partTransform(THOUGHT_ANCHOR.x, THOUGHT_ANCHOR.y, pose.thought)}
					opacity={pose.thought.opacity}
				>
					<Pixels runs={THOUGHT_PIXELS} />
				</g>
				<g
					data-mander-part="sparkle"
					transform={partTransform(SPARKLE_ANCHOR.x, SPARKLE_ANCHOR.y, pose.sparkle)}
					opacity={pose.sparkle.opacity}
				>
					<Pixels runs={SPARKLE_PIXELS} />
				</g>
			</g>
		</>
	);
}

export { MANDER_VIEWBOX };
