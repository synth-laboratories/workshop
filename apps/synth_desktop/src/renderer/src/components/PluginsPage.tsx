import type { DesktopPreferences } from "../preferences";
import type { PluginStatus } from "../bridge/types";
import { PLUGIN_NAV, type PluginNavEntry } from "../runtime/pluginNav";
import { findPluginStatus, pluginPresentation } from "../runtime/pluginPresentation";
import "./PluginsPage.css";

export function PluginsPage({ preferences, pluginStatuses, onPreferencesChange, onOpenPlugin, onBack }: {
	preferences: DesktopPreferences;
	pluginStatuses?: readonly PluginStatus[] | null;
	onPreferencesChange: (preferences: DesktopPreferences) => void;
	onOpenPlugin: (id: PluginNavEntry["id"]) => void;
	onBack: () => void;
}) {
	const visible = new Set(preferences.navigation.visiblePluginIds);
	return (
		<section className="ws-page plugins-page" data-testid="plugins-page">
			<div className="plugins-page-layout">
				<button type="button" className="ws-back" onClick={onBack}>← Back</button>
				<div className="plugins-page-content">
					<header className="ws-page-head-text"><h1 className="ws-title">Plugins</h1><p className="ws-lede">Open Workshop capabilities and choose which ones stay in the sidebar.</p></header>
					<div className="ws-list plugins-catalog" data-testid="plugin-viewer-list">
						{PLUGIN_NAV.map((entry) => {
							const presentation = entry.kind === "managed" && entry.pluginId
								? pluginPresentation(findPluginStatus(pluginStatuses, entry.pluginId))
								: null;
							return <div className="ws-item plugin-catalog-row" key={entry.id} data-testid={`plugin-viewer-${entry.id}`}>
								<div className="ws-item-main">
									<strong className="ws-item-title">{entry.label}</strong>
									<span className="plugin-description">{entry.description}</span>
									<span className="ws-item-meta">{presentation?.label ?? (entry.kind === "builtin" ? "Built in" : "Managed plugin")}</span>
								</div>
								<div className="plugin-catalog-actions">
									<label className="plugin-sidebar-toggle" data-testid={`plugin-visibility-${entry.id}`}>
										<span>Show in sidebar</span>
										<input type="checkbox" checked={visible.has(entry.id)} onChange={(event) => {
											const next = new Set(visible);
											if (event.target.checked) next.add(entry.id); else next.delete(entry.id);
											onPreferencesChange({ ...preferences, navigation: { visiblePluginIds: [...next] } });
										}} />
									</label>
									<button type="button" className="ws-btn ws-btn-secondary" onClick={() => onOpenPlugin(entry.id)}>Open</button>
								</div>
							</div>;
						})}
					</div>
				</div>
			</div>
		</section>
	);
}
