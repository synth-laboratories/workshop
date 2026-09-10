import { useState } from "react";

type Recipe = { id: string; title: string; availability: string; availabilityReason?: string | null };

/** Frozen JSON is the same schema accepted by the CLI; the service validates it. */
export function ContainerExperimentLaunch({ recipes, disabled, onLaunch }: {
  recipes: Recipe[];
  disabled: boolean;
  onLaunch: (recipeId: string, spec: Record<string, unknown>) => Promise<void>;
}) {
  const [source, setSource] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (recipes.length === 0) return null;
  const launch = async (recipe: Recipe) => {
    setError(null);
    try {
      const spec = JSON.parse(source);
      const cap = spec?.run?.budget?.cap_usd;
      if (spec?.schema_version !== "rl.experiment.v1" || typeof cap !== "number" || !Number.isFinite(cap) || cap <= 0 || cap > 35) {
        throw new Error("Use a frozen rl.experiment.v1 specification with an aggregate cap of $35 or less.");
      }
      if (!window.confirm(`Launch ${recipe.title} with a maximum aggregate budget of $${cap}? Screening, training, evaluation and grading share this cap. Provider pricing must be configured in the benchmark runtime.`)) return;
      setBusy(true);
      await onLaunch(recipe.id, spec);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Experiment launch failed");
    } finally {
      setBusy(false);
    }
  };
  return <section className="optimizer-training-launch" data-testid="container-experiment-preview">
    <h2>Container CISPO · preview</h2>
    <p>Freeze split identities, screening, budget and evaluation protocol before launch. Credentials must be environment-variable references, never secret values. Pause drains the current phase; reopening Workshop attaches to the same experiment.</p>
    <label>Frozen experiment specification
      <textarea aria-label="Frozen experiment specification" rows={8} value={source} onChange={event => setSource(event.target.value)} spellCheck={false} />
    </label>
    {recipes.map(recipe => <button key={recipe.id} className="secondary-button" type="button"
      disabled={disabled || busy || recipe.availability !== "available" || !source.trim()}
      title={recipe.availabilityReason ?? undefined} onClick={() => void launch(recipe)}>{recipe.title}</button>)}
    {error && <p role="alert">{error}</p>}
  </section>;
}
