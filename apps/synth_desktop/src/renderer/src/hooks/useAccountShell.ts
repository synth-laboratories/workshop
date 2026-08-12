import { useCallback, useEffect, useMemo, useState } from "react";
import { buildAccountView } from "../runtime/accountView";
import { loadDeviceUsage } from "../runtime/deviceUsage";
import type { DeviceUsageSummary } from "../components/UsageSheet";
import type { SynthAccountSummary, SynthBackendSettings } from "../bridge";

/**
 * Account / billing shell state. Keeps the Account Snapshot refresh path out of
 * App.tsx so the shell can trend toward fewer local useState calls.
 */
export function useAccountShell(showToast: (message: string) => void) {
	const [apiKeyConfigured, setApiKeyConfigured] = useState(false);
	const [backendSettings, setBackendSettings] = useState<SynthBackendSettings | null>(null);
	const [accountUsage, setAccountUsage] = useState<DeviceUsageSummary | null>(null);
	const [accountSummary, setAccountSummary] = useState<SynthAccountSummary | null>(null);
	const [usageSheetOpen, setUsageSheetOpen] = useState(false);

	const refreshAccountSummary = useCallback((force = false) => {
		const bridge = window.synthAccount;
		if (typeof bridge?.getSummary !== "function") {
			setAccountSummary(null);
			return;
		}
		const read =
			force && typeof bridge.refresh === "function" ? bridge.refresh() : bridge.getSummary();
		void read.then(setAccountSummary).catch(() => setAccountSummary(null));
	}, []);

	useEffect(() => {
		refreshAccountSummary();
		void loadDeviceUsage()
			.then(setAccountUsage)
			.catch(() => setAccountUsage(null));
	}, [refreshAccountSummary]);

	const accountView = useMemo(
		() => buildAccountView(accountSummary, apiKeyConfigured),
		[accountSummary, apiKeyConfigured]
	);

	const openBilling = useCallback(
		async (action: "upgrade" | "manage") => {
			const bridge = window.synthAccount;
			if (typeof bridge?.openBilling !== "function") {
				showToast("Billing management requires Synth Desktop");
				return;
			}
			try {
				await bridge.openBilling(action, accountSummary?.billing?.upgradeTier);
				showToast(
					action === "upgrade"
						? "Finish your upgrade in the browser"
						: "Manage billing opened in your browser"
				);
				window.setTimeout(() => refreshAccountSummary(true), 4_000);
			} catch (reason) {
				showToast(reason instanceof Error ? reason.message : String(reason));
			}
		},
		[accountSummary?.billing?.upgradeTier, refreshAccountSummary, showToast]
	);

	return {
		apiKeyConfigured,
		setApiKeyConfigured,
		backendSettings,
		setBackendSettings,
		accountUsage,
		setAccountUsage,
		accountSummary,
		usageSheetOpen,
		setUsageSheetOpen,
		refreshAccountSummary,
		accountView,
		openBilling
	};
}
