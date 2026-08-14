import { ManderFigure, MANDER_VIEWBOX } from "./Mander.figure";
import { useManderMotion } from "./useManderMotion";
import type { ManderProps } from "./Mander.types";

export function Mander({
	state,
	size = 64,
	motion = "auto",
	className,
	label
}: ManderProps) {
	const { pose, resolvedMotion, running } = useManderMotion(state, motion);
	const decorative = !label;

	return (
		<svg
			className={className ? `mander ${className}` : "mander"}
			width={size}
			height={size}
			viewBox={MANDER_VIEWBOX}
			preserveAspectRatio="xMidYMid meet"
			shapeRendering="crispEdges"
			role={decorative ? "presentation" : "img"}
			aria-hidden={decorative || undefined}
			aria-label={label}
			focusable="false"
			data-testid="mander"
			data-mander-state={state}
			data-mander-motion={resolvedMotion}
			data-mander-running={running ? "true" : "false"}
		>
			{label ? <title>{label}</title> : null}
			<ManderFigure pose={pose} />
		</svg>
	);
}
