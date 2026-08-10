import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Editor for the Laguna daemon's runtime settings (`/v1/synth/settings`).
 *
 * The daemon owns the values: every commit is a partial PUT, and the form
 * reconciles to the effective settings the daemon answers with. A 404 marks a
 * daemon build that predates the settings API and renders as a quiet
 * unsupported notice, never a broken form. This file stays free of relative
 * runtime imports so the node --test compile step can load it standalone.
 */

/** Wire shape of the daemon's `settings` object — snake_case per contract. */
export type DaemonSettings = {
	default_temperature: number;
	default_top_p: number;
	default_top_k: number;
	default_reasoning_effort: string;
	default_max_output_tokens: number;
	idle_unload_after_seconds: number;
	prompt_cache_slots: number;
	queue_capacity: number;
};

/** One `/v1/synth/settings` exchange as forwarded by the host process. */
export type SettingsExchange = {
	supported: boolean;
	status: number;
	/** Parsed JSON body; `null` when the daemon answered without one. */
	body: unknown;
};

/** Everything the editor needs from the host process, injectable for tests. */
export type SettingsTransport = {
	snapshot(): Promise<SettingsExchange>;
	update(patch: Partial<DaemonSettings>): Promise<SettingsExchange>;
};

export type SettingsView =
	| { state: "loading" }
	| { state: "unsupported" }
	| { state: "error"; message: string }
	| { state: "ready"; settings: DaemonSettings };

export type SettingsRejection = {
	field: keyof DaemonSettings | null;
	message: string;
};

export type SettingsController = {
	view: SettingsView;
	/** The last commit the daemon rejected, with its typed message. */
	rejection: SettingsRejection | null;
	commit: (field: keyof DaemonSettings, value: number | string) => void;
	commitPatch: (patch: Partial<DaemonSettings>) => void;
	retry: () => void;
};

/** Measured defaults for a local coding workload on the 64 GB Laguna host. */
export const CALIBRATED_LOCAL_CODING_SETTINGS: Partial<DaemonSettings> = {
	default_temperature: 1,
	default_top_p: 1,
	default_top_k: 20,
	default_reasoning_effort: "high",
	idle_unload_after_seconds: 900,
	prompt_cache_slots: 4,
	queue_capacity: 9
};

/* --------------------------------------------------------------- protocol */

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function settingsFromBody(body: unknown): DaemonSettings | null {
	if (!isRecord(body) || !isRecord(body.settings)) return null;
	return body.settings as unknown as DaemonSettings;
}

/** The daemon's typed error message, or an honest status-based fallback. */
export function rejectionMessage(exchange: SettingsExchange): string {
	if (isRecord(exchange.body) && isRecord(exchange.body.error)) {
		const message = exchange.body.error.message;
		if (typeof message === "string" && message.trim() !== "") return message;
	}
	return `The daemon rejected the update (HTTP ${exchange.status}).`;
}

export function interpretSnapshot(exchange: SettingsExchange): SettingsView {
	if (!exchange.supported) return { state: "unsupported" };
	const settings = settingsFromBody(exchange.body);
	if (exchange.status >= 200 && exchange.status < 300 && settings) {
		return { state: "ready", settings };
	}
	return { state: "error", message: rejectionMessage(exchange) };
}

export type CommitOutcome = { settings: DaemonSettings } | { rejection: string };

/** One partial PUT. The daemon's effective settings are the only truth. */
export async function commitSettings(
	transport: SettingsTransport,
	patch: Partial<DaemonSettings>
): Promise<CommitOutcome> {
	const exchange = await transport.update(patch);
	if (!exchange.supported) {
		return { rejection: "This daemon does not support runtime settings yet." };
	}
	const settings = settingsFromBody(exchange.body);
	if (exchange.status >= 200 && exchange.status < 300 && settings) {
		return { settings };
	}
	return { rejection: rejectionMessage(exchange) };
}

export function describeSettingsFailure(reason: unknown): string {
	if (typeof reason === "string") return reason;
	if (reason instanceof Error) return reason.message;
	return "Laguna runtime settings are unavailable.";
}

/* -------------------------------------------------------------- transport */

