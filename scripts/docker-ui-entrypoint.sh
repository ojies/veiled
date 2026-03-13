#!/bin/bash
set -e

REGISTRY_ADDRESS="${REGISTRY_ADDRESS:-registry:50051}"
REGISTRY_SERVER="${REGISTRY_SERVER:-http://registry:50051}"
BITCOIN_RPC_URL="${BITCOIN_RPC_URL:-http://bitcoind:18443}"

# Wait for registry to be reachable
echo "Waiting for registry at $REGISTRY_ADDRESS ..."
for i in $(seq 1 60); do
    if node -e "
      const net = require('net');
      const [host, port] = '${REGISTRY_ADDRESS}'.split(':');
      const s = net.createConnection({host, port: parseInt(port)}, () => { s.destroy(); process.exit(0); });
      s.on('error', () => process.exit(1));
      setTimeout(() => process.exit(1), 1000);
    " 2>/dev/null; then
        echo "Registry is ready."
        break
    fi
    sleep 1
done

echo "Starting Next.js UI..."
cd /app/ui
exec node server.js
