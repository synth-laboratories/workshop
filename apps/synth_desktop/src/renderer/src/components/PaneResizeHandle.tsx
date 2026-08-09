import type { KeyboardEvent, PointerEvent } from "react";

type Props = {
	value: number;
	onChange: (value: number) => void;
	minPrimary?: number;
	minSecondary?: number;
};

export function PaneResizeHandle({ value, onChange, minPrimary = 360, minSecondary = 340 }: Props) {
	const resize = (clientX: number, target: HTMLElement) => {
		const parent = target.parentElement;
		if (!parent) return;
		const bounds = parent.getBoundingClientRect();
		const maximum = Math.max(minSecondary, bounds.width - minPrimary);
		onChange(Math.round(Math.min(maximum, Math.max(minSecondary, bounds.right - clientX))));
	};

	const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
		event.preventDefault();
		event.currentTarget.setPointerCapture(event.pointerId);
		resize(event.clientX, event.currentTarget);
	};

	const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
		if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
		resize(event.clientX, event.currentTarget);
	};

	const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const delta = event.shiftKey ? 64 : 24;
		onChange(Math.max(minSecondary, value + (event.key === "ArrowLeft" ? delta : -delta)));
	};

	return <div
		className="pane-resize-handle"
		role="separator"
		aria-label="Resize container inspector"
		aria-orientation="vertical"
		aria-valuemin={minSecondary}
		aria-valuenow={value}
		tabIndex={0}
		onPointerDown={onPointerDown}
		onPointerMove={onPointerMove}
		onKeyDown={onKeyDown}
	/>;
}
