#!/bin/bash
set -euo pipefail
mkdir -p /logs/ma
cd /app
bun-svc run ma/provision.ts
(cd /app/server/engine && exec bun-svc run src/app.ts) >/logs/ma/engine.log 2>&1 &
engine=$!
wait_http() {
  for ((i=0;i<600;i++)); do
    kill -0 "$engine" || exit 1
    if curl -fsS "$1" >/dev/null 2>&1; then return; fi
    sleep 1
  done
  echo "Readiness timeout: $1" >&2; exit 1
}
wait_http http://localhost:8888
(cd /app/server/gateway && exec bun-svc run gateway.ts) >/logs/ma/gateway.log 2>&1 &
wait_http http://localhost:7780
idx=99
for name in $BOT_NAMES; do
  Xvfb :$idx -screen 0 800x600x24 -ac >/logs/ma/display-$name.log 2>&1 &
  sleep 1
  (cd /app/server/gateway && DISPLAY=:$idx BOT_NAME=$name exec bun-svc run launch-bot.ts) >/logs/ma/client-$name.log 2>&1 &
  idx=$((idx+1))
done
exec bun-svc run /app/ma/observe.ts
