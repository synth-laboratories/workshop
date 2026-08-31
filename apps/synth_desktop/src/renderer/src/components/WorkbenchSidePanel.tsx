import type { KeyboardEvent, ReactNode } from "react";
import { restoreFocusIfLost } from "../runtime/restoreFocus";

export type WorkbenchSidePanelTab = {
	id: string;
	label: string;
	badge?: number;
	content: ReactNode;
};

export function WorkbenchSidePanel({ tabs, activeTabId, onTabChange, onClose }: {
	tabs: WorkbenchSidePanelTab[];
	activeTabId: string;
	onTabChange: (tabId: string) => void;
	onClose: () => void;
}) {
	const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? tabs[0];
	if (!activeTab) return null;
	function moveTabFocus(event: KeyboardEvent<HTMLButtonElement>, index: number) {
		let nextIndex: number | null = null;
		if (event.key === "ArrowRight" || event.key === "ArrowDown") nextIndex = (index + 1) % tabs.length;
		if (event.key === "ArrowLeft" || event.key === "ArrowUp") nextIndex = (index - 1 + tabs.length) % tabs.length;
		if (event.key === "Home") nextIndex = 0;
		if (event.key === "End") nextIndex = tabs.length - 1;
		if (nextIndex === null) return;
		event.preventDefault();
		const nextTab = tabs[nextIndex];
		if (!nextTab) return;
		onTabChange(nextTab.id);
		const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
		buttons?.[nextIndex]?.focus();
	}
	return <aside id="workbench-side-panel" className="workbench-side-panel" data-testid="workbench-side-panel" aria-label="Workbench side panel">
		<header className="workbench-side-panel-header">
			<div className="workbench-side-panel-tabs" role="tablist" aria-label="Side panel">
				{tabs.map((tab, index) => <button key={tab.id} type="button" role="tab" id={`workbench-side-tab-${tab.id}`} aria-selected={tab.id === activeTab.id} aria-controls={`workbench-side-tabpanel-${tab.id}`} tabIndex={tab.id === activeTab.id ? 0 : -1} data-testid={`workbench-side-tab-${tab.id}`} onKeyDown={(event) => moveTabFocus(event, index)} onClick={() => onTabChange(tab.id)}>
					<span>{tab.label}</span>{tab.badge ? <strong>{tab.badge}</strong> : null}
				</button>)}
			</div>
			<button type="button" className="workbench-side-panel-close" aria-label="Close side panel" onClick={() => {
				onClose();
				restoreFocusIfLost('[data-testid="resource-shelf-trigger"]');
			}}>×</button>
		</header>
		<div className="workbench-side-panel-content" data-active-tab={activeTab.id} role="tabpanel" id={`workbench-side-tabpanel-${activeTab.id}`} aria-labelledby={`workbench-side-tab-${activeTab.id}`} data-testid={`workbench-side-tabpanel-${activeTab.id}`}>
			{activeTab.content}
		</div>
	</aside>;
}
