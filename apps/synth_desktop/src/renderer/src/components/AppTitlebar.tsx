import { SynthLogo } from "./SynthLogo";
import { ProviderMark } from "./ProviderMark";
import { truncate } from "../runtime/codexTurn";

function IconSidePanel() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2.5" y="2.5" width="11" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
			<path d="M10 2.5v11" stroke="currentColor" strokeWidth="1.3" />
		</svg>
	);
}

function IconTerminal() {
	return (
		<svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="2.5" y="3" width="11" height="10" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
			<path
				d="M5 6.2l2 1.8L5 9.8M8.2 10.2H11"
				stroke="currentColor"
				strokeWidth="1.3"
				strokeLinecap="round"
				strokeLinejoin="round"
			/>
		</svg>
	);
}

export type AppTitlebarProps = {
	tabLabel: string;
	appVersion: string;
	activeLocalModel: boolean;
	terminalOpen: boolean;
	sidePanelOpen: boolean;
	sidePanelTab: "outputs" | "inference";
	reserveNativeControls?: boolean;
	brand?: "synth" | "openai";
	onCloseTab: () => void;
	onNewConversation: () => void;
	onToggleTerminal: () => void;
	onToggleInference: () => void;
};

export function AppTitlebar({
	tabLabel,
	appVersion,
	activeLocalModel,
	terminalOpen,
	sidePanelOpen,
	sidePanelTab,
	reserveNativeControls = false,
	brand = "synth",
	onCloseTab,
	onNewConversation,
	onToggleTerminal,
	onToggleInference
}: AppTitlebarProps) {
	return (
		<header className={`titlebar${reserveNativeControls ? " titlebar-native-inset" : ""}`} data-testid="titlebar" data-tauri-drag-region="">
			<div className="titlebar-tabs" data-tauri-drag-region="">
				<div className="tab tab-active" role="tab" aria-selected data-tauri-drag-region="">
					{brand === "openai" ? (
						<ProviderMark kind="openai" className="tab-logo" />
					) : (
						<SynthLogo className="tab-logo" compact />
					)}
					<span>{truncate(tabLabel, 28)}</span>
					<button type="button" className="tab-close" aria-label="Close tab" onClick={onCloseTab}>
						×
					</button>
				</div>
				{activeLocalModel ? (
					<button
						type="button"
						className="tab-new"
						aria-label="New tab"
						onClick={onNewConversation}
					>
						+
					</button>
				) : null}
			</div>
			<div className="titlebar-actions">
				<button
					type="button"
					className="titlebar-icon-btn"
					aria-label={terminalOpen ? "Hide terminal" : "Show terminal"}
					title="Toggle terminal (⌘J)"
					data-testid="toggle-terminal"
					onClick={onToggleTerminal}
				>
					<IconTerminal />
				</button>
				{activeLocalModel ? (
					<button
						type="button"
						className={`titlebar-icon-btn${sidePanelOpen && sidePanelTab === "inference" ? " active" : ""}`}
						aria-label={
							sidePanelOpen && sidePanelTab === "inference"
								? "Hide inference panel"
								: "Show inference panel"
						}
						aria-pressed={sidePanelOpen && sidePanelTab === "inference"}
						title="Local inference panel"
						data-testid="toggle-inference-rail"
						onClick={onToggleInference}
					>
						<IconSidePanel />
					</button>
				) : null}
				<span
					className="titlebar-version"
					data-testid="app-version"
					aria-label={`Synth Desktop version ${appVersion}`}
					title={`Synth Desktop v${appVersion}`}
				>
					v{appVersion}
				</span>
			</div>
		</header>
	);
}
