import type { PartPose } from "./Mander.types";

/** 64×64 texel tracing of larval-mander idle_poster.png. One unit = one pixel. */
export const MANDER_VIEWBOX = "0 0 64 64";
export const BODY_ANCHOR = { x: 32, y: 32 };
export const LEFT_EYE_ANCHOR = { x: -15, y: -2 };
export const RIGHT_EYE_ANCHOR = { x: -11, y: -2 };
export const LEFT_FEATURE_ANCHOR = { x: -16, y: -9 };
export const RIGHT_FEATURE_ANCHOR = { x: -6, y: -8 };
export const FORELIMB_ANCHOR = { x: -16, y: 8 };
export const THOUGHT_ANCHOR = { x: 8, y: -24 };
export const STREAK_ANCHOR = { x: 14, y: -2 };
export const SPARKLE_ANCHOR = { x: 0, y: -20 };

export type PixelRun = {
	x: number;
	y: number;
	w: number;
	h?: number;
	tone?: "fill" | "shade" | "mouth";
};

export function partTransform(anchorX: number, anchorY: number, pose: PartPose): string {
	const x = Math.round(anchorX + pose.x);
	const y = Math.round(anchorY + pose.y);
	return `translate(${x} ${y}) rotate(${pose.rotation}) scale(${pose.scaleX} ${pose.scaleY})`;
}

export const LEFT_GILL_PIXELS: PixelRun[] = [
	{ x: -2, y: -7, w: 2 },
	{ x: -3, y: -6, w: 3 },
	{ x: -3, y: -5, w: 3 },
	{ x: -4, y: -5, w: 1, tone: "shade" },
	{ x: -6, y: -4, w: 1 },
	{ x: -4, y: -4, w: 4 },
	{ x: -6, y: -3, w: 1 },
	{ x: -4, y: -3, w: 4 },
	{ x: -5, y: -3, w: 1, tone: "shade" },
	{ x: -6, y: -2, w: 4 },
	{ x: -1, y: -2, w: 1 },
	{ x: -6, y: -1, w: 3 },
	{ x: -1, y: -1, w: 1 },
	{ x: -3, y: -1, w: 2, tone: "shade" },
	{ x: -6, y: 0, w: 11 },
	{ x: -4, y: 1, w: 8 }
];

export const RIGHT_GILL_PIXELS: PixelRun[] = [
	{ x: 1, y: -6, w: 1 },
	{ x: -2, y: -5, w: 1 },
	{ x: 1, y: -5, w: 1 },
	{ x: 3, y: -5, w: 3 },
	{ x: -2, y: -4, w: 1 },
	{ x: 0, y: -4, w: 6 },
	{ x: -1, y: -4, w: 1, tone: "shade" },
	{ x: -3, y: -3, w: 1 },
	{ x: -1, y: -3, w: 6 },
	{ x: -2, y: -3, w: 1, tone: "shade" },
	{ x: -4, y: -2, w: 1 },
	{ x: -2, y: -2, w: 7 },
	{ x: -3, y: -2, w: 1, tone: "shade" },
	{ x: -4, y: -1, w: 3 },
	{ x: 2, y: -1, w: 2 },
	{ x: -1, y: -1, w: 2, tone: "shade" },
	{ x: -5, y: 0, w: 5 },
	{ x: 1, y: 0, w: 1 },
	{ x: 3, y: 0, w: 2 },
	{ x: 6, y: 0, w: 2 },
	{ x: 0, y: 0, w: 1, tone: "shade" },
	{ x: -4, y: 1, w: 8 }
];

