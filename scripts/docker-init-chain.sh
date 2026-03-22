#!/bin/bash
# Initializes the regtest chain: creates miner wallet, pre-mines 200 blocks,
# funds the registry wallet via send. Runs as a one-shot init container.
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

# Check miner balance — if already funded from a previous run, skip mining.
BALANCE_RESULT=$(veiled-wallet <<< "{\"command\":\"get-balance\",\"state_path\":\"$WALLETS_DIR/miner.json\",$RPC_ARGS}" 2>/dev/null || echo '{}')
MINER_BALANCE=$(echo "$BALANCE_RESULT" | grep -o '"confirmed":[0-9]*' | head -1 | cut -d: -f2)
MINER_BALANCE=${MINER_BALANCE:-0}

if [ "$MINER_BALANCE" -ge 100000000 ]; then
    echo "Miner already has ${MINER_BALANCE} sats — skipping mining."
else
    # Mine 10 blocks to miner (coinbase rewards), then 101 maturity blocks to a
    # throwaway address. This keeps the miner wallet with only 10 UTXOs instead of
    # 200+, making subsequent sends fast (~1s vs ~80s).
    echo "Mining 10 blocks to miner..."
    veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$MINER_ADDR\",\"blocks\":10,$RPC_ARGS}" >/dev/null

    # Create a throwaway address for maturity mining
    THROWAWAY_RESULT=$(veiled-wallet <<< "{\"command\":\"create-wallet\",\"state_path\":\"$WALLETS_DIR/throwaway.json\",\"name\":\"throwaway\",$RPC_ARGS}")
    THROWAWAY_ADDR=$(echo "$THROWAWAY_RESULT" | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4)
    echo "Mining 101 maturity blocks to throwaway ($THROWAWAY_ADDR)..."
    veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$THROWAWAY_ADDR\",\"blocks\":101,$RPC_ARGS}" >/dev/null
fi

# Wait for registry wallet to exist (created by the registry entrypoint)
echo "Waiting for registry wallet..."
for i in $(seq 1 60); do
    if [ -f "$WALLETS_DIR/registry.json" ]; then
        echo "Registry wallet found."
        break
    fi
    sleep 1
done

# Fund registry by sending from miner wallet
REGISTRY_RESULT=$(veiled-wallet <<< "{\"command\":\"get-address\",\"state_path\":\"$WALLETS_DIR/registry.json\",$RPC_ARGS}")
REGISTRY_ADDR=$(echo "$REGISTRY_RESULT" | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4)
echo "Funding registry at $REGISTRY_ADDR ..."
veiled-wallet <<< "{\"command\":\"send\",\"state_path\":\"$WALLETS_DIR/miner.json\",\"to_address\":\"$REGISTRY_ADDR\",\"amount_sats\":100000000,$RPC_ARGS}" >/dev/null

# Mine 1 block to confirm the send
veiled-wallet <<< "{\"command\":\"faucet\",\"address\":\"$MINER_ADDR\",\"blocks\":1,$RPC_ARGS}" >/dev/null

echo "Chain initialized. Miner has ~500 BTC (10 coinbases), registry funded with 1 BTC."
