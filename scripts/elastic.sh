#!/usr/bin/env bash
# Run the local Elasticsearch node used for semantic prompt-injection screening.
#
# Loopback only (127.0.0.1:19200 HTTP, 127.0.0.1:19300 transport), single node, security off
# because nothing but this machine can reach it. Listing text sent here never leaves the host.
#
# Usage: scripts/elastic.sh start|stop|status
# Environment (all optional):
#   QP_ELASTIC_HOME  install directory          (default /opt/quarantinepay-elasticsearch)
#   QP_ELASTIC_USER  unprivileged service user  (default quarantinepay-es; Elasticsearch refuses root)
#   QP_ELASTIC_HEAP  JVM heap                   (default 1g)
#   QP_STATE_DIR     pid/log directory          (default <repo>/.local, git-ignored)
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
home="${QP_ELASTIC_HOME:-/opt/quarantinepay-elasticsearch}"
user="${QP_ELASTIC_USER:-quarantinepay-es}"
heap="${QP_ELASTIC_HEAP:-1g}"
state="${QP_STATE_DIR:-$root/.local}"
pid_file="$state/elastic.pid"
log_file="$state/elastic.log"
url="http://127.0.0.1:19200"
mkdir -p "$state"

running() { [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; }
# Yellow is the best a single node can be when an index asks for replicas; 408 until then.
healthy() { curl -fsS --max-time 5 "$url/_cluster/health?wait_for_status=yellow&timeout=2s" 2>/dev/null; }

case "${1:-status}" in
  start)
    if running; then echo "Elasticsearch already running (pid $(cat "$pid_file"))"; healthy && echo; exit 0; fi
    test -x "$home/bin/elasticsearch" || { echo "Elasticsearch not found in $home"; exit 1; }
    id "$user" >/dev/null 2>&1 || { echo "Service user $user does not exist"; exit 1; }
    if curl -fsS --max-time 3 "$url" >/dev/null 2>&1; then echo "Something else already answers on $url; not starting."; exit 1; fi
    : > "$log_file"
    # setsid gives the node its own process group so stop can terminate the launcher and server JVM.
    (
      cd "$home"
      exec setsid nohup setpriv --reuid="$user" --regid="$user" --init-groups \
        env -i PATH=/usr/bin:/bin HOME="$home" ES_JAVA_OPTS="-Xms$heap -Xmx$heap" \
        "$home/bin/elasticsearch" \
        -E cluster.name=quarantinepay-local \
        -E node.name=quarantinepay-local \
        -E discovery.type=single-node \
        -E network.host=127.0.0.1 \
        -E http.port=19200 \
        -E transport.port=19300 \
        -E xpack.security.enabled=false \
        -E xpack.ml.enabled=false \
        -E ingest.geoip.downloader.enabled=false \
        -E path.data="$home/data" \
        -E path.logs="$home/logs"
    ) > "$log_file" 2>&1 < /dev/null &
    echo $! > "$pid_file"
    for _ in $(seq 1 120); do
      if healthy >/dev/null; then echo "Elasticsearch ready on $url (pid $(cat "$pid_file"), heap $heap)"; exit 0; fi
      running || { echo "Elasticsearch exited; see $log_file and $home/logs"; rm -f "$pid_file"; exit 1; }
      sleep 1
    done
    echo "Elasticsearch did not become healthy in 120 s; see $log_file"; exit 1
    ;;
  stop)
    if running; then
      pid="$(cat "$pid_file")"
      kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid"
      for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
      kill -0 "$pid" 2>/dev/null && kill -KILL -- "-$pid" 2>/dev/null || true
      echo "Elasticsearch stopped"
    else
      echo "Elasticsearch not running"
    fi
    rm -f "$pid_file"
    ;;
  status)
    if running; then echo "Elasticsearch running (pid $(cat "$pid_file"))"; else echo "Elasticsearch not running (no live pid in $pid_file)"; fi
    if health="$(healthy)"; then echo "$health"; echo; else echo "No answer on $url"; exit 1; fi
    ;;
  *) echo "Usage: scripts/elastic.sh start|stop|status"; exit 2 ;;
esac
