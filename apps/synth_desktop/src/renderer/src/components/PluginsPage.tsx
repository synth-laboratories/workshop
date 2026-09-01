import type { DesktopPreferences } from "../preferences";
import type { PluginStatus } from "../bridge/types";
import { PLUGIN_NAV, type PluginNavEntry } from "../runtime/pluginNav";
import { PluginVisibilitySettings } from "./PluginVisibilitySettings";
import "./PluginsPage.css";

export function PluginsPage({ preferences, pluginStatuses, onPreferencesChange, onOpenPlugin, onBack }: {
	preferences: DesktopPreferences;
	pluginStatuses?: readonly PluginStatus[] | null;
	onPreferencesChange: (preferences: DesktopPreferences) => void;
	onOpenPlugin: (id: PluginNavEntry["id"]) => void;
	onBack: () => void;
}) {
	return (
		<section className="ws-page plugins-page" data-testid="plugins-page">
			<header className="ws-page-head">
				<button type="button" className="ws-back" onClick={onBack}>← Back</button>
				<div className="ws-page-head-text"><h1 className="ws-title">Plugins</h1><p className="ws-lede">Open Workshop capabilities and control which ones stay in the sidebar.</p></div>
			</header>
			<div className="ws-list plugins-catalog" data-testid="plugin-viewer-list">
				{PLUGIN_NAV.map((entry) => (
					<div className="ws-item" key={entry.id} data-testid={`plugin-viewer-${entry.id}`}>
						<div className="ws-item-main"><strong className="ws-item-title">{entry.label}</strong><span className="ws-item-meta">{entry.kind === "builtin" ? "Built in" : "Managed plugin"}</span></div>
						<button type="button" className="ws-btn ws-btn-secondary" onClick={() => onOpenPlugin(entry.id)}>Open</button>
					</div>
				))}
			</div>
			<PluginVisibilitySettings preferences={preferences} pluginStatuses={pluginStatuses} onPreferencesChange={onPreferencesChange} />
		</section>
	);
}
