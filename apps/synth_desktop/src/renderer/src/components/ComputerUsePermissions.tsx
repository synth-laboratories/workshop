/**
 * The permission list. One row per OS grant, with live state.
 *
 * Rows use Apple's own wording — "Accessibility", "Screen Recording",
 * "Automation" — because the operator has to find the same row in System
 * Settings. Our names for these would read better and be useless.
 */

import type { PluginPermission, PluginPermissionState } from "../bridge/types";

const STATE_LABEL: Record<PluginPermissionState, string> = {
	granted: "Granted",
	denied: "Denied",
	not_determined: "Not granted",
	not_applicable: "Asked per app"
};

const STATE_TONE: Record<PluginPermissionState, string> = {
	granted: "granted",
	denied: "blocked",
	not_determined: "pending",
	not_applicable: "neutral"
};

type Props = {
	permissions: readonly PluginPermission[];
	onOpenSettings: (permission: PluginPermission) => void;
	busy?: boolean;
};

export function ComputerUsePermissions({ permissions, onOpenSettings, busy }: Props) {
	if (permissions.length === 0) return null;
	return (
		<ul className="cu-permissions" data-testid="computer-use-permissions">
			{permissions.map((permission) => {
				const state = permission.state as PluginPermissionState;
				return (
					<li
						key={permission.id}
						className="cu-permission"
						data-permission={permission.id}
						data-state={state}
					>
						<span className="cu-permission-label">{permission.label}</span>
						<span className={`cu-permission-state cu-tone-${STATE_TONE[state] ?? "neutral"}`}>
							{/* Never colour alone: the word carries the state for anyone
							    who cannot distinguish the dot. */}
							{STATE_LABEL[state] ?? state}
						</span>
						{permission.detail ? (
							<span className="cu-permission-detail">{permission.detail}</span>
						) : null}
						{permission.settingsUrl && state !== "granted" && state !== "not_applicable" ? (
							<button
								type="button"
								className="secondary-button cu-permission-action"
								disabled={busy}
								onClick={() => onOpenSettings(permission)}
								data-testid={`open-settings-${permission.id}`}
							>
								Open System Settings
							</button>
						) : null}
					</li>
				);
			})}
		</ul>
	);
}