const unavailableTransport: SettingsTransport = {
	snapshot: () => Promise.reject(new Error("Runtime settings require the desktop app")),
	update: () => Promise.reject(new Error("Runtime settings require the desktop app"))
};

let tauriTransport: SettingsTransport | null = null;

function isTauri(): boolean {
	return (
		typeof window !== "undefined" &&
		(window.location.protocol === "tauri:" || "__TAURI_INTERNALS__" in window)
	);
}

export function defaultSettingsTransport(): SettingsTransport {
	if (!isTauri()) return unavailableTransport;
	tauriTransport ??= {
		snapshot: () => invoke<SettingsExchange>("laguna_settings_snapshot"),
		update: (patch) => invoke<SettingsExchange>("laguna_settings_update", { patch })
	};
	return tauriTransport;
}

/* ------------------------------------------------------------------- hook */

export function useSettingsController(
	supplied?: SettingsTransport,
	enabled = true
): SettingsController {
	const transport = useMemo(() => supplied ?? defaultSettingsTransport(), [supplied]);
	const [attempt, setAttempt] = useState(0);
	const [view, setView] = useState<SettingsView>({ state: "loading" });
	const [rejection, setRejection] = useState<SettingsRejection | null>(null);

	useEffect(() => {
		if (!enabled) return;
		let disposed = false;
		setView({ state: "loading" });
		setRejection(null);
		transport
			.snapshot()
			.then((exchange) => {
				if (!disposed) setView(interpretSnapshot(exchange));
			})
			.catch((reason: unknown) => {
				if (!disposed) setView({ state: "error", message: describeSettingsFailure(reason) });
			});
		return () => {
			disposed = true;
		};
	}, [attempt, enabled, transport]);

	const commitPatch = useCallback(
		(patch: Partial<DaemonSettings>, field: keyof DaemonSettings | null = null) => {
			setRejection(null);
			// Optimistic: the fields show the committed values immediately, then
			// reconciles to whatever the daemon reports as effective.
			setView((current) =>
				current.state === "ready"
					? { state: "ready", settings: { ...current.settings, ...patch } }
					: current
			);
			const reconcile = () =>
				transport
					.snapshot()
					.then((exchange) => setView(interpretSnapshot(exchange)))
					.catch(() => undefined);
			commitSettings(transport, patch)
				.then((outcome) => {
					if ("settings" in outcome) {
						setView({ state: "ready", settings: outcome.settings });
						return;
					}
					setRejection({ field, message: outcome.rejection });
					// The optimistic value was refused; restore the daemon's truth.
					void reconcile();
				})
				.catch((reason: unknown) => {
					setRejection({ field, message: describeSettingsFailure(reason) });
					void reconcile();
				});
		},
		[transport]
	);
	const commit = useCallback(
		(field: keyof DaemonSettings, value: number | string) => {
			commitPatch({ [field]: value }, field);
		},
		[commitPatch]
	);

	const retry = useCallback(() => setAttempt((value) => value + 1), []);

	return { view, rejection, commit, commitPatch, retry };
}

/* --------------------------------------------------------------- rendering */

function SettingField({
	label,
	value,
	min,
	max,
	step,
	testId,
	caption,
	error,
	onCommit
}: {
	label: string;
	value: number;
	min: number;
	max: number;
	step: number;
	testId: string;
	caption?: string;
	error: string | null;
	onCommit: (value: number) => void;
}) {
	const [draft, setDraft] = useState(String(value));
	const [localError, setLocalError] = useState<string | null>(null);
	useEffect(() => { setDraft(String(value)); setLocalError(null); }, [value]);
	const message = localError ?? error;
	return (
		<label className="pref-field">
			<span>{label}</span>
			<input
				type="number"
				min={min}
				max={max}
				step={step}
				value={draft}
				data-testid={testId}
				aria-invalid={Boolean(message)}
				aria-describedby={message ? `${testId}-error` : undefined}
				onChange={(event) => setDraft(event.target.value)}
				onBlur={() => {
					const next = Number(draft);
					if (!Number.isFinite(next) || next < min || next > max) {
						setLocalError(`Enter a number between ${min} and ${max}`);
						setDraft(String(value));
						return;
					}
					setLocalError(null);
					onCommit(step >= 1 ? Math.round(next) : next);
				}}
			/>
			{message ? (
				<span id={`${testId}-error`} className="pref-field-error" role="alert" data-testid={`${testId}-error`}>
					{message}
				</span>
			) : null}
			{caption ? <small className="pref-field-caption">{caption}</small> : null}
		</label>
	);
}

