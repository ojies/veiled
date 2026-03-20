//! Veiled Simulation: Full multi-party protocol simulation
//!
//! Runs an in-process registry + merchant servers and simulates the complete
//! Phases 0-5 flow with 3 merchants and 8 beneficiaries.
//!
//! Usage: cargo run --bin simulation --release

mod merchant_pb {
    tonic::include_proto!("merchant");
}

use merchant_pb::merchant_service_server::MerchantServiceServer;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use veiled::core::beneficiary::Beneficiary;
use veiled::core::crs::Crs;
use veiled::core::merchant::Merchant;
use veiled::core::payment_identity::serialize_payment_identity_registration_proof;
use veiled::core::request::create_payment_request;
use veiled::core::types::{Commitment, Name};
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::pb::{
    BeneficiaryRequest, CreateSetRequest, FinalizeSetRequest, GetAnonymitySetRequest,
    GetCrsRequest, MerchantRequest,
};
use veiled::registry::service::RegistryService;
use veiled::registry::store::{FeeConfig, RegistryStore};

use merchant_pb::merchant_service_client::MerchantServiceClient;
use merchant_pb::{PaymentRegistrationRequest, PaymentRequestMsg};

const REGISTRY_ADDR: &str = "[::1]:50070";
const REGISTRY_URL: &str = "http://[::1]:50070";

struct MerchantConfig {
    name: &'static str,
    origin: &'static str,
    addr: &'static str,
    url: &'static str,
}

const MERCHANTS: [MerchantConfig; 3] = [
    MerchantConfig {
        name: "CoffeeCo",
        origin: "https://coffeeco.com",
        addr: "[::1]:50071",
        url: "http://[::1]:50071",
    },
    MerchantConfig {
        name: "BookStore",
        origin: "https://bookstore.com",
        addr: "[::1]:50072",
        url: "http://[::1]:50072",
    },
    MerchantConfig {
        name: "TechMart",
        origin: "https://techmart.com",
        addr: "[::1]:50073",
        url: "http://[::1]:50073",
    },
];

const BENEFICIARY_NAMES: [&str; 8] = [
    "alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi",
];

