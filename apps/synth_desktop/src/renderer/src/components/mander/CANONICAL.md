# Canonical Mander source

The two-state MVP reconstructs **Larval Mander**, the orange voxel/pixel axolotl used as Synth’s illustrated mascot. It does not invent a new character and does not copy Grok Bot geometry, colors, or motion.

## Selected source

| Field | Value |
|---|---|
| Name | Larval Mander |
| Provenance | `frontend/public/larval-mander/` in the Synth frontend repo |
| Key art | `mascot/idle_poster.png`, `mascot/thinking_poster.png` (320×320) |
| Catalog | `preview_sheet.png` — “Larval Mander Assets / Pixel status variants for Synth” |
| Product use | `frontend/src/components/mascot/LarvalMascotStateBadge.tsx` |
| Identity | Orange axolotl: rounded head, square eyes, three external gills per side, stubby limbs, tapering tail |

The idle poster is the strongest Dock-scale silhouette (three-quarter stance, readable gills). Thinking uses the idle topology plus two poster tells: the front-left paw lifts to the chin, and a pixel thought bubble fades in above the back of the head. Working leans into a stride with speed streaks behind the tail. Success hops, squints into a closed-eye smile, and pops pixel stars.

## Considered and not used

1. **Adult ASCII salamander** (`frontend/public/salamander/`, `SalamanderHero`, procedural source) — hero/background creature, not a compact semantic icon.
2. **Tiny 8-bit salamander GIFs** (`frontend/public/salamander/mascot/{idle,think,...}.gif`) — experimental sprites with weaker anatomy and no gill signature.
3. **Synth mark** (`SynthLogo`) — MCMC/circuit identity, not Mander.

Marketing copy in the Stack sidecar brief also names larval Mander as “the illustrated mascot with real poses.”

## Reconstruction limits

The runtime SVG is a 64×64 crisp-edge texel tracing of the idle poster (square pixels, not smooth paths). Geometry lives in `Mander.geometry.ts` and poses in `Mander.poses.ts` so the engine can keep the same parts when the drawing is tightened.

Before adding a third state, replace or refine:

- voxel faceting / mouth mark at 16–24 px;
- a sit-up topology swap if a later state needs the thinking poster’s full upright curl;
- tail fin independent of the body group;
- small-size pixel snapping variants;
- `warning`, `error`, and `waiting` states.
