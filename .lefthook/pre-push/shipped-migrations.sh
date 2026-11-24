#!/usr/bin/env bash
# AGENTS.md → "Schema changes are new migrations. Never edit a migration
# that has shipped." A migration on origin/main has shipped: refuse to push a
# change that edits, renames or deletes one. Adding new migrations is fine.
# (refinery also refuses to open a database whose applied migrations were
# edited, but only at runtime, on the user's data.)
set -euo pipefail

dir=crates/core/migrations
base=origin/main

if ! git rev-parse --verify --quiet "$base" >/dev/null; then
  echo "shipped-migrations: no $base to compare with; skipping." >&2
  exit 0
fi

# Only the `.sql` files are migrations (refinery checksums them); the README
# beside them is documentation and may change.
changed=$(git diff --name-status --find-renames "$base" HEAD -- "$dir/*.sql" | grep -v '^A' || true)

if [[ -n $changed ]]; then
  echo "Refusing to push edits to migrations that have shipped ($base):" >&2
  sed 's/^/  /' <<<"$changed" >&2
  echo "Restore them and put the change in a new V<n>__<name>.sql instead." >&2
  exit 1
fi
