import { useEffect, useMemo, useState } from "react";
import { SettingsCard, SettingsRow } from "./SettingsCard";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";
import type { PaidComputeAutoApprovalSettings } from "../generated/protocol";
import { parseUsdAmount } from "../runtime/paidComputeUsd";

const PROVIDER_OPTIONS = [
	{ id: "openrouter", label: "OpenRouter" },
	{ id: "tinker", label: "Tinker" }
];
const PROVIDER_IDS = new Set(PROVIDER_OPTIONS.map(({ id }) => id));

const DEFAULT_SETTINGS: PaidComputeAutoApprovalSettings = {
	enabled: false,
	maxRequestUsd: "0.10",
	maxConversationUsd: "10.00",
	providers: []
};

export function PaidComputePermissionSettings() {
	const [settings, setSettings] = useState<PaidComputeAutoApprovalSettings>(DEFAULT_SETTINGS);
	const [approvalPolicy, setApprovalPolicy] = useState("untrusted");
	const [sandboxMode, setSandboxMode] = useState("workspace-write");
	const [maxRequestUsd, setMaxRequestUsd] = useState(DEFAULT_SETTINGS.maxRequestUsd);
	const [maxConversationUsd, setMaxConversationUsd] = useState(DEFAULT_SETTINGS.maxConversationUsd);
	const [error, setError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);

	const load = async () => {
		if (!bridges.config?.getDesktopPermissions) return;
		try {
			const next = await bridges.config.getDesktopPermissions();
			setApprovalPolicy(next.approvalPolicy);
			setSandboxMode(next.sandboxMode);
			const paid = next.paidCompute ?? DEFAULT_SETTINGS;
			setSettings(paid);
			setMaxRequestUsd(paid.maxRequestUsd);
			setMaxConversationUsd(paid.maxConversationUsd);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		}
	};

	useEffect(() => {
		void load();
	}, []);

	const requestError = useMemo(() => parseUsdAmount(maxRequestUsd).error, [maxRequestUsd]);
	const conversationError = useMemo(() => parseUsdAmount(maxConversationUsd).error, [maxConversationUsd]);

	const persist = async (next: PaidComputeAutoApprovalSettings) => {
		if (!bridges.config?.updateDesktopPermissions) return;
		setBusy(true);
		try {
			const supported = {
				...next,
				providers: next.providers.filter((provider) => PROVIDER_IDS.has(provider))
			};
			const stored = await bridges.config.updateDesktopPermissions({
				approvalPolicy,
				sandboxMode,
				paidCompute: supported
			});
			const paid = stored.paidCompute ?? DEFAULT_SETTINGS;
			setSettings(paid);
			setMaxRequestUsd(paid.maxRequestUsd);
			setMaxConversationUsd(paid.maxConversationUsd);
			setError(null);
		} catch (reason) {
			setError(publicError(reason));
		} finally {
			setBusy(false);
		}
	};

	const toggleProvider = (id: string, enabled: boolean) => {
		const providers = enabled
			? [...new Set([...settings.providers, id])]
			: settings.providers.filter((provider) => provider !== id);
		void persist({ ...settings, providers });
	};

	return (
		<SettingsCard
			title="Paid compute"
			description="Limits apply to each request’s hard ceiling, not predicted spend. Changes take effect on new conversations."
			testId="settings-paid-compute"
		>
			<SettingsRow
				label="Automatically approve small paid-compute requests"
				description="Eligible requests receive a capped, auditable approval without the blocking modal."
			>
				<label className="settings-toggle">
					<input
						type="checkbox"
						checked={settings.enabled}
						disabled={busy}
						data-testid="paid-compute-auto-approve"
						onChange={(event) => {
							const enabled = event.target.checked;
							void persist({
								...settings,
								enabled,
								providers: enabled && settings.providers.length === 0 ? ["openrouter"] : settings.providers
							});
						}}
					/>
					<span>{settings.enabled ? "On" : "Off"}</span>
				</label>
			</SettingsRow>
			<SettingsRow
				label="Maximum per request"
				description="USD hard ceiling a single request may auto-approve."
				htmlFor="paid-compute-max-request"
			>
				<input
					id="paid-compute-max-request"
					type="text"
					inputMode="decimal"
					value={maxRequestUsd}
					disabled={busy}
					aria-invalid={Boolean(requestError)}
					data-testid="paid-compute-max-request"
					onChange={(event) => setMaxRequestUsd(event.target.value)}
					onBlur={() => {
						if (requestError) {
							setError(requestError);
							return;
						}
						void persist({ ...settings, maxRequestUsd });
					}}
				/>
			</SettingsRow>
			<SettingsRow
				label="Maximum per conversation"
				description="USD hard-ceiling allowance for this conversation, including outstanding reservations."
				htmlFor="paid-compute-max-conversation"
			>
				<input
					id="paid-compute-max-conversation"
					type="text"
					inputMode="decimal"
					value={maxConversationUsd}
					disabled={busy}
					aria-invalid={Boolean(conversationError)}
					data-testid="paid-compute-max-conversation"
					onChange={(event) => setMaxConversationUsd(event.target.value)}
					onBlur={() => {
						if (conversationError) {
							setError(conversationError);
							return;
						}
						void persist({ ...settings, maxConversationUsd });
					}}
				/>
			</SettingsRow>
			<SettingsRow label="Allowed providers" description="Only listed providers can auto-approve.">
				<div className="settings-provider-toggles" data-testid="paid-compute-providers">
					{PROVIDER_OPTIONS.map((option) => (
						<label key={option.id} className="settings-toggle">
							<input
								type="checkbox"
								checked={settings.providers.includes(option.id)}
								disabled={busy}
								data-testid={`paid-compute-provider-${option.id}`}
								onChange={(event) => toggleProvider(option.id, event.target.checked)}
							/>
							<span>{option.label}</span>
						</label>
					))}
				</div>
			</SettingsRow>
			{error ? <p className="settings-error" data-testid="paid-compute-settings-error">{error}</p> : null}
		</SettingsCard>
	);
}
