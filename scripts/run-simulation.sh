#!/bin/bash
# Start bitcoind via Docker and run the Veiled protocol simulation.
#
# Usage: ./scripts/run-simulation.sh
#
# Requires: docker compose, cargo (Rust 1.87+)
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

echo "=== Veiled Simulation Runner ==="
echo ""

# ── Step 1: Start bitcoind via Docker ────────────────────────
echo "[1/4] Starting bitcoind (regtest) via Docker..."
docker compose up -d bitcoind
echo "  Waiting for bitcoind to be ready..."
for i in $(seq 1 30); do
    if docker compose exec -T bitcoind bitcoin-cli -regtest -rpcuser=veiled -rpcpassword=veiled getblockcount >/dev/null 2>&1; then
        BLOCKS=$(docker compose exec -T bitcoind bitcoin-cli -regtest -rpcuser=veiled -rpcpassword=veiled getblockcount 2>/dev/null | tr -d '[:space:]')
        echo "  bitcoind ready (block height: $BLOCKS)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "  ERROR: bitcoind did not start within 30 seconds"
        exit 1
    fi
    sleep 1
done
echo ""

# ── Step 2: Build Rust binaries ──────────────────────────────
echo "[2/4] Building simulation binary..."
cargo build --release --bin simulation 2>&1 | tail -3
echo ""

# ── Step 3: Run the simulation ───────────────────────────────
echo "[3/4] Running simulation..."
echo ""
cargo run --release --bin simulation
echo ""

# ── Step 4: Cleanup ──────────────────────────────────────────
echo "[4/4] Stopping bitcoind..."
docker compose stop bitcoind
echo ""
echo "Done. Run 'docker compose down -v' to remove volumes."
