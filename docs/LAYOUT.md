# Project Layout

```
veiled/
├── Cargo.toml                           # single package with lib + 3 binaries
├── build.rs                             # protobuf compilation (tonic-build)
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
│   │   ├── tx.rs                       # VTxO tree construction
│   │   ├── verifier.rs                # Proof verification state machine
│   │   └── full_flow_test.rs          # End-to-end Phase 0-5 test
│   ├── registry/                       # gRPC registry service
│   │   ├── mod.rs                      # protobuf includes
│   │   ├── store.rs                    # RegistryStore (in-memory state)
│   │   └── service.rs                  # RegistryService (gRPC handlers)
│   └── bin/
│       ├── registry_grpc.rs            # Registry server entry point
│       ├── beneficiary.rs              # Beneficiary CLI (Phases 1-5)
│       └── merchant/
│           ├── main.rs                 # Merchant server entry point
│           └── service.rs              # MerchantGrpcService handlers
├── docs/
│   ├── SCENARIO.md                     # End-to-end walkthrough
│   ├── API.md                          # gRPC API reference
│   ├── CRYPTOGRAPHY.md                 # Cryptographic primitives + terminology
│   ├── LAYOUT.md                       # This file
│   └── annomymous-credential.pdf       # ASC paper
└── tests/
    └── registry_grpc_test.rs           # gRPC integration tests
```
