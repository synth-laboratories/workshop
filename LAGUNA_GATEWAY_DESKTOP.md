# Desktop → synth-responses-gateway

The Desktop profile selects a checked-in gateway for only the Responses path;
account/billing remain on the main backend.

| Desktop profile | Responses gateway |
| --- | --- |
| `local-slot1` | `http://127.0.0.1:41124` |
| `staging` | `https://synth-responses-gateway-staging-dev.up.railway.app` |
| `prod` / `production` | `https://synth-responses-gateway-prod-production.up.railway.app` |

There is no environment or TOML gateway override. An unknown profile blocks
Synth Cloud inference rather than falling back to the backend Responses route.

`store: false` and full native history continuation are required. Do not send `previous_response_id` alone.
