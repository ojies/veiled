# Project Layout

```
veiled/
├── Cargo.toml                           # single package with lib + 6 binaries
├── build.rs                             # protobuf compilation (tonic-build)
├── .dockerignore                        # excludes target/, .wallets/, node_modules/
├── proto/
│   ├── registry.proto                   # Registry gRPC service definition
│   └── merchant.proto                   # Merchant gRPC service definition
├── src/
│   ├── lib.rs                           # crate root (pub mod core, registry)
│   ├── core/                            # cryptographic primitives & protocol logic
│   │   ├── mod.rs                       # public API re-exports
│   │   ├── crs.rs                       # CRS setup, HashToCurve generators
│   │   ├── credential.rs               # MasterCredential (Phase 1)
│   │   ├── beneficiary.rs              # Beneficiary lifecycle (Phases 1-5)
│   │   ├── merchant.rs                 # Merchant type + registration verification
│   │   ├── registry.rs                 # Identity registry (CRS + anonymity set)
│   │   ├── payment_identity.rs         # ZK proof for payment identity (Phase 3-4)
│   │   ├── request.rs                  # Payment request + Schnorr proof (Phase 5)
│   │   ├── nullifier.rs               # HKDF nullifier derivation
│   │   ├── types.rs                    # Commitment, Name, MasterSecret, etc.
│   │   ├── utils.rs                    # Pedersen commitment helper
│   │   ├── tx.rs                       # Transaction construction utilities
│   │   ├── verifier.rs                # Proof verification state machine
│   │   └── full_flow_test.rs          # End-to-end Phase 0-5 test
│   ├── registry/                       # gRPC registry service
│   │   ├── mod.rs                      # protobuf includes
│   │   ├── db.rs                       # SQLite persistence (merchants, sets, commitments, wallet key)
│   │   ├── store.rs                    # RegistryStore (state + wallet keypair, replays from SQLite on restart)
│   │   └── service.rs                  # RegistryService (gRPC handlers)
│   └── bin/
│       ├── registry_grpc.rs            # Registry server entry point
│       ├── beneficiary.rs              # Beneficiary CLI (Phases 1-5)
│       ├── veiled_core.rs             # JSON helper for web UI crypto ops
│       ├── veiled_wallet.rs           # BDK wallet binary (BIP86 P2TR, 7 commands)
│       ├── simulation.rs              # Full protocol simulation (3 merchants, 8 beneficiaries)
│       └── merchant/
│           ├── main.rs                 # Merchant server entry point
│           └── service.rs              # MerchantGrpcService handlers
├── Dockerfile                           # Multi-stage: rust-builder → registry | ui targets
├── docker-compose.yml                  # Full stack: bitcoind, explorer, registry, chain-init, ui
├── .github/
│   └── workflows/
│       └── ci.yml                      # Rust check/clippy/test + UI tsc/build + Docker build
├── docs/
│   ├── PROTOCOL.md                    # Protocol overview (6 phases, security properties)
│   ├── SCENARIO.md                    # End-to-end walkthrough (Alice, CoffeeCo, BookStore)
│   ├── API.md                         # gRPC + REST API + CLI reference
│   ├── CRYPTOGRAPHY.md                # Cryptographic primitives, Bootle/Groth proof, terminology
│   ├── LAYOUT.md                      # This file
│   ├── annomymous-credential.pdf      # ASC paper by Alupotha et al.
│   └── images/
│       ├── banner.svg                 # Project banner
│       └── logo.svg                   # Project logo
├── scripts/
│   ├── dev.sh                         # Launch via Docker Compose
│   ├── docker-registry-entrypoint.sh  # Registry container: wait for bitcoind, create wallet, start
│   ├── docker-ui-entrypoint.sh        # UI container: wait for registry, start web UI
│   └── docker-init-chain.sh           # Init container: create miner wallet, mine blocks, fund registry
├── ui/                                 # Next.js web UI (React + TypeScript)
│   ├── app/
│   │   ├── page.tsx                   # Landing page — role selector + protocol overview
│   │   ├── demo/page.tsx             # Demo controls — launch, fund wallets, reset
│   │   ├── beneficiary/page.tsx      # Beneficiary flow (credential → register → receive payment)
│   │   ├── merchant/page.tsx         # Merchant dashboard (registrations + send payments)
│   │   └── api/                       # API routes
│   │       ├── setup/init/            # Lazy set creation from registered merchants
│   │       ├── wallet/                # create, balance, send, faucet, tx
│   │       ├── beneficiary/           # credential, register, finalize, payment-id, payment, incoming
│   │       ├── merchant/              # create, identities, payments
│   │       ├── state/                 # Get simulation state
│   │       └── reset/                 # Reset all state
│   ├── components/                    # Reusable UI components
│   │   ├── Stepper.tsx               # Horizontal progress stepper
│   │   ├── PhaseCard.tsx             # Collapsible section wrapper
│   │   ├── WalletCard.tsx            # Balance display + send/receive
│   │   ├── HexDisplay.tsx            # Truncated hex with copy button
│   │   ├── Toast.tsx                 # Notification toast
│   │   ├── ToastProvider.tsx         # Toast context provider
│   │   └── FaucetButton.tsx          # Fund all wallets via regtest mining
│   └── lib/
│       ├── config.ts                  # Centralized configuration (env var overrides)
│       ├── grpc.ts                    # gRPC client (@grpc/grpc-js)
│       ├── helper.ts                  # veiled-core CLI wrapper
│       ├── wallet.ts                  # veiled-wallet CLI wrapper
│       ├── state.ts                   # In-memory simulation state
│       ├── types.ts                   # TypeScript interfaces
│       └── useLocalState.ts           # localStorage persistence hook
└── tests/
    └── registry_grpc_test.rs          # gRPC integration tests
```
