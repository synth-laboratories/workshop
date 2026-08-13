import type { KeyboardEvent, PointerEvent } from "react";

type Props = {
	value: number;
	onChange: (value: number) => void;
	minPrimary?: number;
	minSecondary?: number;
	ariaLabel?: string;
	direction?: "output" | "sidebar" | "primary";
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
		if (direction === "primary") {
			const parent = target.parentElement;
			if (!parent) return;
			const bounds = parent.getBoundingClientRect();
			const maximum = Math.max(minPrimary, bounds.width - minSecondary);
			onChange(Math.round(Math.min(maximum, Math.max(minPrimary, clientX - bounds.left))));
			return;
		}
		if (direction === "sidebar") {
			const appRow = target.parentElement?.parentElement;
			if (!appRow) return;
			const bounds = appRow.getBoundingClientRect();
			const maximum = Math.max(minSecondary, bounds.width - minPrimary);
			onChange(Math.round(Math.min(maximum, Math.max(minSecondary, clientX - bounds.left))));
			return;
		}
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

	const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
		if (event.currentTarget.hasPointerCapture(event.pointerId)) {
			event.currentTarget.releasePointerCapture(event.pointerId);
		}
	};

	const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const delta = event.shiftKey ? 64 : 24;
		const signed = direction === "sidebar" || direction === "primary"
			? (event.key === "ArrowRight" ? delta : -delta)
			: (event.key === "ArrowLeft" ? delta : -delta);
		onChange(Math.max(direction === "primary" ? minPrimary : minSecondary, value + signed));
	};

	return <div
		className={`pane-resize-handle${direction === "sidebar" ? " sidebar-resize-handle" : ""}${direction === "primary" ? " primary-resize-handle" : ""}`}
		role="separator"
		aria-label={ariaLabel}
		aria-orientation="vertical"
		aria-valuemin={minSecondary}
		aria-valuenow={value}
		tabIndex={0}
		data-testid={direction === "sidebar" ? "sidebar-resize-handle" : direction === "primary" ? "visuals-resize-handle" : "pane-resize-handle"}
		onPointerDown={onPointerDown}
		onPointerMove={onPointerMove}
		onPointerUp={onPointerUp}
		onPointerCancel={onPointerUp}
		onKeyDown={onKeyDown}
	/>;
}
