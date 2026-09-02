import { useLayoutEffect, useRef, useState } from "react";

type Props = {
	thumbSelector: string;
	bodySelector: string;
};

const half = (value: number) => Math.round(value * 2) / 2;

/**
 * One SVG path owns the complete pane silhouette: selected-tab thumb,
 * concave shoulder(s), body, and rounded outer corners. Keeping the stroke in
 * one path prevents the doubled seams produced by overlapping CSS borders.
 */
export function MittenFrame({ thumbSelector, bodySelector }: Props) {
	const svgRef = useRef<SVGSVGElement>(null);
	const [geometry, setGeometry] = useState({ width: 1, height: 1, path: "" });

	useLayoutEffect(() => {
		const svg = svgRef.current;
		const host = svg?.parentElement;
		if (!svg || !host) return;

		const measure = () => {
			const thumb = host.querySelector<HTMLElement>(thumbSelector);
			const body = host.querySelector<HTMLElement>(bodySelector);
			if (!thumb || !body) return;
			const hostRect = host.getBoundingClientRect();
			const thumbRect = thumb.getBoundingClientRect();
			const bodyRect = body.getBoundingClientRect();
			if (hostRect.width < 2 || hostRect.height < 2) return;

			const width = half(hostRect.width);
			const height = half(hostRect.height);
			const left = half(Math.max(0.5, bodyRect.left - hostRect.left + 0.5));
			const right = half(Math.min(width - 0.5, bodyRect.right - hostRect.left - 0.5));
			const baseline = half(Math.max(0.5, bodyRect.top - hostRect.top + 0.5));
			const bottom = half(Math.min(height - 0.5, bodyRect.bottom - hostRect.top - 0.5));
			const start = half(Math.max(left, thumbRect.left - hostRect.left + 0.5));
			const end = half(Math.min(right, thumbRect.right - hostRect.left - 0.5));
			const top = half(Math.max(0.5, thumbRect.top - hostRect.top + 0.5));
			const radius = Math.min(14, Math.max(6, (right - left) / 5), Math.max(6, (bottom - baseline) / 5));
			const first = start <= left + 1.5;
			const leftJoin = first ? 0 : Math.min(14, Math.max(6, (start - left) / 2));
			const rightJoin = Math.min(14, Math.max(6, (right - end) / 2));
			const lk = leftJoin * 0.5522848;
			const rk = rightJoin * 0.5522848;
			const commands: string[] = [];

			if (first) {
				commands.push(`M ${left} ${baseline}`, `V ${top + radius}`);
			} else {
				commands.push(
					`M ${left + radius} ${baseline}`,
					`H ${start - leftJoin}`,
					`C ${start - leftJoin + lk} ${baseline} ${start} ${baseline - leftJoin + lk} ${start} ${baseline - leftJoin}`,
					`V ${top + radius}`
				);
			}

			commands.push(
				`Q ${start} ${top} ${start + radius} ${top}`,
				`H ${Math.max(start + radius, end - radius)}`,
				`Q ${end} ${top} ${end} ${top + radius}`,
				`V ${baseline - rightJoin}`,
				`C ${end} ${baseline - rightJoin + rk} ${end + rightJoin - rk} ${baseline} ${end + rightJoin} ${baseline}`,
				`H ${right - radius}`,
				`Q ${right} ${baseline} ${right} ${baseline + radius}`,
				`V ${bottom - radius}`,
				`Q ${right} ${bottom} ${right - radius} ${bottom}`,
				`H ${left + radius}`,
				`Q ${left} ${bottom} ${left} ${bottom - radius}`,
				`V ${baseline + radius}`
			);

			if (first) commands.push(`V ${baseline}`, "Z");
			else commands.push(`Q ${left} ${baseline} ${left + radius} ${baseline}`, "Z");

			const path = commands.join(" ");
			setGeometry((current) => current.width === width && current.height === height && current.path === path
				? current
				: { width, height, path });
		};

		const resize = new ResizeObserver(measure);
		resize.observe(host);
		const mutation = new MutationObserver(measure);
		mutation.observe(host, { attributes: true, subtree: true, attributeFilter: ["class", "aria-selected"] });
		measure();
		return () => {
			resize.disconnect();
			mutation.disconnect();
		};
	}, [bodySelector, thumbSelector]);

	return <svg
		ref={svgRef}
		className="mitten-frame-svg"
		viewBox={`0 0 ${geometry.width} ${geometry.height}`}
		preserveAspectRatio="none"
		aria-hidden="true"
		focusable="false"
	>
		<path d={geometry.path} vectorEffect="non-scaling-stroke" />
	</svg>;
}
