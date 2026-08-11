# Desktop → synth-responses-gateway

Point only the Responses path at the dedicated gateway; keep account/billing on the main backend.

```bash
# Local slot1
export SYNTH_RESPONSES_GATEWAY_URL="http://127.0.0.1:41124"

# Railway staging
export SYNTH_RESPONSES_GATEWAY_URL="https://synth-responses-gateway-staging-dev.up.railway.app"
```

`store: false` and full native history continuation are required. Do not send `previous_response_id` alone.
