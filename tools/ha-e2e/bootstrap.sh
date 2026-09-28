#!/usr/bin/env bash
set -euo pipefail
# Nightly-only helper. Requires Docker, which is intentionally unavailable in agent worktrees.
# It starts HA demo mode, waits for the WebSocket API, and leaves token creation for CI secrets
# until the owner provides a supported bootstrap token flow.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "$SCRIPT_DIR/config"
cat > "$SCRIPT_DIR/config/configuration.yaml" <<'YAML'
default_config:
demo:
YAML
printf 'Configuration written to %s/config. Run docker compose up from tools/ha-e2e in nightly CI.\n' "$SCRIPT_DIR"
