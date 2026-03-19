# Veiled — Local Development Makefile
# Runs everything natively: bitcoind (regtest), registry, UI, chain init.
#
# Usage:
#   make setup        # one-time: install Node 22, npm deps, build Rust
#   make dev          # start all services (bitcoind + registry + UI)
#   make stop         # stop all background services
#   make clean        # stop + remove data (wallets, db, regtest chain)
#   make build-rust   # rebuild Rust binaries only
#   make build-ui     # rebuild UI deps only
#   make logs         # tail last 20 lines of each service log
#   make status       # show running/stopped state of each service

SHELL := /bin/bash

# ── Paths ──
ROOT       := $(shell pwd)
UI_DIR     := $(ROOT)/ui
WALLETS    := $(ROOT)/.wallets
DATA_DIR   := $(ROOT)/.data
PIDS_DIR   := $(ROOT)/.pids
LOG_DIR    := $(ROOT)/.logs

# ── Rust binaries ──
REGISTRY_BIN := $(ROOT)/target/release/veiled-registry-grpc
MERCHANT_BIN := $(ROOT)/target/release/merchant
WALLET_BIN   := $(ROOT)/target/release/veiled-wallet
HELPER_BIN   := $(ROOT)/target/release/veiled-core

# ── Bitcoin config ──
BTC_DATADIR  := $(DATA_DIR)/bitcoin
BTC_RPC_PORT := 18443
BTC_RPC_URL  := http://localhost:$(BTC_RPC_PORT)
BTC_RPC_USER := veiled
BTC_RPC_PASS := veiled

# ── Registry config ──
REGISTRY_LISTEN := [::1]:50051
REGISTRY_DB     := $(DATA_DIR)/registry.db

# ── UI config ──
UI_PORT := 3000

# ── Node (need >=20 for Next.js 16) ──
NODE22 := $(shell command -v node 2>/dev/null)
NODE_OK := $(shell node -e "process.exit(parseInt(process.version.slice(1))>=20?0:1)" 2>/dev/null && echo yes || echo no)

# Use n-managed Node 22 if system Node is too old
ifeq ($(NODE_OK),no)
  N_NODE := /usr/local/bin/node
  NPM    := /usr/local/bin/npm
  NPX    := /usr/local/bin/npx
else
  N_NODE := node
  NPM    := npm
  NPX    := npx
endif

.PHONY: setup dev stop clean build-rust build-ui start-bitcoind start-registry \
        init-chain start-ui check-node logs status

# ═══════════════════════════════════════════════════════════════
# Setup (one-time)
# ═══════════════════════════════════════════════════════════════

setup: check-node build-rust build-ui
	@echo ""
	@echo "✅ Setup complete. Run 'make dev' to start."

check-node:
ifeq ($(NODE_OK),no)
	@echo "⚠ Node.js >= 20 required (have $(shell node --version)). Installing Node 22 via n..."
	@sudo n 22 || (echo "❌ Failed to install Node 22. Install manually: sudo n 22"; exit 1)
	@echo "✅ Node $$(node --version) installed"
else
	@echo "✅ Node $$($(N_NODE) --version) OK"
endif

build-rust:
	@echo "🔨 Building Rust binaries (release)..."
	cargo build --release \
	  --bin veiled-registry-grpc \
	  --bin merchant \
	  --bin veiled-core \
	  --bin veiled-wallet
	@echo "✅ Rust binaries built"

build-ui:
	@echo "📦 Installing UI dependencies..."
	cd $(UI_DIR) && $(NPM) install
	@echo "✅ UI dependencies installed"

# ═══════════════════════════════════════════════════════════════
# Dev (start everything)
# ═══════════════════════════════════════════════════════════════

dev:
	@mkdir -p $(WALLETS) $(DATA_DIR) $(PIDS_DIR) $(LOG_DIR) $(BTC_DATADIR)
	@$(MAKE) start-bitcoind
	@sleep 2
	@$(MAKE) start-registry
	@sleep 1
	@$(MAKE) init-chain
	@$(MAKE) start-ui
	@echo ""
	@echo "════════════════════════════════════════════"
	@echo "  Veiled is running!"
	@echo ""
	@echo "  UI:       http://localhost:$(UI_PORT)"
	@echo "  Demo:     http://localhost:$(UI_PORT)/demo"
	@echo "  Registry: $(REGISTRY_LISTEN)"
	@echo "  Bitcoin:  $(BTC_RPC_URL)"
	@echo "  Explorer: (not running locally)"
	@echo ""
	@echo "  Logs:     $(LOG_DIR)/"
	@echo "  Stop:     make stop"
	@echo "════════════════════════════════════════════"

