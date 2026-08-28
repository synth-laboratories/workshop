import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

export type PaneResizeDirection = "output" | "sidebar" | "primary";

type Props = {
	value: number;
	onChange: (value: number) => void;
	minPrimary?: number;
	minSecondary?: number;
	ariaLabel?: string;
	direction?: PaneResizeDirection;
	resetValue?: number;
};

/**
 * Keyboard spatial model (RP-CUA-012): Left shrinks the control's named
 * dimension for every splitter (visual pane, visuals list, sidebar). Right
 * grows it. Arrow keys move 40px; Shift+Arrow moves 64px. Home/End jump to
 * the advertised min/max.
 */
export const PANE_KEYBOARD_STEP_PX = 40;
export const PANE_KEYBOARD_SHIFT_STEP_PX = 64;

export function clampPaneWidth(value: number, min: number, max: number): number {
	return Math.round(Math.min(max, Math.max(min, value)));
}

/** CSS-pixel width after min/max. Prefer the layout box when it has been measured. */
export function realizedPaneWidth(requested: number, min: number, max: number, cssWidth?: number | null): number {
	if (typeof cssWidth === "number" && Number.isFinite(cssWidth)) {
		return clampPaneWidth(cssWidth, min, max);
	}
	return clampPaneWidth(requested, min, max);
}

/**
 * Left always decreases the named pane width. Directions do not invert the
 * keyboard axis — pointer geometry may still measure from the right edge.
 */
export function keyboardWidthDelta(key: string, shiftKey = false): number | null {
	const step = shiftKey ? PANE_KEYBOARD_SHIFT_STEP_PX : PANE_KEYBOARD_STEP_PX;
	if (key === "ArrowLeft") return -step;
	if (key === "ArrowRight") return step;
	return null;
}

export function applyKeyboardResize(options: {
	key: string;
	shiftKey?: boolean;
	value: number;
	min: number;
	max: number;
}): number | null {
	const { key, shiftKey = false, value, min, max } = options;
	if (key === "Home") return Math.round(min);
	if (key === "End") return Math.round(max);
	const delta = keyboardWidthDelta(key, shiftKey);
	if (delta == null) return null;
	return clampPaneWidth(value + delta, min, max);
}

/** The pane whose width this separator names and reports. */
export function namedPaneElement(handle: HTMLElement, direction: PaneResizeDirection): HTMLElement | null {
	if (direction === "sidebar") return handle.parentElement;
	if (direction === "primary") return handle.previousElementSibling instanceof HTMLElement ? handle.previousElementSibling : null;
	return handle.nextElementSibling instanceof HTMLElement ? handle.nextElementSibling : null;
}

