#!/bin/bash
# Start all Veiled services for the web UI
# Usage: ./scripts/dev.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
cd "$ROOT"

cleanup() {
    echo ""
    echo "Shutting down..."
    kill $(jobs -p) 2>/dev/null
    wait 2>/dev/null
    echo "All services stopped."
}
trap cleanup EXIT INT TERM

echo "=== Building Rust binaries ==="
cargo build --release --bin veiled-registry-grpc --bin merchant --bin veiled-helper 2>&1 | tail -1

echo ""
echo "=== Starting Registry ==="
cargo run --release --bin veiled-registry-grpc -- --listen "[::1]:50051" &
sleep 1

echo ""
echo "=== Starting Merchants ==="
# Start 3 merchant servers (they'll register with registry and wait for finalization)
cargo run --release --bin merchant -- --name "CoffeeCo" --origin "https://coffeeco.com" --listen "[::1]:50061" --set-id 1 &
cargo run --release --bin merchant -- --name "BookStore" --origin "https://bookstore.com" --listen "[::1]:50062" --set-id 1 &
cargo run --release --bin merchant -- --name "TechMart" --origin "https://techmart.com" --listen "[::1]:50063" --set-id 1 &
sleep 1

echo ""
echo "=== Starting Next.js UI ==="
cd ui
npm run dev &
cd ..

echo ""
echo "======================================="
echo "  Veiled UI ready at http://localhost:3000"
echo ""
echo "  Services:"
echo "    Registry:  [::1]:50051"
echo "    CoffeeCo:  [::1]:50061"
echo "    BookStore: [::1]:50062"
echo "    TechMart:  [::1]:50063"
echo ""
echo "  Press Ctrl+C to stop all services"
echo "======================================="

wait