start-bitcoind:
	@if [ -f $(PIDS_DIR)/bitcoind.pid ] && kill -0 $$(cat $(PIDS_DIR)/bitcoind.pid) 2>/dev/null; then \
		echo "bitcoind already running (pid $$(cat $(PIDS_DIR)/bitcoind.pid))"; \
	else \
		echo "🟠 Starting bitcoind (regtest)..."; \
		bitcoind \
			-regtest \
			-server \
			-txindex \
			-datadir=$(BTC_DATADIR) \
			-rpcport=$(BTC_RPC_PORT) \
			-rpcuser=$(BTC_RPC_USER) \
			-rpcpassword=$(BTC_RPC_PASS) \
			-fallbackfee=0.00001 \
			-daemon \
			-pid=$(PIDS_DIR)/bitcoind.pid \
			> $(LOG_DIR)/bitcoind.log 2>&1; \
		sleep 1; \
		echo "✅ bitcoind started (pid $$(cat $(PIDS_DIR)/bitcoind.pid 2>/dev/null || echo '?'))"; \
	fi

start-registry:
	@if [ -f $(PIDS_DIR)/registry.pid ] && kill -0 $$(cat $(PIDS_DIR)/registry.pid) 2>/dev/null; then \
		echo "Registry already running (pid $$(cat $(PIDS_DIR)/registry.pid))"; \
	else \
		echo "🟢 Starting registry..."; \
		mkdir -p $(WALLETS); \
		WALLETS_DIR=$(WALLETS) \
		$(WALLET_BIN) <<< '{"command":"create-wallet","state_path":"$(WALLETS)/registry.json","name":"registry","rpc_url":"$(BTC_RPC_URL)","rpc_user":"$(BTC_RPC_USER)","rpc_pass":"$(BTC_RPC_PASS)"}' > /dev/null 2>&1 || true; \
		BITCOIN_RPC_URL=$(BTC_RPC_URL) \
		BITCOIN_RPC_USER=$(BTC_RPC_USER) \
		BITCOIN_RPC_PASS=$(BTC_RPC_PASS) \
		REGISTRY_DB_PATH=$(REGISTRY_DB) \
		$(REGISTRY_BIN) --listen $(REGISTRY_LISTEN) \
			> $(LOG_DIR)/registry.log 2>&1 & \
		echo $$! > $(PIDS_DIR)/registry.pid; \
		echo "✅ Registry started (pid $$(cat $(PIDS_DIR)/registry.pid))"; \
	fi

init-chain:
	@echo "⛏  Initializing chain (mining blocks, funding registry)..."
	@# Create miner wallet
	@$(WALLET_BIN) <<< '{"command":"create-wallet","state_path":"$(WALLETS)/miner.json","name":"miner","rpc_url":"$(BTC_RPC_URL)","rpc_user":"$(BTC_RPC_USER)","rpc_pass":"$(BTC_RPC_PASS)"}' > /dev/null 2>&1 || true
	@# Get miner address
	@MINER_ADDR=$$($(WALLET_BIN) <<< '{"command":"get-address","state_path":"$(WALLETS)/miner.json","rpc_url":"$(BTC_RPC_URL)","rpc_user":"$(BTC_RPC_USER)","rpc_pass":"$(BTC_RPC_PASS)"}' | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4); \
	echo "  Miner: $$MINER_ADDR"; \
	$(WALLET_BIN) <<< "{\"command\":\"faucet\",\"address\":\"$$MINER_ADDR\",\"blocks\":101,\"rpc_url\":\"$(BTC_RPC_URL)\",\"rpc_user\":\"$(BTC_RPC_USER)\",\"rpc_pass\":\"$(BTC_RPC_PASS)\"}" > /dev/null 2>&1; \
	REGISTRY_ADDR=$$($(WALLET_BIN) <<< '{"command":"get-address","state_path":"$(WALLETS)/registry.json","rpc_url":"$(BTC_RPC_URL)","rpc_user":"$(BTC_RPC_USER)","rpc_pass":"$(BTC_RPC_PASS)"}' | grep -o '"address":"[^"]*"' | head -1 | cut -d'"' -f4); \
	echo "  Registry: $$REGISTRY_ADDR"; \
	$(WALLET_BIN) <<< "{\"command\":\"faucet\",\"address\":\"$$REGISTRY_ADDR\",\"blocks\":1,\"rpc_url\":\"$(BTC_RPC_URL)\",\"rpc_user\":\"$(BTC_RPC_USER)\",\"rpc_pass\":\"$(BTC_RPC_PASS)\"}" > /dev/null 2>&1; \
	$(WALLET_BIN) <<< "{\"command\":\"faucet\",\"address\":\"$$MINER_ADDR\",\"blocks\":100,\"rpc_url\":\"$(BTC_RPC_URL)\",\"rpc_user\":\"$(BTC_RPC_USER)\",\"rpc_pass\":\"$(BTC_RPC_PASS)\"}" > /dev/null 2>&1
	@echo "✅ Chain initialized, registry funded"

