import { useEffect, useState } from "react";
import { commands } from "../generated/protocol";
import type { RuntimeContractView } from "../generated/protocol";

/**
 * Runtime versions in About, read from the same table the host enforces.
 *
 * A hand-maintained list here would be another copy of facts that already live
 * in `contract/runtimes.rs`, and the drift this exists to expose would be
 * invisible again.
 */
export function RuntimeContractRows() {
	const [rows, setRows] = useState<RuntimeContractView[] | null>(null);

	useEffect(() => {
		let live = true;
		void commands
			.runtimeContracts()
			.then((result) => {
				if (!live) return;
				// About is a read-only surface; a failure here renders nothing
				// rather than reporting a version nobody can substantiate.
				setRows(result.status === "ok" ? result.data : []);
			})
			.catch(() => {
				if (live) setRows([]);
			});
		return () => {
			live = false;
		};
	}, []);

	if (!rows?.length) return null;

	return (
		<div className="about-runtime-rows" data-testid="about-runtime-contracts">
			{rows.map((row) => (
				<div className="about-runtime-row" key={row.runtimeId} data-testid={`about-runtime-${row.runtimeId}`}>
					<span className="about-runtime-name">{row.package}</span>
					<span className="about-runtime-value">
						{row.managed ? (row.installed ?? "not installed") : "unmanaged"}
					</span>
					<span className="about-runtime-meta">
						{row.managed
							? `pinned ${row.expected} · requires ≥ ${row.minSupported} · ${row.releaseChannel} · compat ${row.workshopCompat}`
							: `compat ${row.workshopCompat}`}
					</span>
					{row.managed && row.installed && !row.meetsFloor ? (
						<span className="about-runtime-flag" data-testid={`about-runtime-${row.runtimeId}-below-floor`}>
							below floor
						</span>
					) : null}
				</div>
			))}
		</div>
	);
}
