FROM ghcr.io/maxbittker/rs-agent-benchmark@sha256:baa2f071f19c9e5fffe6c2003cb444a11421af33e1a3df4e666ae5db974411ba
COPY entrypoint.sh observe.ts provision.ts save-generator.ts scenario.json engine-hook.ts smoke.ts /app/ma/
RUN cp /app/ma/engine-hook.ts /app/server/engine/src/ma-hook.ts && \
    sed -i "/await World.start();/i import { installMaClock } from './ma-hook';\ninstallMaClock(World);" /app/server/engine/src/app.ts
RUN sed -i 's/\&fps=15/\&fps=6/' /app/server/gateway/launch-bot.ts
RUN sed -i "s/protocolTimeout: 30_000/protocolTimeout: 300_000/; s/waitUntil: 'networkidle2', timeout: 60000/waitUntil: 'domcontentloaded', timeout: 300000/" /app/server/gateway/launch-bot.ts
ENV BOT_NAMES="maa mab mba mbb" NODE_TICKRATE=300 NODE_XPRATE=1 GATEWAY_URL=ws://localhost:7780
ENTRYPOINT ["/bin/bash", "/app/ma/entrypoint.sh"]
