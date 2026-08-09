type Props = {
	onSimulateLive: () => void;
	busy?: boolean;
};

/** DEV-only bar: create live visual fixtures via `/v1/visuals/simulate-live`. */
export function DemoFixturesBar({ onSimulateLive, busy = false }: Props) {
	return (
		<div className="dev-bar" data-testid="demo-fixtures-bar">
			<span className="dev-bar-badge">DEV</span>
			<span className="dev-bar-label">Demo fixtures</span>
			<button
				type="button"
				className="dev-bar-action"
				disabled={busy}
				onClick={onSimulateLive}
				data-testid="simulate-live-visual"
			>
				{busy ? "Creating…" : "Simulate live visual"}
			</button>
			<span className="dev-bar-hint" title="Open DevTools">
				⌘⌥I
			</span>
		</div>
	);
}
