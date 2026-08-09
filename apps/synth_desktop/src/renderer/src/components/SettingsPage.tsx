import { useState } from "react";
import type { RuntimeHealth } from "@synth/runtime-protocol";
import {
	AVAILABLE_LORAS,
	LORA_NONE,
	type LoraAdapter
} from "../types/landing";

type Props = {
	selectedLoraId: string;
	onSelectLora: (id: string) => void;
	onBack: () => void;
	onAction: (label: string) => void;
	health?: RuntimeHealth | null;
	lagunaPhase?: string | null;
};

const SECTIONS = [
	{ id: "finetunes", label: "Finetunes" },
	{ id: "models", label: "Models" },
	{ id: "runtime", label: "Runtime" },
	{ id: "account", label: "Account" }
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

function statusLabel(status: LoraAdapter["status"]) {
	if (status === "ready") return "Ready";
	if (status === "downloading") return "Downloading";
	return "Training";
}

function FinetunesSection({
	selectedLoraId,
	onSelectLora,
	onAction
}: {
	selectedLoraId: string;
	onSelectLora: (id: string) => void;
	onAction: (label: string) => void;
}) {
	const local = AVAILABLE_LORAS.filter((l) => l.scope === "local");
	const remote = AVAILABLE_LORAS.filter((l) => l.scope === "remote");

	return (
		<div className="settings-finetunes" data-testid="settings-finetunes">
			<header className="settings-section-head">
				<div>
					<h2>Finetunes</h2>
					<p>
						LoRAs are first-class adapters on a base model — not opaque merged names. Local Metal
						first; same identity for remote when available.
					</p>
				</div>
				<button type="button" className="settings-secondary-btn" onClick={() => onAction("Install LoRA")}>
					Install LoRA
				</button>
			</header>

			<div className="finetune-base-card">
				<span className="finetune-kicker">Base</span>
				<strong>synth/Laguna-XS-2.1-NVFP4</strong>
				<span className="finetune-meta">Local · MLX · Metal</span>
			</div>

			<button
				type="button"
				className={`finetune-row${selectedLoraId === LORA_NONE ? " selected" : ""}`}
				onClick={() => onSelectLora(LORA_NONE)}
				data-testid="lora-base"
			>
				<div className="finetune-row-main">
					<span className="finetune-name">Base (no adapter)</span>
					<span className="finetune-summary">Stock Laguna XS — no LoRA loaded</span>
				</div>
				{selectedLoraId === LORA_NONE ? <span className="finetune-active">Active</span> : null}
			</button>

			<h3 className="finetune-group-label">Local adapters</h3>
			<div className="finetune-list">
				{local.map((lora) => (
					<button
						key={lora.id}
						type="button"
						className={`finetune-row${selectedLoraId === lora.id ? " selected" : ""}`}
						disabled={lora.status !== "ready"}
						onClick={() => onSelectLora(lora.id)}
						data-testid={`lora-${lora.id}`}
					>
						<div className="finetune-row-main">
							<span className="finetune-name">{lora.displayName}</span>
							<span className="finetune-file">{lora.name}</span>
							<span className="finetune-summary">{lora.summary}</span>
							<span className="finetune-meta">
								{lora.revision} · {lora.digest} · {statusLabel(lora.status)}
							</span>
						</div>
						{selectedLoraId === lora.id ? (
							<span className="finetune-active">Active</span>
						) : lora.status === "ready" ? (
							<span className="finetune-use">Use</span>
						) : (
							<span className="finetune-badge">{statusLabel(lora.status)}</span>
						)}
					</button>
				))}
			</div>

			<h3 className="finetune-group-label">Remote adapters</h3>
			<p className="finetune-group-note">
				Same adapter identity on OpenRouter / cloud when the host supports it.
			</p>
			<div className="finetune-list">
				{remote.map((lora) => (
					<button
						key={lora.id}
						type="button"
						className={`finetune-row${selectedLoraId === lora.id ? " selected" : ""}`}
						onClick={() => onSelectLora(lora.id)}
						data-testid={`lora-${lora.id}`}
					>
						<div className="finetune-row-main">
							<span className="finetune-name">{lora.displayName}</span>
							<span className="finetune-file">{lora.name}</span>
							<span className="finetune-summary">{lora.summary}</span>
							<span className="finetune-meta">
								{lora.revision} · {lora.digest} · remote
							</span>
						</div>
						{selectedLoraId === lora.id ? (
							<span className="finetune-active">Active</span>
						) : (
							<span className="finetune-use">Use</span>
						)}
					</button>
				))}
			</div>
		</div>
	);
}

export function SettingsPage({ selectedLoraId, onSelectLora, onBack, onAction, health, lagunaPhase }: Props) {
	const [section, setSection] = useState<SectionId>("finetunes");

	return (
		<div className="settings-page" data-testid="settings-page">
			<header className="settings-top">
				<button type="button" className="desk-back" onClick={onBack}>
					← Back
				</button>
				<h1>Settings</h1>
			</header>

			<div className="settings-body">
				<nav className="settings-nav" aria-label="Settings sections">
					{SECTIONS.map((s) => (
						<button
							key={s.id}
							type="button"
							className={`settings-nav-item${section === s.id ? " active" : ""}`}
							onClick={() => setSection(s.id)}
						>
							{s.label}
						</button>
					))}
				</nav>

				<div className="settings-content">
					{section === "finetunes" ? (
						<FinetunesSection
							selectedLoraId={selectedLoraId}
							onSelectLora={onSelectLora}
							onAction={onAction}
						/>
					) : null}
					{section === "models" ? (
						<div className="settings-finetunes" data-testid="settings-models">
							<header className="settings-section-head">
								<div><h2>Models</h2><p>Residency and adapter identity stay visible while you move between local Metal, remote models, and Intern.</p></div>
								<button type="button" className="settings-secondary-btn" onClick={() => onAction("Reload Laguna")}>Reload</button>
							</header>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Local base</span>
								<strong>Laguna XS 2.1 · NVFP4</strong>
								<span className="finetune-meta">{health?.local.mode === "mlx" ? "MLX / Metal resident" : "Stub boundary"} · {lagunaPhase ?? "unknown"}</span>
								<span className="finetune-file">{health?.local.modelPath ?? "Model path not detected"}</span>
							</div>
							<div className="settings-runtime-grid">
								<div><span>OpenRouter</span><strong>{health?.openrouter.mode ?? "connecting"}</strong></div>
								<div><span>Intern</span><strong>{health?.intern.mode ?? "connecting"}</strong></div>
								<div><span>Adapters</span><strong>{AVAILABLE_LORAS.filter((l) => l.status === "ready").length} ready</strong></div>
							</div>
						</div>
					) : null}
					{section === "runtime" ? (
						<div className="settings-finetunes" data-testid="settings-runtime">
							<h2>Runtime</h2>
							<p className="settings-runtime-copy">One append-only local authority owns sessions, runs, events, approvals, traces, visuals, and usage. The UI can inspect the store without leaving the workbench.</p>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Data store</span>
								<strong>{health?.dataStore?.events ?? 0} events · {health?.dataStore?.runs ?? 0} runs</strong>
								<span className="finetune-meta">{health?.dataStore?.projects ?? 0} projects · {health?.dataStore?.usage ?? 0} usage entries</span>
								<span className="finetune-file">{health?.dataStore?.path ?? "Runtime is connecting"}</span>
							</div>
							<div className="finetune-base-card">
								<span className="finetune-kicker">Intern routing</span>
								<strong>
									{health?.intern.mode === "remote"
										? "Cloud mailbox connected"
										: health?.intern.mode === "demo"
											? "Explicit demo mailbox"
											: "Setup required"}
								</strong>
								<span className="finetune-meta">
									{health?.intern.mode === "remote"
										? "Sync and Async mirror the cloud Intern through the local daemon."
										: health?.intern.mode === "demo"
											? "Scripted local events are active; no cloud mailbox is contacted."
											: "Choose cloud credentials or explicit demo mode before starting Intern."}
								</span>
								<code className="finetune-file">
									{health?.intern.mode === "unconfigured"
										? "SYNTH_API_KEY=… npm run dev:desktop"
										: health?.intern.mode === "demo"
											? "SYNTH_INTERN_DEMO=1 npm run dev:desktop"
											: health?.intern.backendUrl ?? "Cloud endpoint configured"}
								</code>
							</div>
						</div>
					) : null}
					{section === "account" ? (
						<div className="settings-placeholder">
							<h2>Account</h2>
							<p>Synth Cloud org and billing — stub.</p>
						</div>
					) : null}
				</div>
			</div>
		</div>
	);
}
