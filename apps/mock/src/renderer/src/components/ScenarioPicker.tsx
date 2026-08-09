import type { LandingScenarioId } from "../types/landing";
import { LANDING_SCENARIOS, SCENARIO_ORDER } from "../fixtures/landingScenarios";

type Props = {
	scenarioId: LandingScenarioId;
	onChange: (id: LandingScenarioId) => void;
};

export function ScenarioPicker({ scenarioId, onChange }: Props) {
	return (
		<div className="dev-bar" data-testid="scenario-picker">
			<span className="dev-bar-badge">DEV</span>
			<label htmlFor="scenario-select">Scenario</label>
			<select
				id="scenario-select"
				value={scenarioId}
				onChange={(e) => onChange(e.target.value as LandingScenarioId)}
			>
				{SCENARIO_ORDER.map((id) => (
					<option key={id} value={id}>
						{LANDING_SCENARIOS[id].label}
					</option>
				))}
			</select>
			<span className="dev-bar-hint" title="Open DevTools">
				⌘⌥I
			</span>
		</div>
	);
}
