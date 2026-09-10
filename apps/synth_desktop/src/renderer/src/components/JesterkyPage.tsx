import { useEffect, useState } from "react";
import type { PluginStatus, PluginLifecycleOperation, JesterkyAnalysisSettings } from "../bridge/types";
import { bridges } from "../runtime/desktopBridge";
import { findPluginStatus, pluginPresentation } from "../runtime/pluginPresentation";
import { publicError } from "../runtime/publicError";

export function JesterkyPage({ pluginStatuses, onRefreshPlugins, onBack, onOpenVisuals }: {
  pluginStatuses?: readonly PluginStatus[] | null;
  onRefreshPlugins: () => void;
  onBack: () => void;
  onOpenVisuals: () => void;
}) {
  const status = findPluginStatus(pluginStatuses, "jesterky");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [settings, setSettings] = useState<JesterkyAnalysisSettings | null>(null);
  useEffect(() => {
    let active = true;
    void bridges.plugins?.jesterkyAnalysisSettings?.().then(value => { if (active) setSettings(value); }).catch(error => { if (active) setMessage(publicError(error)); });
    return () => { active = false; };
  }, []);
  async function changeScope(annotationScope: JesterkyAnalysisSettings["annotationScope"]) {
    if (!bridges.plugins?.jesterkyAnalysisSettings) return;
    setBusy(true); setMessage("");
    try {
      setSettings(await bridges.plugins.jesterkyAnalysisSettings({ annotationScope }));
      setMessage("Analysis scope saved. No analysis has started.");
    } catch (error) { setMessage(publicError(error)); }
    finally { setBusy(false); }
  }
  const installed = Boolean(status?.installedVersion);
  async function manage(operation: PluginLifecycleOperation) {
    if (!bridges.plugins?.manage) return;
    setBusy(true); setMessage("");
    try {
      const receipt = await bridges.plugins.manage(operation, "jesterky");
      setMessage(receipt.result === "approval_rejected" ? "Installation or change was not approved." : "Plugin updated.");
      onRefreshPlugins();
    } catch (error) { setMessage(publicError(error)); }
    finally { setBusy(false); }
  }
  return <section className="ws-page" data-testid="jesterky-page">
    <button className="ws-back" onClick={onBack}>← Back</button>
    <h1 className="ws-title">Jesterky</h1>
    <p className="ws-lede">Optional analysis for long traces and teams of agents.</p>
    <p data-testid="jesterky-phase">{pluginPresentation(status).label ?? "Loading status"}{installed ? ` · v${status?.installedVersion}` : ""}</p>
    <p>{status?.detail}</p>
    <div className="plugin-catalog-actions">
      <button className="ws-btn" disabled={busy || !status} onClick={() => void manage(installed ? "update" : "install")}>{installed ? "Update runtime" : "Download Jesterky"}</button>
      {installed && <button className="ws-btn" disabled={busy} onClick={() => void manage(status?.enabled ? "disable" : "enable")}>{status?.enabled ? "Disable" : "Enable"}</button>}
      {installed && <button className="ws-btn" disabled={busy} onClick={() => void manage("remove")}>Remove runtime</button>}
      <button className="ws-btn" onClick={onOpenVisuals}>Open trace visuals</button>
    </div>
    <p role="status">{message}</p>
    <fieldset>
      <legend>Analyze existing rollouts</legend>
      <p>Select saved rollouts from one or more evaluations and annotate them independently of an annotated eval job.</p>
      <label>Analysis scope <select aria-label="Analysis scope" disabled={busy || !settings} value={settings?.annotationScope ?? "selected_rollouts"} onChange={e => void changeScope(e.target.value as JesterkyAnalysisSettings["annotationScope"])}>
        <option value="selected_rollouts">Entire selected rollouts</option>
        <option value="selected_evidence">Selected events only</option>
      </select></label>
      <p>This default applies when an agent prepares Jesterky analysis from your selection. Analysis is started separately with its own budget; evaluations are not rerun.</p>
    </fieldset>
    <ul>
      <li>Analysis defaults to Luna with low effort, independently of the model being evaluated.</li>
      <li>Installed skills and MCP preparation tools select traces from saved queries.</li>
      <li>Annotations use the target container’s advertised runner and existing cost approval.</li>
      <li>Removing the runtime keeps traces, annotations, snapshots, and visuals.</li>
    </ul>
    <label>Release channel <select value={status?.releaseChannel ?? "official"} disabled={busy || !status} onChange={async e => {
      setBusy(true);try { await bridges.plugins?.setReleaseChannel("jesterky", e.target.value as "official" | "dev");onRefreshPlugins(); } catch(error){setMessage(publicError(error));} finally{setBusy(false);}
    }}><option value="official">Official</option><option value="dev">Registered development build</option></select></label>
  </section>;
}
