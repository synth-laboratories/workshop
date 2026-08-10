import type { KeyboardEvent, PointerEvent } from "react";

type Props = {
	value: number;
	onChange: (value: number) => void;
	minPrimary?: number;
	minSecondary?: number;
	ariaLabel?: string;
	direction?: "output" | "sidebar";
};

export function PaneResizeHandle({
	value,
	onChange,
	minPrimary = 360,
	minSecondary = 340,
	ariaLabel = "Resize container inspector",
	direction = "output"
}: Props) {
	const resize = (clientX: number, target: HTMLElement) => {
		const parent = target.parentElement;
		if (!parent) return;
		const bounds = parent.getBoundingClientRect();
		if (direction === "sidebar") {
			const maximum = Math.max(minSecondary, bounds.width - minPrimary);
			onChange(Math.round(Math.min(maximum, Math.max(minSecondary, clientX - bounds.left))));
			return;
		}
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
		const signed = direction === "sidebar"
			? (event.key === "ArrowRight" ? delta : -delta)
			: (event.key === "ArrowLeft" ? delta : -delta);
		onChange(Math.max(minSecondary, value + signed));
	};

	return <div
		className={`pane-resize-handle${direction === "sidebar" ? " sidebar-resize-handle" : ""}`}
		role="separator"
		aria-label={ariaLabel}
		aria-orientation="vertical"
		aria-valuemin={minSecondary}
		aria-valuenow={value}
		tabIndex={0}
		data-testid={direction === "sidebar" ? "sidebar-resize-handle" : "pane-resize-handle"}
		onPointerDown={onPointerDown}
		onPointerMove={onPointerMove}
		onKeyDown={onKeyDown}
	/>;
}
