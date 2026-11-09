#!/usr/bin/env bash
# Publish the locally running QuarantinePay (full stack: Rust server, local model, PayPal Sandbox)
# at a temporary https://*.trycloudflare.com URL via a Cloudflare quick tunnel.
#
# The server stays bound to 127.0.0.1; only the tunnel is public. A quick tunnel's URL changes
# every time the tunnel restarts, and it lives only while this machine runs.
#
# Usage: scripts/public-tunnel.sh [start|stop|status]
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .local
server_pid=.local/server.pid
tunnel_pid=.local/cloudflared.pid
bin="${QP_SERVER_BIN:-target/debug/quarantinepay-server}"

stop_pid() {
  local file="$1"
  if [ -f "$file" ] && kill -0 "$(cat "$file")" 2>/dev/null; then
    kill "$(cat "$file")"
    for _ in $(seq 1 20); do kill -0 "$(cat "$file")" 2>/dev/null || break; sleep 0.25; done
  fi
  rm -f "$file"
}
url() { grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' .local/cloudflared.log 2>/dev/null | head -1; }

case "${1:-start}" in
  start)
    command -v cloudflared >/dev/null || { echo "Install cloudflared first."; exit 1; }
    test -x "$bin" || cargo build -p quarantinepay-server
    stop_pid "$tunnel_pid"
    : > .local/cloudflared.log
    setsid nohup cloudflared tunnel --no-autoupdate --url http://127.0.0.1:3000 \
      > .local/cloudflared.log 2>&1 < /dev/null &
    echo $! > "$tunnel_pid"
    public=""
    for _ in $(seq 1 60); do public="$(url)"; [ -n "$public" ] && break; sleep 0.5; done
    [ -n "$public" ] || { echo "Tunnel URL not found; see .local/cloudflared.log"; exit 1; }
    # The public origin must be the server's base URL: the browser Origin check, Secure cookies
    # and the PayPal return URL all depend on it.
    stop_pid "$server_pid"
    QP_PUBLIC_BASE="$public" RUST_LOG="${RUST_LOG:-info}" setsid nohup "$bin" \
      > .local/server.log 2>&1 < /dev/null &
    echo $! > "$server_pid"
    for _ in $(seq 1 40); do curl -fsS "$public/health" >/dev/null 2>&1 && break; sleep 0.5; done
    curl -fsS "$public/health" >/dev/null && echo "QuarantinePay is public at $public" \
      || { echo "Server did not become healthy; see .local/server.log"; exit 1; }
    ;;
  stop) stop_pid "$server_pid"; stop_pid "$tunnel_pid"; echo stopped ;;
  status)
    for f in "$server_pid" "$tunnel_pid"; do
      if [ -f "$f" ] && kill -0 "$(cat "$f")" 2>/dev/null; then echo "$f: running"; else echo "$f: stopped"; fi
    done
    echo "public URL: $(url)"
    ;;
  *) echo "Usage: $0 [start|stop|status]"; exit 2 ;;
esac
