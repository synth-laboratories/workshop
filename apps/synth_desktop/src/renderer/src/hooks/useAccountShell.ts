import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { buildAccountView } from "../runtime/accountView";
import { loadDeviceUsage } from "../runtime/deviceUsage";
import type { DeviceUsageSummary } from "../components/UsageSheet";
import type { SynthAccountSummary, SynthBackendSettings } from "../bridge";
import { bridges } from "../runtime/desktopBridge";
import { BILLING_RETURN_KEY, billingIdentity, billingReturnState, parseBillingReturn, type BillingReturn } from "../runtime/billingReturn";
import { publicError } from "../runtime/publicError";

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

	const refreshSequence = useRef(0);
	const pendingBilling = useRef<BillingReturn | null>(null);
	const savePendingBilling = useCallback((value: BillingReturn | null) => {
		pendingBilling.current = value;
		try {
			if (value) localStorage.setItem(BILLING_RETURN_KEY, JSON.stringify(value));
			else localStorage.removeItem(BILLING_RETURN_KEY);
		} catch { /* Reconciliation continues in memory if browser storage is unavailable. */ }
	}, []);

	const refreshAccountSummary = useCallback((force = false) => {
		const bridge = bridges.account;
		if (typeof bridge?.getSummary !== "function") {
			setAccountSummary(null);
			return;
		}
		const read =
			force && typeof bridge.refresh === "function" ? bridge.refresh() : bridge.getSummary();
		const sequence = ++refreshSequence.current;
		void read.then((summary) => {
			if (sequence !== refreshSequence.current) return;
			setAccountSummary(summary);
			const pending = pendingBilling.current;
			if (pending && billingReturnState(pending, summary) !== "pending") savePendingBilling(null);
		}).catch(() => {
			if (sequence === refreshSequence.current) setAccountSummary(null);
		});
	}, [savePendingBilling]);

	useEffect(() => {
		// Relaunch is an account reconciliation boundary. Avoid rendering a stale
		// cached local-only snapshot before the paired cloud identity is checked.
		try { pendingBilling.current = parseBillingReturn(localStorage.getItem(BILLING_RETURN_KEY), Date.now()); }
		catch { pendingBilling.current = null; }
		refreshAccountSummary(true);
		const onFocus = () => refreshAccountSummary(true);
		const onVisibility = () => { if (document.visibilityState === "visible") onFocus(); };
		window.addEventListener("focus", onFocus);
		document.addEventListener("visibilitychange", onVisibility);
		const timer = window.setInterval(() => {
			const pending = pendingBilling.current;
			if (!pending) return;
			if (!parseBillingReturn(JSON.stringify(pending), Date.now())) { savePendingBilling(null); return; }
			refreshAccountSummary(true);
		}, 5_000);
		void loadDeviceUsage()
			.then(setAccountUsage)
			.catch(() => setAccountUsage(null));
		return () => {
			++refreshSequence.current;
			window.clearInterval(timer);
			window.removeEventListener("focus", onFocus);
			document.removeEventListener("visibilitychange", onVisibility);
		};
	}, [refreshAccountSummary, savePendingBilling]);

	const accountView = useMemo(
		() => buildAccountView(accountSummary, apiKeyConfigured),
		[accountSummary, apiKeyConfigured]
	);

	const openBilling = useCallback(
		async (action: "upgrade" | "manage") => {
			const bridge = bridges.account;
			if (typeof bridge?.openBilling !== "function") {
				showToast("Billing management requires Synth Desktop");
				return;
			}
			try {
				const identity = billingIdentity(accountSummary);
				if (identity) savePendingBilling({ identity, tier: action === "upgrade" ? accountSummary?.billing?.upgradeTier ?? null : null, startedAt: Date.now() });
				await bridge.openBilling(action, accountSummary?.billing?.upgradeTier);
				showToast(
					action === "upgrade"
						? "Finish your upgrade in the browser"
						: "Manage billing opened in your browser"
				);
				refreshAccountSummary(true);
			} catch (reason) {
				savePendingBilling(null);
				showToast(publicError(reason));
			}
		},
		[accountSummary, refreshAccountSummary, savePendingBilling, showToast]
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
