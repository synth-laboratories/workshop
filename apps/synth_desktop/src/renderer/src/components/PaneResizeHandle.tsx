import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

type Props = {
	value: number;
	onChange: (value: number) => void;
	minPrimary?: number;
	minSecondary?: number;
	ariaLabel?: string;
	direction?: "output" | "sidebar" | "primary";
	resetValue?: number;
};

export function PaneResizeHandle({
	value,
	onChange,
	minPrimary = 360,
	minSecondary = 340,
	ariaLabel = "Resize container inspector",
	direction = "output",
	resetValue
}: Props) {
	const handleRef = useRef<HTMLDivElement>(null);
	const activePointer = useRef<number | null>(null);
	const [maximum, setMaximum] = useState(value);
	const resolvedResetValue = resetValue ?? (direction === "sidebar" ? 260 : direction === "primary" ? 560 : 420);

	const measureMaximum = useCallback((target: HTMLElement) => {
		if (direction === "sidebar") {
			const appRow = target.parentElement?.parentElement;
			return appRow ? Math.max(minSecondary, appRow.getBoundingClientRect().width - minPrimary) : minSecondary;
		}
		const parent = target.parentElement;
		const minimum = direction === "primary" ? minPrimary : minSecondary;
		return parent ? Math.max(minimum, parent.getBoundingClientRect().width - (direction === "primary" ? minSecondary : minPrimary)) : minimum;
	}, [direction, minPrimary, minSecondary]);

	const resize = useCallback((clientX: number, target: HTMLElement) => {
		if (direction === "primary") {
			const parent = target.parentElement;
			if (!parent) return;
			const bounds = parent.getBoundingClientRect();
			onChange(Math.round(Math.min(measureMaximum(target), Math.max(minPrimary, clientX - bounds.left))));
			return;
		}
		if (direction === "sidebar") {
			const appRow = target.parentElement?.parentElement;
			if (!appRow) return;
			const bounds = appRow.getBoundingClientRect();
			onChange(Math.round(Math.min(measureMaximum(target), Math.max(minSecondary, clientX - bounds.left))));
			return;
		}
		const parent = target.parentElement;
		if (!parent) return;
		const bounds = parent.getBoundingClientRect();
		onChange(Math.round(Math.min(measureMaximum(target), Math.max(minSecondary, bounds.right - clientX))));
	}, [direction, measureMaximum, minPrimary, minSecondary, onChange]);

	const release = useCallback(() => {
		const target = handleRef.current;
		const pointerId = activePointer.current;
		activePointer.current = null;
		if (target && pointerId !== null && target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
	}, []);

	useEffect(() => {
		const target = handleRef.current;
		if (!target) return;
		const measure = () => setMaximum(Math.round(measureMaximum(target)));
		measure();
		const observer = new ResizeObserver(measure);
		const observed = direction === "sidebar" ? target.parentElement?.parentElement : target.parentElement;
		if (observed) observer.observe(observed);
		window.addEventListener("blur", release);
		return () => { observer.disconnect(); window.removeEventListener("blur", release); release(); };
	}, [direction, measureMaximum, release]);

	const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
		event.preventDefault();
		activePointer.current = event.pointerId;
		event.currentTarget.setPointerCapture(event.pointerId);
		resize(event.clientX, event.currentTarget);
	};

	const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
		if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
		resize(event.clientX, event.currentTarget);
	};

	const onPointerUp = () => {
		release();
	};

	const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		const delta = event.shiftKey ? 64 : 24;
		const signed = direction === "sidebar" || direction === "primary"
			? (event.key === "ArrowRight" ? delta : -delta)
			: (event.key === "ArrowLeft" ? delta : -delta);
		const minimum = direction === "primary" ? minPrimary : minSecondary;
		onChange(Math.round(Math.min(maximum, Math.max(minimum, value + signed))));
	};

	return <div
		ref={handleRef}
		className={`pane-resize-handle${direction === "sidebar" ? " sidebar-resize-handle" : ""}${direction === "primary" ? " primary-resize-handle" : ""}`}
		role="separator"
		aria-label={ariaLabel}
		aria-orientation="vertical"
		aria-valuemin={direction === "primary" ? minPrimary : minSecondary}
		aria-valuemax={maximum}
		aria-valuenow={value}
		tabIndex={0}
		data-testid={direction === "sidebar" ? "sidebar-resize-handle" : direction === "primary" ? "visuals-resize-handle" : "pane-resize-handle"}
		onPointerDown={onPointerDown}
		onPointerMove={onPointerMove}
		onPointerUp={onPointerUp}
		onPointerCancel={onPointerUp}
		onLostPointerCapture={() => { activePointer.current = null; }}
		onKeyDown={onKeyDown}
		onDoubleClick={() => onChange(Math.min(maximum, Math.max(direction === "primary" ? minPrimary : minSecondary, resolvedResetValue)))}
	/>;
}
