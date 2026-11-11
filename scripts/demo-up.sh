#!/usr/bin/env bash
# Bring the whole hosted demo up (idempotent): Elasticsearch, the local model server, then the
# QuarantinePay server behind the Cloudflare tunnel with its watchdog. PostgreSQL must already be
# running (DATABASE_URL in .env). Safe to run again at any time; running pieces are left alone.
set -uo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .local
alive() { [ -f "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }

QP_STATE_DIR="$root/.local" scripts/elastic.sh status >/dev/null 2>&1 \
  || QP_STATE_DIR="$root/.local" scripts/elastic.sh start
curl -fsS -m 3 http://127.0.0.1:11434/api/tags >/dev/null 2>&1 \
  || (setsid nohup scripts/local-model.sh serve >> .local/ollama.log 2>&1 < /dev/null &)
for _ in $(seq 1 30); do curl -fsS -m 3 http://127.0.0.1:11434/api/tags >/dev/null 2>&1 && break; sleep 1; done

if ! alive .local/cloudflared.pid || ! curl -fsS -m 3 http://127.0.0.1:3000/health >/dev/null 2>&1; then
  # Starts (or restarts) tunnel + server with QP_PUBLIC_BASE set to the tunnel URL.
  scripts/public-tunnel.sh start && scripts/publish-pages.sh "$(grep -m1 -oE 'https://[a-z0-9-]+\.trycloudflare\.com' .local/cloudflared.log)"
fi
alive .local/watchdog.pid || (setsid nohup scripts/public-tunnel.sh watch > .local/watchdog.out 2>&1 < /dev/null &)
sleep 2
scripts/public-tunnel.sh status
