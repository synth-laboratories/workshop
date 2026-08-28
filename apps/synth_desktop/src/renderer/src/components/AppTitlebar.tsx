import { useEffect, useRef, useState } from "react";
import { SynthLogo } from "./SynthLogo";
import { ProviderMark } from "./ProviderMark";
import { truncate } from "../runtime/codexTurn";
import { BUILD_TIER } from "../flags/tier";
import type { SidePanelTab } from "../hooks/useShellLayout";

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

function IconCopy() {
	return (
		<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
			<rect x="5.25" y="2.25" width="8.5" height="9.5" rx="1.5" stroke="currentColor" strokeWidth="1.25" />
			<path d="M10.75 12.25v.5a1.5 1.5 0 0 1-1.5 1.5h-6a1.5 1.5 0 0 1-1.5-1.5v-6a1.5 1.5 0 0 1 1.5-1.5h.5" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
		</svg>
	);
}

function IconEllipsis() {
	return (
		<svg width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden>
			<circle cx="4" cy="10" r="1.6" fill="currentColor" />
			<circle cx="10" cy="10" r="1.6" fill="currentColor" />
			<circle cx="16" cy="10" r="1.6" fill="currentColor" />
		</svg>
	);
}

export type AppTitlebarProps = {
	tabLabel: string;
	appVersion: string;
	activeLocalModel: boolean;
	terminalOpen: boolean;
	sidePanelOpen: boolean;
	sidePanelTab: SidePanelTab;
	reserveNativeControls?: boolean;
	brand?: "synth" | "openai";
	copyItems?: TabCopyItem[];
	onCopyItem?: (item: TabCopyItem) => Promise<void>;
	onCloseTab: () => void;
	onNewConversation: () => void;
	onToggleTerminal: () => void;
	onToggleInference: () => void;
};

export type TabCopyItem = {
	id: "working-directory" | "session-id" | "markdown";
	label: string;
	successMessage: string;
	value: string;
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
	copyItems = [],
	onCopyItem,
	onCloseTab,
	onNewConversation,
	onToggleTerminal,
	onToggleInference
}: AppTitlebarProps) {
	const [menuOpen, setMenuOpen] = useState(false);
	const menuRef = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!menuOpen) return;
		const closeOnPointerDown = (event: PointerEvent) => {
			if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
		};
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") setMenuOpen(false);
		};
		document.addEventListener("pointerdown", closeOnPointerDown);
		document.addEventListener("keydown", closeOnEscape);
		return () => {
			document.removeEventListener("pointerdown", closeOnPointerDown);
			document.removeEventListener("keydown", closeOnEscape);
		};
	}, [menuOpen]);

	return (
		<header className={`titlebar${reserveNativeControls ? " titlebar-native-inset" : ""}`} data-testid="titlebar" data-tauri-drag-region="">
			<div className="titlebar-tabs" data-tauri-drag-region="">
				<div className="tab tab-active" role="group" aria-label={`${tabLabel} chat tab`} data-tauri-drag-region="">
					{brand === "openai" ? (
						<ProviderMark kind="openai" className="tab-logo" />
					) : (
						<SynthLogo className="tab-logo" compact />
					)}
					<span>{truncate(tabLabel, 28)}</span>
					{copyItems.length > 0 && onCopyItem ? (
						<div className="tab-menu" ref={menuRef}>
							<button
								type="button"
								className="tab-menu-trigger"
								aria-label="Chat tab actions"
								aria-haspopup="menu"
								aria-expanded={menuOpen}
								title="Copy chat details"
								onPointerDown={(event) => event.stopPropagation()}
								onClick={(event) => {
									event.stopPropagation();
									setMenuOpen((open) => !open);
								}}
							>
								<IconEllipsis />
							</button>
							{menuOpen ? (
								<div className="tab-menu-popover" role="menu" aria-label="Copy chat details">
									<div className="tab-menu-heading">Copy</div>
									{copyItems.map((item) => (
										<button
											type="button"
											role="menuitem"
											className="tab-menu-item"
											key={item.id}
											onClick={() => {
												setMenuOpen(false);
												void onCopyItem(item);
											}}
										>
											<IconCopy />
											{item.label}
										</button>
									))}
								</div>
							) : null}
						</div>
					) : null}
					<button type="button" className="tab-close" aria-label="Close tab" onPointerDown={(event) => event.stopPropagation()} onClick={onCloseTab}>
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
				{/* Statically eliminated from stable/core bundles — the badge is
				    the beta-tier prerelease_build_badge feature, so the public
				    app is structurally unable to render it. */}
				{__TIER_HAS_BETA__ ? (
					<span
						className="tier-badge"
						data-testid="titlebar-tier-badge"
						title={`Pre-release ${BUILD_TIER} build — see Settings → Build`}
					>
						{BUILD_TIER}
					</span>
				) : null}
			</div>
		</header>
	);
}
