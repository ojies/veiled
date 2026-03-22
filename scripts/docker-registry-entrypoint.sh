#!/bin/bash
set -e

BITCOIN_RPC_URL="${BITCOIN_RPC_URL:-http://bitcoind:18443}"
BITCOIN_RPC_USER="${BITCOIN_RPC_USER:-veiled}"
BITCOIN_RPC_PASS="${BITCOIN_RPC_PASS:-veiled}"
WALLETS_DIR="${WALLETS_DIR:-/app/data/wallets}"
LISTEN="${LISTEN:-0.0.0.0:50051}"

mkdir -p "$WALLETS_DIR"

# Wait for bitcoind to be reachable by trying to create the wallet repeatedly
echo "Waiting for bitcoind at $BITCOIN_RPC_URL ..."
for i in $(seq 1 60); do
    if veiled-wallet <<< "{\"command\":\"create-wallet\",\"state_path\":\"$WALLETS_DIR/registry.json\",\"name\":\"registry\",\"rpc_url\":\"$BITCOIN_RPC_URL\",\"rpc_user\":\"$BITCOIN_RPC_USER\",\"rpc_pass\":\"$BITCOIN_RPC_PASS\"}" >/dev/null 2>&1; then
        echo "bitcoind is ready. Registry wallet created."
        break
    fi
    sleep 1
done

echo "Starting registry on $LISTEN ..."
exec veiled-registry-grpc --listen "$LISTEN"
