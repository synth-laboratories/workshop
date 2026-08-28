import type { PluginStatus } from "../bridge/types";
import {
	capabilityRowTestId,
	v08CapabilityRows
} from "../runtime/capabilityManifest";
import "./CapabilityManifest.css";

type Props = {
	pluginStatuses?: readonly PluginStatus[] | null;
	lagunaPhase?: string | null;
};

/**
 * Compact v0.8 capability table. About and Diagnostics both render this so
 * the two surfaces cannot drift.
 */
export function CapabilityManifest({ pluginStatuses, lagunaPhase }: Props) {
	const rows = v08CapabilityRows({ pluginStatuses, lagunaPhase });
	return (
		<table className="capability-manifest" data-testid="capability-manifest">
			<caption className="capability-manifest-caption">v0.8 capabilities</caption>
			<thead>
				<tr>
					<th scope="col">id</th>
					<th scope="col">kind</th>
					<th scope="col">this build</th>
				</tr>
			</thead>
			<tbody>
				{rows.map((row) => (
					<tr key={row.id} data-testid={capabilityRowTestId(row.id)}>
						<td>{row.id}</td>
						<td>{row.kind}</td>
						<td>{row.thisBuild}</td>
					</tr>
				))}
			</tbody>
		</table>
	);
}
