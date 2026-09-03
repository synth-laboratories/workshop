import type { LandingState } from "../types/landing";
import type { AccountViewModel } from "../runtime/accountView";
import type { SynthAccountSummary } from "../bridge";
import { ConversationSearch } from "./ConversationSearch";
import { UsageSheet } from "./UsageSheet";
import { ZoomHud } from "./ZoomHud";

export type AppOverlaysProps = {
	searchOpen: boolean;
	state: LandingState;
	onCloseSearch: () => void;
	onOpenChat: (id: string) => void;
	usageSheetOpen: boolean;
	accountView: AccountViewModel;
	accountSummary: SynthAccountSummary | null;
	onCloseUsage: () => void;
	onSignIn: () => void;
	onBilling: (action: "upgrade" | "manage") => void;
	onRetryAccount: () => void;
	onOpenDeviceUsage: () => void;
	toast: string | null;
};

/** Floating overlays owned by the App shell (search, usage, toast). */
export function AppOverlays({
	searchOpen,
	state,
	onCloseSearch,
	onOpenChat,
	usageSheetOpen,
	accountView,
	accountSummary,
	onCloseUsage,
	onSignIn,
	onBilling,
	onRetryAccount,
	onOpenDeviceUsage,
	toast
}: AppOverlaysProps) {
	return (
		<>
			{searchOpen ? (
				<ConversationSearch state={state} onClose={onCloseSearch} onOpenChat={onOpenChat} />
			) : null}

			<UsageSheet
				open={usageSheetOpen}
				view={accountView}
				summary={accountSummary}
				onClose={onCloseUsage}
				onSignIn={onSignIn}
				onBilling={onBilling}
				onRetry={onRetryAccount}
				onOpenDeviceUsage={onOpenDeviceUsage}
			/>

			{toast ? (
				<div className="toast" role="status" key={toast}>
					{toast}
				</div>
			) : null}

			<ZoomHud />
		</>
	);
}
