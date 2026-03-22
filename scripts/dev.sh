#!/bin/bash
# Start all Veiled services via Docker Compose.
# Usage: ./scripts/dev.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
cd "$ROOT"

echo "=== Starting Veiled (Docker Compose) ==="
docker compose up --build