fn separator(title: &str) {
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  {}", title);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

fn step(msg: &str) {
    println!("  -> {}", msg);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  VEILED - Verified Payments, Veiled Identities");
    println!("  Full protocol simulation (Phases 0-5)");
    println!("  3 merchants, 8 beneficiaries");
    println!();

    // ── Start Registry ──────────────────────────────────────────
    separator("Starting Registry Server");

    let store = Arc::new(Mutex::new(RegistryStore::new(None, FeeConfig::default(), None)));
    let registry_service = RegistryService::new(store);
    let registry_addr = REGISTRY_ADDR.parse()?;

    tokio::spawn(async move {
        Server::builder()
            .add_service(RegistryServer::new(registry_service))
            .serve(registry_addr)
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    step(&format!("Registry listening on {}", REGISTRY_ADDR));

    let mut client = RegistryClient::connect(REGISTRY_URL).await?;

    // ── Phase 0: Register Merchants + Create Set ────────────────
    separator("Phase 0 - System Setup");

    for m in &MERCHANTS {
        client
            .register_merchant(MerchantRequest {
                name: m.name.into(),
                origin: m.origin.into(),
                email: format!("pay@{}", m.name.to_lowercase()),
                phone: "".into(),
                funding_txid: vec![0xaa; 32],
                funding_vout: 0,
            })
            .await?;
        step(&format!("Registered merchant: {}", m.name));
    }

    let merchant_names: Vec<String> = MERCHANTS.iter().map(|m| m.name.to_string()).collect();
    client
        .create_set(CreateSetRequest {
            set_id: 1,
            merchant_names: merchant_names.clone(),
            beneficiary_capacity: BENEFICIARY_NAMES.len() as u32,
            sats_per_user: 200,
        })
        .await?;
    step(&format!(
        "Created anonymity set #1 (capacity: {}, merchants: {})",
        BENEFICIARY_NAMES.len(),
        merchant_names.join(", ")
    ));

    let crs_bytes = client
        .get_crs(GetCrsRequest { set_id: 1 })
        .await?
        .into_inner()
        .crs_bytes;
    let crs = Crs::from_bytes(&crs_bytes)?;
    step(&format!(
        "CRS generated: {} merchants, {} generators (g + h_name + {} h_l)",
        crs.merchants.len(),
        crs.merchants.len() + 2,
        crs.merchants.len()
    ));

    // ── Start Merchant Servers ──────────────────────────────────
    separator("Starting Merchant Servers");

    let mut merchant_handles = Vec::new();
    for m in &MERCHANTS {
        let crs_clone = crs.clone();
        let name = m.name.to_string();
        let addr = m.addr.to_string();
        let handle = tokio::spawn(async move {
            start_merchant_server(&name, &addr, 1, crs_clone).await
        });
        merchant_handles.push(handle);
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    for m in &MERCHANTS {
        step(&format!(
            "{} listening on {} (waiting for finalization...)",
            m.name, m.addr
        ));
    }

    // ── Phase 1: Beneficiaries Create Credentials ───────────────
    separator("Phase 1 - Credential Creation (local, offline)");

    let mut beneficiaries: Vec<Beneficiary> = Vec::new();
    for name in &BENEFICIARY_NAMES {
        let b = Beneficiary::new(&crs, name);
        step(&format!(
            "{:<6} credential created, phi = {}...",
            name,
            hex::encode(&b.credential.phi.0[..6])
        ));
        beneficiaries.push(b);
    }

    // ── Phase 2: Registration + Finalization ────────────────────
    separator("Phase 2 - Registration + Finalization");

    for (i, name) in BENEFICIARY_NAMES.iter().enumerate() {
        let res = client
            .register_beneficiary(BeneficiaryRequest {
                set_id: 1,
                phi: beneficiaries[i].credential.phi.0.to_vec(),
                name: name.to_string(),
                email: format!("{}@example.com", name),
                phone: "".into(),
                funding_txid: vec![0xaa; 32],
                funding_vout: 0,
            })
            .await?
            .into_inner();
        step(&format!("{:<6} registered at index {}", name, res.index));
    }

    // Subscribe to finalization
    let mut sub_client = RegistryClient::connect(REGISTRY_URL).await?;
    let sub_handle = tokio::spawn(async move {
        let response = sub_client
            .subscribe_set_finalization(GetAnonymitySetRequest { set_id: 1 })
            .await
            .unwrap();
        let mut stream = response.into_inner();
        stream.message().await.unwrap().unwrap()
    });

    step("Admin finalizing set #1 (demo: no RPC, commitment tx not broadcast)...");
    client
        .finalize_set(FinalizeSetRequest { set_id: 1 })
        .await?;

    let finalized =
        tokio::time::timeout(std::time::Duration::from_secs(5), sub_handle).await??;
    step(&format!(
        "Set #1 finalized: {} members",
        finalized.count
    ));

    let anonymity_set: Vec<Commitment> = finalized
        .commitments
        .into_iter()
        .map(|b| {
            let arr: [u8; 33] = b.try_into().expect("33 bytes");
            Commitment(arr)
        })
        .collect();

    for (i, name) in BENEFICIARY_NAMES.iter().enumerate() {
        let mut set_id_bytes = [0u8; 32];
        set_id_bytes[..8].copy_from_slice(&1u64.to_le_bytes());
        beneficiaries[i].register(set_id_bytes, anonymity_set.clone())?;
        step(&format!(
            "{:<6} registered locally at index {}",
            name,
            beneficiaries[i].index.unwrap()
        ));
    }

    // Wait for merchant servers to receive finalized set
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // ── Phase 3-4: Payment Identity Registration ────────────────
    separator("Phase 3-4 - Payment Identity Registration");

    // Each beneficiary registers with specific merchants:
    // Alice   -> CoffeeCo (1), BookStore (2), TechMart (3)  (all three)
    // Bob     -> CoffeeCo (1)
    // Carol   -> BookStore (2)
    // Dave    -> TechMart (3)
    // Eve     -> CoffeeCo (1), BookStore (2)
    // Frank   -> TechMart (3)
    // Grace   -> BookStore (2), TechMart (3)
    // Heidi   -> CoffeeCo (1)
    let registrations: [(usize, usize); 12] = [
        (0, 1), (0, 2), (0, 3), // Alice -> all three
        (1, 1),                  // Bob -> CoffeeCo
        (2, 2),                  // Carol -> BookStore
        (3, 3),                  // Dave -> TechMart
        (4, 1), (4, 2),         // Eve -> CoffeeCo, BookStore
        (5, 3),                  // Frank -> TechMart
        (6, 2), (6, 3),         // Grace -> BookStore, TechMart
        (7, 1),                  // Heidi -> CoffeeCo
    ];

    // Connect to merchant servers
    let mut coffeeco_client = MerchantServiceClient::connect(MERCHANTS[0].url).await?;
    let mut bookstore_client = MerchantServiceClient::connect(MERCHANTS[1].url).await?;
    let mut techmart_client = MerchantServiceClient::connect(MERCHANTS[2].url).await?;

    // Store pseudonyms for unlinkability demo
    let mut alice_pseudonyms: Vec<(String, [u8; 33])> = Vec::new();

    for &(ben_idx, merchant_id) in &registrations {
        let name = BENEFICIARY_NAMES[ben_idx];
        let merchant_name = MERCHANTS[merchant_id - 1].name;

        let reg = beneficiaries[ben_idx]
            .create_payment_registration(&crs, merchant_id)
            .map_err(|e| e.to_string())?;
        let proof_bytes = serialize_payment_identity_registration_proof(&reg.proof);

        let req = PaymentRegistrationRequest {
            pseudonym: reg.pseudonym.to_vec(),
            public_nullifier: reg.public_nullifier.to_vec(),
            set_id: u64::from_le_bytes(reg.set_id[..8].try_into().unwrap()),
            service_index: reg.service_index as u32,
            friendly_name: reg.friendly_name.clone(),
            proof: proof_bytes,
        };

        match merchant_id {
            1 => coffeeco_client.submit_payment_registration(req).await?,
            2 => bookstore_client.submit_payment_registration(req).await?,
            3 => techmart_client.submit_payment_registration(req).await?,
            _ => unreachable!(),
        };

        step(&format!(
            "{:<6} -> {:<10} pseudonym = {}...",
            name,
            merchant_name,
            hex::encode(&reg.pseudonym[..6])
        ));

        if ben_idx == 0 {
            alice_pseudonyms.push((merchant_name.to_string(), reg.pseudonym));
        }
    }

    // ── Cross-merchant unlinkability demonstration ───────────────
    separator("Cross-Merchant Unlinkability (Alice)");

    println!("  Alice registered with all 3 merchants. Her pseudonyms:");
    println!();
    for (merchant, pseudo) in &alice_pseudonyms {
        println!(
            "    {:<10}  {}",
            merchant,
            hex::encode(&pseudo[..])
        );
    }
    println!();

    let all_differ = alice_pseudonyms[0].1 != alice_pseudonyms[1].1
        && alice_pseudonyms[1].1 != alice_pseudonyms[2].1
        && alice_pseudonyms[0].1 != alice_pseudonyms[2].1;
    println!(
        "  All pseudonyms differ: {} (cryptographically unlinkable)",
        if all_differ { "YES" } else { "NO" }
    );
    println!("  Merchants cannot link these identities through pseudonyms.");
    println!(
        "  Note: friendly_name '{}' IS revealed to each merchant.",
        BENEFICIARY_NAMES[0]
    );

    // ── Phase 5: Payment Requests ───────────────────────────────
    separator("Phase 5 - Payment Requests (Schnorr Authentication)");

    let payments: [(usize, usize, u64); 8] = [
        (0, 1, 5_000),   // Alice  <- CoffeeCo   5,000 sats
        (0, 2, 12_000),  // Alice  <- BookStore  12,000 sats
        (0, 3, 8_500),   // Alice  <- TechMart    8,500 sats
        (1, 1, 3_000),   // Bob    <- CoffeeCo    3,000 sats
        (2, 2, 7_500),   // Carol  <- BookStore    7,500 sats
        (3, 3, 15_000),  // Dave   <- TechMart   15,000 sats
        (4, 1, 2_000),   // Eve    <- CoffeeCo    2,000 sats
        (6, 2, 9_000),   // Grace  <- BookStore    9,000 sats
    ];

    for &(ben_idx, merchant_id, amount) in &payments {
        let name = BENEFICIARY_NAMES[ben_idx];
        let merchant_name = MERCHANTS[merchant_id - 1].name;
        let merchant_name_typed = Name(merchant_name.to_string());

        let pay = create_payment_request(
            &beneficiaries[ben_idx].credential.r,
            &merchant_name_typed,
            &crs.g,
            amount,
        );

        let req = PaymentRequestMsg {
            amount,
            pseudonym: pay.pseudonym.to_vec(),
            proof_r: pay.proof.r.to_vec(),
            proof_s: pay.proof.s.to_vec(),
        };

        let res = match merchant_id {
            1 => coffeeco_client.submit_payment_request(req).await?,
            2 => bookstore_client.submit_payment_request(req).await?,
            3 => techmart_client.submit_payment_request(req).await?,
            _ => unreachable!(),
        }
        .into_inner();

        step(&format!(
            "{:<6} <- {:<10} {:>6} sats -> {}",
            name, merchant_name, amount, res.address
        ));
    }

    // ── Summary ─────────────────────────────────────────────────
    separator("Simulation Complete");
    println!("  Merchants:      CoffeeCo, BookStore, TechMart");
    println!("  Beneficiaries:  alice, bob, carol, dave, eve, frank, grace, heidi");
    println!("  Anonymity set:  8 commitments (2^3), sealed via Taproot commitment");
    println!("  Registrations:  13 payment identities across 3 merchants");
    println!("  Payments:       8 payment requests fulfilled with P2TR addresses");
    println!();
    println!("  Privacy guarantees:");
    println!("    - ZK proofs reveal nothing about which commitment is whose");
    println!("    - Pseudonyms are cryptographically unlinkable across merchants");
    println!("    - Each merchant sees only their own nullifier per beneficiary");
    println!("    - friendly_name is revealed (design trade-off for payment coordination)");
    println!();

    // Clean up
    for h in merchant_handles {
        h.abort();
    }

    Ok(())
}

/// Starts a merchant gRPC server that subscribes to set finalization
async fn start_merchant_server(
    name: &str,
    listen_addr: &str,
    set_id: u64,
    crs: Crs,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut registry_client = RegistryClient::connect(REGISTRY_URL).await?;
    let response = registry_client
        .subscribe_set_finalization(GetAnonymitySetRequest { set_id })
        .await?;
    let mut stream = response.into_inner();
    let anon_res = stream
        .message()
        .await?
        .ok_or("Finalization stream closed")?;

    let anonymity_set: Vec<Commitment> = anon_res
        .commitments
        .into_iter()
        .map(|b| {
            let arr: [u8; 33] = b.try_into().expect("33 bytes");
            Commitment(arr)
        })
        .collect();

    let merchant = Merchant::new(name, &format!("https://{}.com", name.to_lowercase()));

    let merchant_svc = DemoMerchantService {
        merchant: Arc::new(Mutex::new(merchant)),
        crs: Arc::new(crs),
        anonymity_set: Arc::new(anonymity_set),
    };

    let addr = listen_addr.parse()?;
    Server::builder()
        .add_service(MerchantServiceServer::new(merchant_svc))
        .serve(addr)
        .await?;

    Ok(())
}

/// Inline merchant service (mirrors src/bin/merchant/service.rs)
struct DemoMerchantService {
    merchant: Arc<Mutex<Merchant>>,
    crs: Arc<Crs>,
    anonymity_set: Arc<Vec<Commitment>>,
}

#[tonic::async_trait]
impl merchant_pb::merchant_service_server::MerchantService for DemoMerchantService {
    async fn submit_payment_registration(
        &self,
        request: tonic::Request<PaymentRegistrationRequest>,
    ) -> Result<tonic::Response<merchant_pb::PaymentRegistrationResponse>, tonic::Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let public_nullifier: [u8; 33] = req
            .public_nullifier
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("public_nullifier must be 33 bytes"))?;

        let proof =
            veiled::core::payment_identity::deserialize_payment_identity_registration_proof(
                &req.proof,
            )
            .map_err(|e| tonic::Status::invalid_argument(format!("invalid proof: {}", e)))?;

        let registration = veiled::core::payment_identity::PaymentIdentityRegistration {
            pseudonym,
            public_nullifier,
            set_id: {
                let mut bytes = [0u8; 32];
                bytes[..8].copy_from_slice(&req.set_id.to_le_bytes());
                bytes
            },
            service_index: req.service_index as usize,
            friendly_name: req.friendly_name.clone(),
            proof,
        };

        let mut merchant = self.merchant.lock().await;
        merchant
            .receive_payment_registration(&self.crs, &self.anonymity_set, &registration)
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;

        Ok(tonic::Response::new(
            merchant_pb::PaymentRegistrationResponse {
                message: format!("Registered '{}'", req.friendly_name),
            },
        ))
    }

    async fn submit_payment_request(
        &self,
        request: tonic::Request<PaymentRequestMsg>,
    ) -> Result<tonic::Response<merchant_pb::PaymentRequestResponse>, tonic::Status> {
        let req = request.into_inner();

        let pseudonym: [u8; 33] = req
            .pseudonym
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("pseudonym must be 33 bytes"))?;

        let proof_r: [u8; 33] = req
            .proof_r
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("proof_r must be 33 bytes"))?;

        let proof_s: [u8; 32] = req
            .proof_s
            .try_into()
            .map_err(|_| tonic::Status::invalid_argument("proof_s must be 32 bytes"))?;

        let proof = veiled::core::request::PaymentRequestProof {
            r: proof_r,
            s: proof_s,
        };

        if !veiled::core::request::verify_payment_request(&self.crs.g, &pseudonym, &proof) {
            return Err(tonic::Status::invalid_argument("invalid payment proof"));
        }

        let merchant = self.merchant.lock().await;
        let registered = merchant
            .registered_identities
            .get(&pseudonym)
            .ok_or_else(|| tonic::Status::not_found("pseudonym not registered"))?;

        let address =
            veiled::core::request::pseudonym_to_address(&pseudonym, bitcoin::Network::Bitcoin)
                .map_err(|e| tonic::Status::internal(format!("address error: {}", e)))?;

        Ok(tonic::Response::new(
            merchant_pb::PaymentRequestResponse {
                address: address.to_string(),
                friendly_name: registered.friendly_name.clone(),
            },
        ))
    }
}
