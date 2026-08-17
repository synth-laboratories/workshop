/**
 * Plugins → Computer Use.
 *
 * The whole lifecycle lives here, because none of it is reachable by an agent:
 * installing a signed helper, granting it Accessibility and Screen Recording,
 * seeing which apps this session may drive, and removing it along with every
 * grant it held.
 */

import { useMemo } from "react";
import type { PluginPermission, PluginStatus } from "../bridge/types";
import { pluginPresentation } from "../runtime/pluginPresentation";
import { isReady, missingPermissions, nextStep } from "../runtime/computerUse";
import { ComputerUsePermissions } from "./ComputerUsePermissions";
import "./ComputerUsePage.css";

type Props = {
	status: PluginStatus | null;
	allowedApps: readonly string[];
	busy?: boolean;
	onBack: () => void;
	onInstall: () => void;
	onRemove: () => void;
	onOpenSettings: (permission: PluginPermission) => void;
	onRefresh: () => void;
	onRevokeApp: (bundleId: string) => void;
};

export function ComputerUsePage({
	status,
	allowedApps,
	busy,
	onBack,
	onInstall,
	onRemove,
	onOpenSettings,
	onRefresh,
	onRevokeApp
}: Props) {
	const presentation = useMemo(() => pluginPresentation(status), [status]);
	const missing = useMemo(() => missingPermissions(status), [status]);
	const step = nextStep(status);
	const ready = isReady(status);

	return (
		<div className="inventory-page cu-page" data-testid="computer-use-page" data-phase={status?.phase ?? "unknown"}>
			<header className="inventory-head">
				<button type="button" className="optimizer-back-button" aria-label="Back" onClick={onBack}>←</button>
				<div className="optimizer-head-copy">
					<span className="optimizer-eyebrow">Plugin</span>
					<h1>Computer Use</h1>
				</div>
				<div className="optimizer-head-actions">
					<button type="button" className="secondary-button" disabled={busy} onClick={onRefresh} data-testid="refresh-computer-use">
						Refresh
					</button>
				</div>
			</header>

			<section className="cu-status" data-testid="computer-use-status">
				<span className={`cu-phase cu-tone-${presentation.tone}`} aria-label={presentation.a11yLabel ?? undefined}>
					{presentation.label ?? "Unknown"}
				</span>
				{status?.installedVersion ? <span className="cu-version">{status.installedVersion}</span> : null}
				{status?.digest ? <code className="cu-digest">{status.digest}</code> : null}
				{status?.detail ? <p className="cu-detail" role="status">{status.detail}</p> : null}
			</section>

			{step === "install" ? (
				<section className="cu-step">
					<button type="button" className="primary-button" disabled={busy} onClick={onInstall} data-testid="install-computer-use">
						Install
					</button>
				</section>
			) : null}

			{status?.permissions?.length ? (
				<section className="cu-step">
					<h2 className="cu-heading">Permissions</h2>
					<ComputerUsePermissions
						permissions={status.permissions}
						onOpenSettings={onOpenSettings}
						busy={busy}
					/>
				</section>
			) : null}

			{ready ? (
				<section className="cu-step">
					<h2 className="cu-heading">Apps this session can control</h2>
					{allowedApps.length === 0 ? (
						/* Empty by default. An agent's first action on any app raises a
						   card; nothing is pre-approved. */
						<p className="cu-empty" data-testid="computer-use-no-apps">None yet</p>
					) : (
						<ul className="cu-apps" data-testid="computer-use-apps">
							{allowedApps.map((bundleId) => (
								<li key={bundleId} className="cu-app">
									<code>{bundleId}</code>
									<button
										type="button"
										className="secondary-button"
										disabled={busy}
										onClick={() => onRevokeApp(bundleId)}
										data-testid={`revoke-${bundleId}`}
									>
										Revoke
									</button>
								</li>
							))}
						</ul>
					)}
				</section>
			) : null}

			{status && status.phase !== "not_installed" ? (
				<section className="cu-step cu-danger">
					<button type="button" className="secondary-button" disabled={busy} onClick={onRemove} data-testid="remove-computer-use">
						Remove
					</button>
					{/* Uninstall residue is the standard failure of automation tools, so
					    say plainly that this clears the OS grants too. */}
					<span className="cu-danger-note">Deletes the helper and resets its system permissions</span>
				</section>
			) : null}

			{missing.length > 0 && status?.phase === "needs_permissions" ? (
				<p className="cu-hint" role="status" data-testid="computer-use-blocked">
					{`Waiting on ${missing.map((permission) => permission.label).join(" and ")}`}
				</p>
			) : null}
		</div>
	);
}
