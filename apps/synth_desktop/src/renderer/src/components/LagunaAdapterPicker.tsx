import { useEffect, useRef, useState } from "react";
import type { LagunaPolicy } from "../bridge/types";
import { LOCAL_BASE_POLICY, orderedLagunaPolicies, policyLabel, policySpeed } from "../runtime/lagunaPolicies";
import { compactModelLabel } from "../runtime/modelPresentation";

type Props = {
	adapters: LagunaPolicy[];
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
	const selected = adapters.find((adapter) => adapter.modelId === selectedId);
	const label = selected ? policyLabel(selected) : policyLabel({ modelId: LOCAL_BASE_POLICY, isBase: true } as LagunaPolicy);

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
				{variant === "composer" ? <span className="model-chip-short-label" aria-hidden>{compactModelLabel(label)}</span> : null}
				<svg className="model-pill-chevron" width="12" height="12" viewBox="0 0 12 12" aria-hidden>
					<path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" fill="none" />
				</svg>
			</button>
			{open ? (
				<div id="laguna-adapter-menu" className={menuClass} role="listbox" data-testid="laguna-adapter-menu">
					{orderedLagunaPolicies(adapters).map((policy) => {
						const value = policy.isBase ? null : policy.modelId;
						const selectedHere = value === selectedId;
						const { rate, delta } = policySpeed(policy);
						const speed = delta ? `${rate} · ${delta}` : rate;
						return (
							<button
								key={policy.modelId}
								type="button"
								role="option"
								aria-selected={selectedHere}
								className={`${optionClass}${selectedHere ? " selected" : ""}`}
								data-testid={`laguna-adapter-option-${policy.isBase ? "base" : policy.modelId}`}
								onClick={() => pick(value)}
							>
								{variant === "landing" ? (
									<>
										<span className="model-option-label">{policyLabel(policy)}</span>
										<span className="model-option-desc">{speed}</span>
									</>
								) : (
									<span className="composer-model-option-main">
										<span className="composer-model-option-label">{policyLabel(policy)}</span>
										<span className="composer-model-option-desc">{speed}</span>
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
