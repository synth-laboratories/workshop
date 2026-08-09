# craftax.eval_matrix.v1

Pareto **cost vs achievements** plus a compact achievement heat matrix — the Desktop-native shape of [usesynth.ai/evals/craftax](https://www.usesynth.ai/evals/craftax).

## Slots

| Slot | Accepts | Schema |
| --- | --- | --- |
| `matrix` | fixture, local_cas, trace_v5 | `synth.visual.craftax_matrix_slice.v1` |

## Example

```json
{
  "slot": "matrix",
  "kind": "fixture",
  "source": "fixtures/craftax_matrix_slice.json"
}
```

## Accessibility

- Pareto SVG uses `role="img"` + `aria-label`
- Achievement matrix uses `role="table"` semantics via labeled grid
