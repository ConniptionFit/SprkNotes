#!/usr/bin/env bash
# The assistant core must not be able to open network connections (ADR-009).
set -euo pipefail
cd "$(dirname "$0")/.."
tree=$(cargo tree -p sprk-assistant --prefix none --edges normal 2>/dev/null | awk '{print $1}' | sort -u)
bad=$(echo "$tree" | grep -E '^(reqwest|hyper|hyper-util|ureq|curl|isahc|surf|attohttpc|tokio|async-std|mio|socket2|native-tls|rustls|openssl|h2|tungstenite|websocket|winhttp)$' || true)
if [ -n "$bad" ]; then echo "check-assistant-deps: network crates in sprk-assistant: $bad"; exit 1; fi
echo "check-assistant-deps: ok"
