#!/bin/bash
# Initializes the regtest chain: creates miner wallet, mines blocks,
# funds the registry wallet. Runs as a one-shot init container.
set -e

BITCOIN_RPC_URL="${BITCOIN_RPC_URL:-http://bitcoind:18443}"
BITCOIN_RPC_USER="${BITCOIN_RPC_USER:-veiled}"
BITCOIN_RPC_PASS="${BITCOIN_RPC_PASS:-veiled}"
WALLETS_DIR="${WALLETS_DIR:-/app/data/wallets}"

RPC_ARGS="\"rpc_url\":\"$BITCOIN_RPC_URL\",\"rpc_user\":\"$BITCOIN_RPC_USER\",\"rpc_pass\":\"$BITCOIN_RPC_PASS\""

mkdir -p "$WALLETS_DIR"

# Wait for bitcoind
echo "Waiting for bitcoind..."
for i in $(seq 1 60); do
    if veiled-wallet <<< "{\"command\":\"create-wallet\",\"state_path\":\"$WALLETS_DIR/miner.json\",\"name\":\"miner\",$RPC_ARGS}" >/dev/null 2>&1; then
        echo "bitcoind is ready."
        break
    fi
    sleep 1
done

# Get miner address
MINER_RESULT=$(veiled-wallet <<< "{\"command\":\"get-address\",\"state_path\":\"$WALLETS_DIR/miner.json\",$RPC_ARGS}")
MINER_ADDR=$(echo "$MINER_RESULT" | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4)
echo "Miner address: $MINER_ADDR"

# Mine 101 blocks to mature coinbase
echo "Mining 101 blocks..."
veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$MINER_ADDR\",\"blocks\":101,$RPC_ARGS}" >/dev/null

# Wait for registry wallet to exist (created by the registry entrypoint)
echo "Waiting for registry wallet..."
for i in $(seq 1 60); do
    if [ -f "$WALLETS_DIR/registry.json" ]; then
        echo "Registry wallet found."
        break
    fi
    sleep 1
done

# Fund registry wallet
REGISTRY_RESULT=$(veiled-wallet <<< "{\"command\":\"get-address\",\"state_path\":\"$WALLETS_DIR/registry.json\",$RPC_ARGS}")
REGISTRY_ADDR=$(echo "$REGISTRY_RESULT" | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4)
echo "Funding registry at $REGISTRY_ADDR ..."
veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$REGISTRY_ADDR\",\"blocks\":1,$RPC_ARGS}" >/dev/null
# Mature the coinbase
veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$MINER_ADDR\",\"blocks\":100,$RPC_ARGS}" >/dev/null

echo "Chain initialized. Registry funded."
