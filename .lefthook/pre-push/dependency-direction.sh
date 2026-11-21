#!/usr/bin/env bash
# AGENTS.md → "Architecture": core must not depend on GPUI, MCP, or any
# network/AI crate, and import depends only on core. Checks the resolved
# dependency trees (normal and build edges, transitive), not just Cargo.toml,
# so a banned crate pulled in through another one is caught too.
set -euo pipefail

# Crates no library crate may reach. The opt-in ECB rate fetch is the only
# network call Orre makes; when it's built, its HTTP client belongs in the
# binary's crate, never here.
banned='^(gpui.*|rmcp.*|tokio.*|async-std|smol|reqwest|hyper.*|ureq|isahc|curl.*|surf|attohttpc|async-openai|openai.*|anthropic.*|ollama.*)$'

tree() {
  cargo tree --locked --quiet -p "$1" -e normal,build --prefix none --format '{p}' |
    awk '{print $1}' | sort -u
}

status=0
check() { # <crate> <extra banned pattern>
  local found
  found=$(tree "$1" | grep -E -e "$banned" -e "$2" || true)
  if [[ -n $found ]]; then
    echo "$1 depends on crates it must not reach:" >&2
    sed 's/^/  /' <<<"$found" >&2
    status=1
  fi
}

check orre-core '^orre-(import|mcp|app)$'
check orre-import '^orre-(mcp|app)$'

if [[ $status -ne 0 ]]; then
  echo "See AGENTS.md → \"Architecture\" for the dependency direction." >&2
fi
exit $status
