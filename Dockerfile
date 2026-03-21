# ── Stage 1: Build Rust binaries ──
FROM docker.io/rust:1.87-bookworm AS rust-builder

RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ src/
COPY proto/ proto/

ARG VEILED_M=2
ENV VEILED_M=${VEILED_M}

RUN cargo build --release \
    --bin veiled-registry-grpc \
    --bin merchant \
    --bin veiled-core \
    --bin veiled-wallet

# ── Stage 2: Build Next.js UI ──
FROM docker.io/node:22-bookworm-slim AS node-builder

WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci

COPY ui/ ./
# Proto files needed at build time by grpc loader
COPY proto/ /build/proto/

ENV NEXT_TELEMETRY_DISABLED=1
RUN npm run build

# ── Stage 3: Registry runtime ──
FROM docker.io/debian:bookworm-slim AS registry

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /build/target/release/veiled-registry-grpc /usr/local/bin/
COPY --from=rust-builder /build/target/release/veiled-wallet /usr/local/bin/
COPY scripts/docker-registry-entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 50051
ENTRYPOINT ["/entrypoint.sh"]

# ── Stage 4: UI runtime ──
FROM docker.io/node:22-bookworm-slim AS ui

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Rust binaries the UI spawns at runtime
COPY --from=rust-builder /build/target/release/veiled-core /app/bin/veiled-core
COPY --from=rust-builder /build/target/release/veiled-wallet /app/bin/veiled-wallet
COPY --from=rust-builder /build/target/release/merchant /app/bin/merchant

# Proto files for gRPC client
COPY proto/ /app/proto/

# Next.js standalone build
COPY --from=node-builder /build/ui/.next/standalone/ /app/ui/
COPY --from=node-builder /build/ui/.next/static/ /app/ui/.next/static/
COPY --from=node-builder /build/ui/public/ /app/ui/public/

# Entrypoint
COPY scripts/docker-ui-entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /app/data/wallets

ENV WALLET_BIN=/app/bin/veiled-wallet \
    HELPER_BIN=/app/bin/veiled-core \
    MERCHANT_BIN=/app/bin/merchant \
    WALLETS_DIR=/app/data/wallets \
    PROTO_DIR=/app/proto \
    NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1

EXPOSE 3000
ENTRYPOINT ["/entrypoint.sh"]
