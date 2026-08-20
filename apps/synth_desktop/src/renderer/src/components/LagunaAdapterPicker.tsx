import { useEffect, useRef, useState } from "react";
import type { LagunaAdapterOption } from "../runtime/lagunaAdapters";

type Props = {
	adapters: LagunaAdapterOption[];
	selectedId: string | null;
	onSelect: (checkpointId: string | null) => void;
	disabled?: boolean;
	variant: "landing" | "composer";
};

export function LagunaAdapterPicker({
	adapters,
	selectedId,
	onSelect,
	disabled = false,
	variant
}: Props) {
	const [open, setOpen] = useState(false);
	const ref = useRef<HTMLDivElement>(null);
	const selected = adapters.find((adapter) => adapter.checkpointId === selectedId);
	const label = selected?.name ?? "Base model";

	useEffect(() => {
		if (!open) return;
		const onDocClick = (event: MouseEvent) => {
			if (!ref.current?.contains(event.target as Node)) setOpen(false);
		};
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") setOpen(false);
		};
		document.addEventListener("mousedown", onDocClick);
		document.addEventListener("keydown", onKey);
		return () => {
			document.removeEventListener("mousedown", onDocClick);
			document.removeEventListener("keydown", onKey);
		};
	}, [open]);

	const pick = (checkpointId: string | null) => {
		onSelect(checkpointId);
		setOpen(false);
	};

	const wrapClass = variant === "landing" ? "model-picker-wrap" : "composer-model-wrap";
	const buttonClass = variant === "landing"
		? `model-pill${open ? " open" : ""}`
		: `model-chip${open ? " open" : ""}`;
	const menuClass = variant === "landing" ? "model-dropdown" : "composer-model-menu";
	const optionClass = variant === "landing" ? "model-option" : "composer-model-option";

	return (
		<div className={wrapClass} ref={ref}>
			<button
				type="button"
				className={buttonClass}
				disabled={disabled}
				onClick={() => setOpen((current) => !current)}
				aria-label={`Laguna adapter: ${label}`}
				aria-expanded={open}
				aria-controls="laguna-adapter-menu"
				aria-haspopup="listbox"
				data-testid="laguna-adapter-picker"
			>
				<span className={variant === "landing" ? "model-pill-label" : "model-chip-label"}>{label}</span>
				<svg className="model-pill-chevron" width="12" height="12" viewBox="0 0 12 12" aria-hidden>
					<path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" fill="none" />
				</svg>
			</button>
			{open ? (
				<div id="laguna-adapter-menu" className={menuClass} role="listbox" data-testid="laguna-adapter-menu">
					<button
						type="button"
						role="option"
						aria-selected={!selectedId}
						className={`${optionClass}${selectedId ? "" : " selected"}`}
						data-testid="laguna-adapter-option-base"
						onClick={() => pick(null)}
					>
						{variant === "landing" ? (
							<>
								<span className="model-option-label">Base model</span>
								<span className="model-option-desc">Laguna XS 2.1 without a LoRA</span>
							</>
						) : (
							<span className="composer-model-option-main">
								<span className="composer-model-option-label">Base model</span>
								<span className="composer-model-option-desc">Laguna XS 2.1 without a LoRA</span>
							</span>
						)}
					</button>
					{adapters.map((adapter) => {
						const selectedHere = adapter.checkpointId === selectedId;
						return (
							<button
								key={adapter.checkpointId}
								type="button"
								role="option"
								aria-selected={selectedHere}
								className={`${optionClass}${selectedHere ? " selected" : ""}`}
								data-testid={`laguna-adapter-option-${adapter.checkpointId}`}
								onClick={() => pick(adapter.checkpointId)}
							>
								{variant === "landing" ? (
									<>
										<span className="model-option-label">{adapter.name}</span>
										<span className="model-option-desc">Loads on this chat · This Mac LoRA</span>
									</>
								) : (
									<span className="composer-model-option-main">
										<span className="composer-model-option-label">{adapter.name}</span>
										<span className="composer-model-option-desc">Loads on this chat · This Mac LoRA</span>
									</span>
								)}
							</button>
						);
					})}
				</div>
			) : null}
		</div>
	);
}
