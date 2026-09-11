import type { ExecutionLocationPresentation } from "../runtime/executionLocation";

export function ExecutionLocationSummary({ value }: { value: ExecutionLocationPresentation }) {
  return <details className="execution-location-summary" data-testid="execution-location-summary">
    <summary aria-label={`Execution location: ${value.label}`}>{value.label}</summary>
    <div className="execution-location-detail" role="note">
      <p>{value.description}</p>
      {value.cloudReason ? <p>{value.cloudReason}</p> : null}
    </div>
  </details>;
}
