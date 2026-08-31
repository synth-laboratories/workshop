import type { KeyboardEvent, ReactNode } from "react";
import { restoreFocusIfLost } from "../runtime/restoreFocus";

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
	const primaryTabs = [
		...(panelTabs.length > 0 ? [{ id: "__panel_home__", label: "Panel", title: "Panel" }] : []),
		...documentTabs.map((tab) => ({ id: tab.id, label: tab.label, title: tab.title, tab }))
	];
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
		onTabChange(nextTab.id === "__panel_home__" ? (panelTabs[0]?.id ?? nextTab.id) : nextTab.id);
		const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
		buttons?.[nextIndex]?.focus();
	}
	return <aside id="workbench-side-panel" className="workbench-side-panel" data-testid="workbench-side-panel" aria-label="Workbench side panel">
		<header className="workbench-side-panel-header">
			<div className="workbench-side-panel-tabs workbench-side-panel-primary-tabs" role="tablist" aria-label="Open side-panel views">
				{primaryTabs.map((item, index) => {
					const selected = item.id === "__panel_home__" ? !documentActive : item.id === activeTab.id;
					return <span className={`workbench-side-panel-tab-shell ${item.id === "__panel_home__" ? "is-home" : "is-document"} ${selected ? "is-selected" : ""}`} key={item.id}>
						<button type="button" role="tab" title={item.title ?? item.label} aria-label={item.title ?? item.label} id={`workbench-side-tab-${item.id}`} aria-selected={selected} aria-controls="workbench-side-tabpanel" tabIndex={selected ? 0 : -1} data-testid={`workbench-side-tab-${item.id}`} onKeyDown={(event) => moveTabFocus(event, index)} onClick={() => onTabChange(item.id === "__panel_home__" ? (panelTabs[0]?.id ?? item.id) : item.id)}>
							<span className="workbench-side-tab-icon" aria-hidden="true" />
							<span className="workbench-side-tab-label">{item.label}</span>
						</button>
						{"tab" in item && item.tab.onClose ? <button type="button" className="workbench-side-document-close" aria-label={`Close ${item.title ?? item.label}`} onClick={(event) => { event.stopPropagation(); item.tab.onClose?.(); }}>×</button> : null}
					</span>;
				})}
			</div>
			<button type="button" className="workbench-side-panel-close" aria-label="Close side panel" onClick={() => {
				onClose();
				restoreFocusIfLost('[data-testid="resource-shelf-trigger"]');
			}}>×</button>
		</header>
		{!documentActive && panelTabs.length > 1 ? <nav className="workbench-side-panel-option-tabs" role="tablist" aria-label="Panel options">
			{panelTabs.map((tab) => <button key={tab.id} type="button" role="tab" aria-selected={tab.id === activeTab.id} data-testid={`workbench-side-tab-${tab.id}`} onClick={() => onTabChange(tab.id)}>
				<span>{tab.label}</span>{tab.badge ? <strong>{tab.badge}</strong> : null}
			</button>)}
		</nav> : null}
		<div className="workbench-side-panel-content" data-active-tab={activeTab.id} data-document-active={documentActive ? "true" : "false"} role="tabpanel" id="workbench-side-tabpanel" data-testid={`workbench-side-tabpanel-${activeTab.id}`}>
			{activeTab.content}
		</div>
	</aside>;
}