export function paneKeyboardValueText(width: number): string {
	return `${width} pixels. Arrow keys move ${PANE_KEYBOARD_STEP_PX} pixels. Shift+Arrow moves ${PANE_KEYBOARD_SHIFT_STEP_PX} pixels. Home and End jump to the minimum and maximum.`;
}

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
	const settling = useRef(false);
	const settleFrame = useRef<number | null>(null);
	const valueRef = useRef(value);
	const onChangeRef = useRef(onChange);
	const [maximum, setMaximum] = useState(value);
	const [realized, setRealized] = useState<number | null>(null);
	const resolvedResetValue = resetValue ?? (direction === "sidebar" ? 260 : direction === "primary" ? 560 : 420);
	const minimum = direction === "primary" ? minPrimary : minSecondary;
	valueRef.current = value;
	onChangeRef.current = onChange;

	const measureMaximum = useCallback((target: HTMLElement) => {
		if (direction === "sidebar") {
			const appRow = target.parentElement?.parentElement;
			return appRow ? Math.max(minSecondary, appRow.getBoundingClientRect().width - minPrimary) : minSecondary;
		}
		const parent = target.parentElement;
		const floor = direction === "primary" ? minPrimary : minSecondary;
		return parent ? Math.max(floor, parent.getBoundingClientRect().width - (direction === "primary" ? minSecondary : minPrimary)) : floor;
	}, [direction, minPrimary, minSecondary]);

	const persistRealized = useCallback((target: HTMLElement) => {
		if (target.getClientRects().length === 0 || getComputedStyle(target).display === "none") return;
		const named = namedPaneElement(target, direction);
		if (!named) return;
		const cssWidth = Math.round(named.getBoundingClientRect().width);
		if (!Number.isFinite(cssWidth) || cssWidth < 1) return;
		setRealized(cssWidth);
		if (Math.abs(cssWidth - valueRef.current) >= 1) onChangeRef.current(cssWidth);
	}, [direction]);

	const cancelSettlement = useCallback(() => {
		if (settleFrame.current !== null) cancelAnimationFrame(settleFrame.current);
		settleFrame.current = null;
		settling.current = false;
	}, []);

	const settleAfterLayout = useCallback((target: HTMLElement) => {
		cancelSettlement();
		settling.current = true;
		// Pointer capture is released before React is guaranteed to have painted
		// the final drag value. Reconcile only after two layout frames so a stale
		// pre-release box cannot overwrite the user's resize and snap the pane back.
		settleFrame.current = requestAnimationFrame(() => {
			settleFrame.current = requestAnimationFrame(() => {
				settleFrame.current = null;
				settling.current = false;
				persistRealized(target);
			});
		});
	}, [cancelSettlement, persistRealized]);

	const resize = useCallback((clientX: number, target: HTMLElement) => {
		const max = measureMaximum(target);
		if (direction === "primary") {
			const parent = target.parentElement;
			if (!parent) return;
			const bounds = parent.getBoundingClientRect();
			onChange(clampPaneWidth(clientX - bounds.left, minPrimary, max));
			return;
		}
		if (direction === "sidebar") {
			const appRow = target.parentElement?.parentElement;
			if (!appRow) return;
			const bounds = appRow.getBoundingClientRect();
			onChange(clampPaneWidth(clientX - bounds.left, minSecondary, max));
			return;
		}
		const parent = target.parentElement;
		if (!parent) return;
		const bounds = parent.getBoundingClientRect();
		onChange(clampPaneWidth(bounds.right - clientX, minSecondary, max));
	}, [direction, measureMaximum, minPrimary, minSecondary, onChange]);

	const release = useCallback(() => {
		const target = handleRef.current;
		const pointerId = activePointer.current;
		activePointer.current = null;
		if (target && pointerId !== null && target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
		if (target) settleAfterLayout(target);
	}, [settleAfterLayout]);

	useEffect(() => {
		const target = handleRef.current;
		if (!target) return;
		const named = namedPaneElement(target, direction);
		const parentObserved = direction === "sidebar" ? target.parentElement?.parentElement : target.parentElement;
		const measure = () => {
			// A stacked responsive layout hides the separator and lets the named
			// pane fill the row. That temporary width is not a user resize and must
			// never overwrite the persisted split-view preference.
			if (target.getClientRects().length === 0 || getComputedStyle(target).display === "none") return;
			setMaximum(Math.round(measureMaximum(target)));
			if (!named) return;
			const cssWidth = Math.round(named.getBoundingClientRect().width);
			if (!Number.isFinite(cssWidth) || cssWidth < 1) return;
			setRealized(cssWidth);
			if (activePointer.current === null && !settling.current && Math.abs(cssWidth - valueRef.current) >= 1) {
				onChangeRef.current(cssWidth);
			}
		};
		measure();
		const observer = new ResizeObserver(measure);
		if (parentObserved) observer.observe(parentObserved);
		if (named) observer.observe(named);
		window.addEventListener("blur", release);
		return () => {
			observer.disconnect();
			window.removeEventListener("blur", release);
			cancelSettlement();
			const pointerId = activePointer.current;
			activePointer.current = null;
			if (pointerId !== null && target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
		};
	}, [cancelSettlement, direction, measureMaximum, release]);

	const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
		event.preventDefault();
		cancelSettlement();
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
		const current = realizedPaneWidth(value, minimum, maximum, realized);
		const next = applyKeyboardResize({
			key: event.key,
			shiftKey: event.shiftKey,
			value: current,
			min: minimum,
			max: maximum
		});
		if (next == null) return;
		event.preventDefault();
		onChange(next);
	};

	const reported = realizedPaneWidth(value, minimum, maximum, realized);

	return <div
		ref={handleRef}
		className={`pane-resize-handle${direction === "sidebar" ? " sidebar-resize-handle" : ""}${direction === "primary" ? " primary-resize-handle" : ""}`}
		role="separator"
		aria-label={ariaLabel}
		aria-orientation="vertical"
		aria-valuemin={minimum}
		aria-valuemax={maximum}
		aria-valuenow={reported}
		aria-valuetext={paneKeyboardValueText(reported)}
		tabIndex={0}
		data-testid={direction === "sidebar" ? "sidebar-resize-handle" : direction === "primary" ? "visuals-resize-handle" : "pane-resize-handle"}
		onPointerDown={onPointerDown}
		onPointerMove={onPointerMove}
		onPointerUp={onPointerUp}
		onPointerCancel={onPointerUp}
		onLostPointerCapture={() => {
			activePointer.current = null;
			const target = handleRef.current;
			if (target) settleAfterLayout(target);
		}}
		onKeyDown={onKeyDown}
		onDoubleClick={() => onChange(clampPaneWidth(resolvedResetValue, minimum, maximum))}
	/>;
}
