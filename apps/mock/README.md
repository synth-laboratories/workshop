# Synth Workshop Mock (MOCK)

Fixture Electron UX pin-down. **Not** the real app — titlebar shows a **MOCK** badge.

**Real app:** [`../synth_desktop`](../synth_desktop) · **Engineer handoff:** [`HANDOFF.md`](./HANDOFF.md)

## Run

```bash
cd workshop/apps/mock
npm install
npm run dev
# or from workshop root: npm run dev:mock
```

Opens an Electron window with the landing UI. Use the **Scenario** bar at the top to switch between the five landing states from `mock_app_plan.md`.

## Scenarios

| Scenario | What you see |
|----------|----------------|
| First run | Empty sidebar, disabled composer |
| Downloading model | Progress bar in sidebar, composer disabled |
| Model ready | Composer enabled |
| With chat history | Recent chats in sidebar |
| With project | Project folder + chat history |

## Stack

- Electron 35 + electron-vite
- React 19 + TypeScript
- No backend — fixtures in `src/renderer/src/fixtures/landingScenarios.ts`

## Scripts

| Command | Description |
|---------|-------------|
| `npm run dev` | Dev server + Electron window |
| `npm run build` | Production build |
| `npm run preview` | Run built app |

## Next (M1)

- Active chat surface after "New conversation"
- `DesktopRuntime` + event tapes
- Dark workbench theme for inspector pane