export const BODY_PIXELS: PixelRun[] = [
	{ x: 16, y: -9, w: 3 },
	{ x: -24, y: -8, w: 13 },
	{ x: 16, y: -8, w: 3 },
	{ x: -25, y: -7, w: 22 },
	{ x: -2, y: -7, w: 4 },
	{ x: 17, y: -7, w: 1 },
	{ x: -3, y: -7, w: 1, tone: "shade" },
	{ x: -26, y: -6, w: 26 },
	{ x: 0, y: -6, w: 1, tone: "shade" },
	{ x: -26, y: -5, w: 26 },
	{ x: 2, y: -5, w: 5 },
	{ x: 21, y: -5, w: 6 },
	{ x: 0, y: -5, w: 2, tone: "shade" },
	{ x: -26, y: -4, w: 24 },
	{ x: -1, y: -4, w: 11 },
	{ x: 19, y: -4, w: 4 },
	{ x: 25, y: -4, w: 2 },
	{ x: -2, y: -4, w: 1, tone: "shade" },
	{ x: 23, y: -4, w: 2, tone: "shade" },
	{ x: -26, y: -3, w: 10 },
	{ x: -13, y: -3, w: 7 },
	{ x: -2, y: -3, w: 14 },
	{ x: 17, y: -3, w: 5 },
	{ x: 23, y: -3, w: 1 },
	{ x: 25, y: -3, w: 1 },
	{ x: -6, y: -3, w: 1, tone: "shade" },
	{ x: -3, y: -3, w: 1, tone: "shade" },
	{ x: 22, y: -3, w: 1, tone: "shade" },
	{ x: 24, y: -3, w: 1, tone: "shade" },
	{ x: -26, y: -2, w: 10 },
	{ x: -13, y: -2, w: 7 },
	{ x: -4, y: -2, w: 2 },
	{ x: 0, y: -2, w: 14 },
	{ x: 15, y: -2, w: 3 },
	{ x: 19, y: -2, w: 2 },
	{ x: 22, y: -2, w: 1 },
	{ x: 24, y: -2, w: 1 },
	{ x: -6, y: -2, w: 2, tone: "shade" },
	{ x: -2, y: -2, w: 2, tone: "shade" },
	{ x: 18, y: -2, w: 1, tone: "shade" },
	{ x: 21, y: -2, w: 1, tone: "shade" },
	{ x: 23, y: -2, w: 1, tone: "shade" },
	{ x: -26, y: -1, w: 1 },
	{ x: -24, y: -1, w: 3 },
	{ x: -20, y: -1, w: 4 },
	{ x: -13, y: -1, w: 10 },
	{ x: -2, y: -1, w: 17 },
	{ x: 18, y: -1, w: 1 },
	{ x: 20, y: -1, w: 3 },
	{ x: 24, y: -1, w: 1 },
	{ x: -21, y: -1, w: 1, tone: "shade" },
	{ x: -3, y: -1, w: 1, tone: "shade" },
	{ x: 15, y: -1, w: 3, tone: "shade" },
	{ x: 19, y: -1, w: 1, tone: "shade" },
	{ x: 23, y: -1, w: 1, tone: "shade" },
	{ x: -26, y: 0, w: 24 },
	{ x: 0, y: 0, w: 22 },
	{ x: 23, y: 0, w: 2 },
	{ x: -2, y: 0, w: 2, tone: "shade" },
	{ x: 22, y: 0, w: 1, tone: "shade" },
	{ x: -26, y: 1, w: 19 },
	{ x: -5, y: 1, w: 26 },
	{ x: 23, y: 1, w: 1 },
	{ x: -7, y: 1, w: 2, tone: "shade" },
	{ x: 21, y: 1, w: 2, tone: "shade" },
	{ x: -26, y: 2, w: 20 },
	{ x: -5, y: 2, w: 2 },
	{ x: -2, y: 2, w: 22 },
	{ x: 21, y: 2, w: 3 },
	{ x: -6, y: 2, w: 1, tone: "shade" },
	{ x: -3, y: 2, w: 1, tone: "shade" },
	{ x: 20, y: 2, w: 1, tone: "shade" },
	{ x: -25, y: 3, w: 44 },
	{ x: 20, y: 3, w: 1 },
	{ x: 22, y: 3, w: 1 },
	{ x: 19, y: 3, w: 1, tone: "shade" },
	{ x: 21, y: 3, w: 1, tone: "shade" },
	{ x: -23, y: 4, w: 40 },
	{ x: 19, y: 4, w: 1 },
	{ x: 21, y: 4, w: 2 },
	{ x: 17, y: 4, w: 2, tone: "shade" },
	{ x: 20, y: 4, w: 1, tone: "shade" },
	{ x: -13, y: 5, w: 28 },
	{ x: 17, y: 5, w: 5 },
	{ x: -15, y: 5, w: 2, tone: "shade" },
	{ x: 15, y: 5, w: 2, tone: "shade" },
	{ x: -15, y: 6, w: 17 },
	{ x: 3, y: 6, w: 4 },
	{ x: 8, y: 6, w: 12 },
	{ x: 2, y: 6, w: 1, tone: "shade" },
	{ x: 7, y: 6, w: 1, tone: "shade" },
	{ x: -13, y: 7, w: 15 },
	{ x: 3, y: 7, w: 7 },
	{ x: 12, y: 7, w: 6 },
	{ x: -16, y: 7, w: 2, tone: "shade" },
	{ x: 2, y: 7, w: 1, tone: "shade" },
	{ x: -11, y: 8, w: 4 },
	{ x: -5, y: 8, w: 1 },
	{ x: -1, y: 8, w: 4 },
	{ x: 7, y: 8, w: 4 },
	{ x: -4, y: 8, w: 3, tone: "shade" },
	{ x: -12, y: 9, w: 5 },
	{ x: -1, y: 9, w: 4 },
	{ x: 7, y: 9, w: 4 },
	{ x: -13, y: 10, w: 5 },
	{ x: -2, y: 10, w: 4 },
	{ x: 6, y: 10, w: 4 },
	{ x: -13, y: 11, w: 4 },
	{ x: -2, y: 11, w: 3 },
	{ x: 7, y: 11, w: 3 },
	{ x: 6, y: 11, w: 1, tone: "shade" },
	{ x: -12, y: 12, w: 3 },
	{ x: -22, y: 1, w: 4, tone: "mouth" }
];

