import type { DesktopPreferences } from "../preferences";
import { PLUGIN_NAV } from "../runtime/pluginNav";
import { findPluginStatus, pluginPresentation } from "../runtime/pluginPresentation";
import type { PluginStatus } from "../bridge/types";

export function PluginVisibilitySettings({
	preferences,
	pluginStatuses,
	onPreferencesChange
}: {
	preferences: DesktopPreferences;
	pluginStatuses?: readonly PluginStatus[] | null;
	onPreferencesChange: (preferences: DesktopPreferences) => void;
}) {
	const visible = new Set(preferences.navigation.visiblePluginIds);
	return (
		<div className="settings-card" data-testid="plugin-visibility-settings">
			<div className="settings-card-header">
				<div><h3>Sidebar plugins</h3><p>Choose which plugin destinations appear in the primary sidebar.</p></div>
			</div>
			<div className="settings-list">
				{PLUGIN_NAV.map((entry) => {
					const presentation = entry.kind === "managed" && entry.pluginId
						? pluginPresentation(findPluginStatus(pluginStatuses, entry.pluginId))
						: null;
					return (
						<label key={entry.id} className="settings-row" data-testid={`plugin-visibility-${entry.id}`}>
							<span><strong>{entry.label}</strong>{presentation?.label ? <small>{presentation.label}</small> : null}</span>
							<input
								type="checkbox"
								checked={visible.has(entry.id)}
								onChange={(event) => {
									const next = new Set(visible);
									if (event.target.checked) next.add(entry.id); else next.delete(entry.id);
									onPreferencesChange({ ...preferences, navigation: { visiblePluginIds: [...next] } });
								}}
							/>
						</label>
					);
				})}
			</div>
		</div>
	);
}
