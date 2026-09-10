import type { KeyboardEvent, ReactNode } from "react";
import { restoreFocusIfLost } from "../runtime/restoreFocus";
import { MittenFrame } from "./MittenFrame";

export type WorkbenchSidePanelTab = {
	id: string;
	label: string;
	/** Longer descriptive text retained for hover and assistive context. */
	title?: string;
	badge?: number;
	content: ReactNode;
	kind?: "panel" | "document";
	onClose?: () => void;
};

export function WorkbenchSidePanel({ tabs, activeTabId, onTabChange, onClose }: {
	tabs: WorkbenchSidePanelTab[];
	activeTabId: string;
	onTabChange: (tabId: string) => void;
	onClose: () => void;
}) {
	const panelTabs = tabs.filter((tab) => tab.kind !== "document");
	const documentTabs = tabs.filter((tab) => tab.kind === "document");
	const requestedTab = tabs.find((tab) => tab.id === activeTabId);
	const activeTab = requestedTab ?? panelTabs[0] ?? documentTabs[0];
	if (!activeTab) return null;
	const documentActive = activeTab.kind === "document";
	const primaryTabs = tabs;
	function closeSelectedTab() {
		if (activeTab.onClose) {
			activeTab.onClose();
			return;
		}
		onClose();
		restoreFocusIfLost('[data-testid="toggle-inference-rail"]');
	}
	function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>, index: number) {
		let nextIndex: number | null = null;
		if (event.key === "ArrowRight" || event.key === "ArrowDown") nextIndex = (index + 1) % primaryTabs.length;
		if (event.key === "ArrowLeft" || event.key === "ArrowUp") nextIndex = (index - 1 + primaryTabs.length) % primaryTabs.length;
		if (event.key === "Home") nextIndex = 0;
		if (event.key === "End") nextIndex = primaryTabs.length - 1;
		if (nextIndex === null) return;
		event.preventDefault();
		const nextTab = primaryTabs[nextIndex];
		if (!nextTab) return;
		onTabChange(nextTab.id);
		const tablist = event.currentTarget.closest<HTMLElement>('[role="tablist"]');
		const buttons = tablist?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
		buttons?.[nextIndex]?.focus();
	}
	return <aside id="workbench-side-panel" className="workbench-side-panel" data-testid="workbench-side-panel" aria-label="Workbench side panel">
		<MittenFrame thumbSelector=".workbench-side-panel-header .workbench-side-panel-tab-shell.is-selected" bodySelector=".workbench-side-panel-content" />
		<header className="workbench-side-panel-header">
			<div className="workbench-side-panel-tabs workbench-side-panel-option-tabs" role="tablist" aria-label="Side-panel views">
				{primaryTabs.map((item, index) => {
					const selected = item.id === activeTab.id;
					return <span className={`workbench-side-panel-tab-shell ${item.kind === "document" ? "is-document" : "is-panel"} ${selected ? "is-selected" : ""}`} key={item.id}>
						<button type="button" role="tab" title={item.title ?? item.label} aria-label={item.title ?? item.label} id={`workbench-side-tab-${item.id}`} aria-selected={selected} aria-controls="workbench-side-tabpanel" tabIndex={selected ? 0 : -1} data-testid={`workbench-side-tab-${item.id}`} onKeyDown={(event) => moveTabFocus(event, index)} onClick={() => onTabChange(item.id)}>
							<span className="workbench-side-tab-label">{item.label}</span>
							{item.badge ? <strong>{item.badge}</strong> : null}
						</button>
						{selected ? <button type="button" className="workbench-side-tab-close" aria-label={item.onClose ? `Close ${item.title ?? item.label}` : "Close side panel"} onClick={(event) => { event.stopPropagation(); closeSelectedTab(); }}>×</button> : null}
					</span>;
				})}
			</div>
		</header>
		<div className="workbench-side-panel-content" data-active-tab={activeTab.id} data-document-active={documentActive ? "true" : "false"} role="tabpanel" id="workbench-side-tabpanel" data-testid={`workbench-side-tabpanel-${activeTab.id}`}>
			{activeTab.content}
		</div>
	</aside>;
}