start-ui:
	@if [ -f $(PIDS_DIR)/ui.pid ] && kill -0 $$(cat $(PIDS_DIR)/ui.pid) 2>/dev/null; then \
		echo "UI already running (pid $$(cat $(PIDS_DIR)/ui.pid))"; \
	else \
		echo "🔵 Starting Next.js UI (port $(UI_PORT))..."; \
		cd $(UI_DIR) && \
		BITCOIN_RPC_URL=$(BTC_RPC_URL) \
		BITCOIN_RPC_USER=$(BTC_RPC_USER) \
		BITCOIN_RPC_PASS=$(BTC_RPC_PASS) \
		REGISTRY_ADDRESS=[::1]:50051 \
		REGISTRY_SERVER=http://[::1]:50051 \
		WALLET_BIN=$(WALLET_BIN) \
		HELPER_BIN=$(HELPER_BIN) \
		MERCHANT_BIN=$(MERCHANT_BIN) \
		WALLETS_DIR=$(WALLETS) \
		PROTO_DIR=$(ROOT)/proto \
		BENEFICIARY_CAPACITY=4 \
		MIN_MERCHANTS=2 \
		MATURITY_BLOCKS=10 \
		MERCHANT_START_PORT=50061 \
		PORT=$(UI_PORT) \
		$(N_NODE) node_modules/.bin/next dev --webpack --port $(UI_PORT) \
			> $(LOG_DIR)/ui.log 2>&1 & \
		echo $$! > $(PIDS_DIR)/ui.pid; \
		echo "✅ UI starting (pid $$(cat $(PIDS_DIR)/ui.pid)) — wait a few seconds for compilation"; \
	fi

# ═══════════════════════════════════════════════════════════════
# Stop
# ═══════════════════════════════════════════════════════════════

stop:
	@echo "Stopping services..."
	@# Helper: kill PID file if process alive, always remove file
	@for svc in ui registry; do \
		f=$(PIDS_DIR)/$$svc.pid; \
		if [ -f "$$f" ]; then \
			PID=$$(cat "$$f"); \
			kill $$PID 2>/dev/null && echo "  $$svc stopped (pid $$PID)" || echo "  $$svc already dead"; \
			rm -f "$$f"; \
		fi; \
	done
	@# Stop bitcoind via RPC (3s timeout) then fallback to kill
	@f=$(PIDS_DIR)/bitcoind.pid; \
	if [ -f "$$f" ]; then \
		PID=$$(cat "$$f"); \
		if kill -0 $$PID 2>/dev/null; then \
			timeout 3 bitcoin-cli -regtest -datadir=$(BTC_DATADIR) \
				-rpcport=$(BTC_RPC_PORT) -rpcuser=$(BTC_RPC_USER) -rpcpassword=$(BTC_RPC_PASS) \
				stop 2>/dev/null || kill $$PID 2>/dev/null || true; \
			echo "  bitcoind stopped (pid $$PID)"; \
		else \
			echo "  bitcoind already dead"; \
		fi; \
		rm -f "$$f"; \
	fi
	@echo "✅ All stopped"

# ═══════════════════════════════════════════════════════════════
# Clean (stop + wipe data)
# ═══════════════════════════════════════════════════════════════

clean: stop
	@echo "🗑  Removing data..."
	rm -rf $(WALLETS) $(DATA_DIR) $(PIDS_DIR) $(LOG_DIR)
	rm -rf $(UI_DIR)/.next
	@echo "✅ Clean"

# ═══════════════════════════════════════════════════════════════
# Shortcuts
# ═══════════════════════════════════════════════════════════════

logs:
	@echo "=== bitcoind ===" && tail -20 $(LOG_DIR)/bitcoind.log 2>/dev/null || true
	@echo ""
	@echo "=== registry ===" && tail -20 $(LOG_DIR)/registry.log 2>/dev/null || true
	@echo ""
	@echo "=== ui ===" && tail -20 $(LOG_DIR)/ui.log 2>/dev/null || true

status:
	@echo "Services:"
	@for svc in bitcoind registry ui; do \
		if [ -f $(PIDS_DIR)/$$svc.pid ] && kill -0 $$(cat $(PIDS_DIR)/$$svc.pid) 2>/dev/null; then \
			echo "  $$svc: running (pid $$(cat $(PIDS_DIR)/$$svc.pid))"; \
		else \
			echo "  $$svc: stopped"; \
		fi; \
	done
