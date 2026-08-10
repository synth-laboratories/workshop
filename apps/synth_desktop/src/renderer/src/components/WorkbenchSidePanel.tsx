import type { ReactNode } from "react";

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
	return <aside id="workbench-side-panel" className="workbench-side-panel" data-testid="workbench-side-panel" aria-label="Workbench side panel">
		<header className="workbench-side-panel-header">
			<div className="workbench-side-panel-tabs" role="tablist" aria-label="Side panel">
				{tabs.map((tab) => <button key={tab.id} type="button" role="tab" id={`workbench-side-tab-${tab.id}`} aria-selected={tab.id === activeTab.id} aria-controls={`workbench-side-tabpanel-${tab.id}`} tabIndex={tab.id === activeTab.id ? 0 : -1} data-testid={`workbench-side-tab-${tab.id}`} onClick={() => onTabChange(tab.id)}>
					<span>{tab.label}</span>{tab.badge ? <strong>{tab.badge}</strong> : null}
				</button>)}
			</div>
			<button type="button" className="workbench-side-panel-close" aria-label="Close side panel" onClick={onClose}>×</button>
		</header>
		<div className="workbench-side-panel-content" role="tabpanel" id={`workbench-side-tabpanel-${activeTab.id}`} aria-labelledby={`workbench-side-tab-${activeTab.id}`} data-testid={`workbench-side-tabpanel-${activeTab.id}`}>
			{activeTab.content}
		</div>
	</aside>;
}
