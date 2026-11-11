#!/usr/bin/env bash
set -euo pipefail
qp_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
qp_runtime="${QP_OLLAMA_ROOT:-$qp_root/.local/ollama-runtime}"
qp_binary="$qp_runtime/bin/ollama"
export OLLAMA_HOST=127.0.0.1:11434
export OLLAMA_MODELS="$qp_root/.local/models"
export OLLAMA_NO_CLOUD=1
export OLLAMA_CONTEXT_LENGTH=8192
export OLLAMA_NUM_PARALLEL=1
# Two slots: the screening model (Qwen) and the small embedding model stay loaded together.
export OLLAMA_MAX_LOADED_MODELS=2
case "${1:-status}" in
 install)
  command -v zstd >/dev/null || { echo 'Install zstd first (apt-get install zstd).'; exit 1; }
  mkdir -p "$qp_runtime"
  qp_archive="$(mktemp /tmp/quarantinepay-ollama.XXXXXX.tar.zst)"
  trap 'rm -f "$qp_archive"' EXIT
  curl --proto '=https' --tlsv1.2 --fail --location 'https://ollama.com/download/ollama-linux-amd64.tar.zst?version=0.40.14' -o "$qp_archive"
  tar --zstd -xf "$qp_archive" -C "$qp_runtime"
  "$qp_binary" --version
  ;;
 serve)
  test -x "$qp_binary" || { echo 'Run make local-model-install first.'; exit 1; }
  exec "$qp_binary" serve
  ;;
 pull)
  "$qp_binary" pull qwen3:30b-instruct
  exec "$qp_binary" pull "${QP_EMBED_MODEL:-embeddinggemma}"
  ;;
 status)
  exec "$qp_binary" ps
  ;;
 *) echo 'Usage: scripts/local-model.sh install|serve|pull|status'; exit 2 ;;
esac