const REASONING_OPTIONS: Array<{ effort: string; label: string }> = [
	{ effort: "none", label: "Off" },
	{ effort: "high", label: "On" }
];

/** Output-token choices follow 1024 × 2^k through the daemon's 32K limit. */
export const OUTPUT_TOKEN_OPTIONS = [1024, 2048, 4096, 8192, 16384, 32768] as const;

export type InferenceSettingsProps = {
	transport?: SettingsTransport;
	/** Supply to hoist the fetch/commit lifecycle into the caller or a test. */
	controller?: SettingsController;
};

export function InferenceSettings({ transport, controller }: InferenceSettingsProps) {
	// The hook is always called; it stays inert when the caller owns the state.
	const internal = useSettingsController(transport, !controller);
	const control = controller ?? internal;
	const { view, rejection } = control;
	// Daemon identity for the unsupported notice; the bridge already carries it.
	const [daemon, setDaemon] = useState<{ detail: string | null; baseUrl: string | null } | null>(null);

	useEffect(() => {
		if (view.state !== "unsupported") return;
		void window.synthLaguna?.getStatus()
			.then((status) => setDaemon({ detail: status.detail ?? status.phase, baseUrl: status.baseUrl }))
			.catch(() => undefined);
	}, [view.state]);

	if (view.state === "loading") {
		return (
			<p className="settings-runtime-copy" role="status" data-testid="inference-settings-loading">
				Reading daemon runtime settings…
			</p>
		);
	}

	if (view.state === "unsupported") {
		return (
			<div className="finetune-base-card" data-testid="inference-settings-unsupported">
				<span className="finetune-kicker">Runtime settings</span>
				<strong>This daemon does not support runtime settings yet.</strong>
				<span className="finetune-meta">
					{daemon?.detail ?? "Daemon identity unavailable"}
					{daemon?.baseUrl ? ` · ${daemon.baseUrl}` : ""}
				</span>
			</div>
		);
	}

	if (view.state === "error") {
		return (
			<div className="settings-finetunes" data-testid="inference-settings-failed">
				<p className="inference-error" role="alert" data-testid="inference-settings-error">
					{view.message}
				</p>
				<button type="button" className="inference-retry" onClick={control.retry}>
					Try again
				</button>
			</div>
		);
	}

	const settings = view.settings;
	const fieldError = (field: keyof DaemonSettings) =>
		rejection?.field === field ? rejection.message : null;

	return (
		<div data-testid="inference-settings">
			<section className="pref-section" aria-labelledby="inference-preset" data-testid="inference-preset">
				<h3 id="inference-preset">Local coding preset</h3>
				<p className="settings-runtime-copy">
					Applies the measured Laguna recipe for this 64 GB host while preserving your output-token limit.
				</p>
				<button
					type="button"
					className="inference-retry"
					data-testid="inference-apply-calibrated-preset"
					onClick={() => control.commitPatch(CALIBRATED_LOCAL_CODING_SETTINGS)}
				>
					Use calibrated defaults
				</button>
				<p className="settings-runtime-copy">
					Temperature 1.0 · top p 1.0 · top k 20 · reasoning on · unload after 15 minutes · 4 cache slots · queue 9
				</p>
				{rejection?.field === null ? (
					<span className="pref-field-error" role="alert" data-testid="inference-preset-error">
						{rejection.message}
					</span>
				) : null}
			</section>

			<section className="pref-section" aria-labelledby="inference-sampling" data-testid="inference-sampling">
				<h3 id="inference-sampling">Sampling defaults</h3>
				<p className="settings-runtime-copy">Used only when a request does not specify its own value.</p>
				<div className="pref-grid">
					<SettingField
						label="Temperature"
						value={settings.default_temperature}
						min={0}
						max={2}
						step={0.1}
						testId="inference-default-temperature"
						error={fieldError("default_temperature")}
						onCommit={(value) => control.commit("default_temperature", value)}
					/>
					<SettingField
						label="Top p"
						value={settings.default_top_p}
						min={0}
						max={1}
						step={0.05}
						testId="inference-default-top-p"
						error={fieldError("default_top_p")}
						onCommit={(value) => control.commit("default_top_p", value)}
					/>
					<SettingField
						label="Top k"
						value={settings.default_top_k}
						min={0}
						max={8192}
						step={1}
						testId="inference-default-top-k"
						error={fieldError("default_top_k")}
						onCommit={(value) => control.commit("default_top_k", value)}
					/>
				</div>
				<p className="settings-runtime-copy">
					Poolside’s published recipe is temperature 1.0 · top_k 20 · top_p 1.0.
				</p>
			</section>

			<section className="pref-section" aria-labelledby="inference-reasoning" data-testid="inference-reasoning">
				<h3 id="inference-reasoning">Reasoning default</h3>
				<div className="pref-chip-row" role="radiogroup" aria-label="Reasoning default">
					{REASONING_OPTIONS.map((option) => (
						<button
							key={option.effort}
							type="button"
							role="radio"
							aria-checked={settings.default_reasoning_effort === option.effort}
							className={settings.default_reasoning_effort === option.effort ? "active" : ""}
							data-testid={`inference-reasoning-${option.effort}`}
							onClick={() => control.commit("default_reasoning_effort", option.effort)}
						>
							{option.label}
						</button>
					))}
				</div>
				{fieldError("default_reasoning_effort") ? (
					<span className="pref-field-error" role="alert" data-testid="inference-reasoning-error">
						{fieldError("default_reasoning_effort")}
					</span>
				) : null}
			</section>

			<section className="pref-section" aria-labelledby="inference-output" data-testid="inference-output">
				<h3 id="inference-output">Output tokens default</h3>
				<div className="pref-grid">
					<label className="pref-field">
						<span>Max output tokens</span>
						<select
							value={settings.default_max_output_tokens}
							data-testid="inference-default-max-output-tokens"
							aria-invalid={Boolean(fieldError("default_max_output_tokens"))}
							onChange={(event) => control.commit("default_max_output_tokens", Number(event.target.value))}
						>
							{!OUTPUT_TOKEN_OPTIONS.includes(settings.default_max_output_tokens as typeof OUTPUT_TOKEN_OPTIONS[number]) ? (
								<option value={settings.default_max_output_tokens} disabled>
									{settings.default_max_output_tokens.toLocaleString()} (custom)
								</option>
							) : null}
							{OUTPUT_TOKEN_OPTIONS.map((tokens) => (
								<option key={tokens} value={tokens}>{tokens.toLocaleString()}</option>
							))}
						</select>
						{fieldError("default_max_output_tokens") ? (
							<span className="pref-field-error" role="alert" data-testid="inference-default-max-output-tokens-error">
								{fieldError("default_max_output_tokens")}
							</span>
						) : null}
						<small className="pref-field-caption">1,024 × 2^k</small>
					</label>
				</div>
			</section>

			<section className="pref-section" aria-labelledby="inference-runtime" data-testid="inference-runtime">
				<h3 id="inference-runtime">Runtime</h3>
				<div className="pref-grid">
					<SettingField
						label="Idle unload (minutes)"
						value={Math.round(settings.idle_unload_after_seconds / 60)}
						min={0}
						max={10080}
						step={1}
						testId="inference-idle-unload-minutes"
						caption="0 = never unload"
						error={fieldError("idle_unload_after_seconds")}
						onCommit={(minutes) => control.commit("idle_unload_after_seconds", minutes * 60)}
					/>
					<SettingField
						label="Prompt cache slots"
						value={settings.prompt_cache_slots}
						min={1}
						max={32}
						step={1}
						testId="inference-prompt-cache-slots"
						error={fieldError("prompt_cache_slots")}
						onCommit={(value) => control.commit("prompt_cache_slots", value)}
					/>
					<SettingField
						label="Queue capacity"
						value={settings.queue_capacity}
						min={1}
						max={32}
						step={1}
						testId="inference-queue-capacity"
						error={fieldError("queue_capacity")}
						onCommit={(value) => control.commit("queue_capacity", value)}
					/>
				</div>
			</section>

		</div>
	);
}