/** Front-left stub. Idle hangs as a leg; thinking translates it to the chin. */
export const FORELIMB_PIXELS: PixelRun[] = [
	{ x: -1, y: 0, w: 4, tone: "shade" },
	{ x: 0, y: 0, w: 2 },
	{ x: -2, y: 1, w: 4, tone: "shade" },
	{ x: -1, y: 1, w: 2 },
	{ x: -2, y: 2, w: 3, tone: "shade" },
	{ x: -1, y: 2, w: 2 }
];

/** Pixel thought bubble — trail of squares plus a hollow frame with three dots. */
export const THOUGHT_PIXELS: PixelRun[] = [
	{ x: -4, y: 10, w: 2, h: 2 },
	{ x: -2, y: 8, w: 2, h: 2, tone: "shade" },
	{ x: 1, y: 0, w: 10 },
	{ x: 0, y: 1, w: 1, h: 6 },
	{ x: 11, y: 1, w: 1, h: 6 },
	{ x: 1, y: 7, w: 10 },
	{ x: 3, y: 3, w: 1, h: 2 },
	{ x: 6, y: 3, w: 1, h: 2, tone: "shade" },
	{ x: 9, y: 3, w: 1, h: 2 }
];

/** Horizontal speed ticks behind the tail — working only. */
export const STREAK_PIXELS: PixelRun[] = [
	{ x: 0, y: -6, w: 6 },
	{ x: 3, y: -3, w: 9, tone: "shade" },
	{ x: 1, y: 0, w: 11 },
	{ x: 4, y: 3, w: 7 },
	{ x: 2, y: 6, w: 8, tone: "shade" },
	{ x: 5, y: 9, w: 5 }
];

function star(x: number, y: number, tone?: PixelRun["tone"]): PixelRun[] {
	const mark = tone ? { tone } : {};
	return [
		{ x, y, w: 1, ...mark },
		{ x: x - 1, y: y + 1, w: 3, ...mark },
		{ x, y: y + 2, w: 1, ...mark }
	];
}

/** Four-point stars around the figure — success only. */
export const SPARKLE_PIXELS: PixelRun[] = [
	...star(-18, 0),
	...star(14, 2, "shade"),
	...star(8, -6),
	...star(-10, 10, "shade"),
	{ x: 18, y: 12, w: 1 },
	{ x: -22, y: 6, w: 1 },
	{ x: 4, y: 14, w: 1, tone: "shade" }
];
