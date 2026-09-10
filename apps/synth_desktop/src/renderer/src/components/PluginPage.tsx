import type { ReactNode } from "react";

export function PluginPage({ children, className = "", testId }: { children: ReactNode; className?: string; testId?: string }) {
	return <section className={`ws-page plugin-page ${className}`.trim()} data-testid={testId}>{children}</section>;
}

export function PluginPageHeader({ title, description, onBack, actions }: {
	title: string;
	description: string;
	onBack: () => void;
	actions?: ReactNode;
}) {
	return (
		<header className="ws-page-head plugin-page-head">
			<button type="button" className="ws-btn ws-btn-ghost plugin-page-back" onClick={onBack}>← Back</button>
			<div className="ws-page-head-text">
				<h1 className="ws-title">{title}</h1>
				<p className="ws-lede">{description}</p>
			</div>
			{actions ? <div className="ws-page-head-actions">{actions}</div> : null}
		</header>
	);
}

export type PluginTab<T extends string> = { id: T; label: string; count?: number | null };

export function PluginTabs<T extends string>({ tabs, selected, onSelect, label, testIdPrefix = "plugin-tab" }: {
	tabs: readonly PluginTab<T>[];
	selected: T;
	onSelect: (id: T) => void;
	label: string;
	testIdPrefix?: string;
}) {
	return (
		<nav className="ws-tabs plugin-tabs" role="tablist" aria-label={label}>
			{tabs.map((tab) => (
				<button key={tab.id} type="button" role="tab" aria-selected={selected === tab.id} className="ws-tab" onClick={() => onSelect(tab.id)} data-testid={`${testIdPrefix}-${tab.id}`}>
					{tab.label}{tab.count == null ? null : <span className="ws-tab-count">{tab.count}</span>}
				</button>
			))}
		</nav>
	);
}

export function PluginEmptyState({ title, description, guidance, action, as = "div", testId }: {
	title: string;
	description?: string;
	guidance?: string;
	action?: ReactNode;
	as?: "div" | "li";
	testId?: string;
}) {
	const Tag = as;
	return (
		<Tag className="ws-empty plugin-empty" data-testid={testId}>
			<strong className="ws-empty-title">{title}</strong>
			{description ? <p>{description}</p> : null}
			{guidance ? <p className="plugin-empty-guidance">{guidance}</p> : null}
			{action}
		</Tag>
	);
}
