//! Stateless JSON helper for the web UI.
//!
//! Reads a JSON command from stdin, executes the crypto operation,
//! writes JSON result to stdout. No persistent state — the caller
//! (web UI API routes) manages state between calls.
//!
//! Commands:
//!   create-credential       Phase 1: generate secrets + compute Φ
//!   register-locally        Phase 2: find index in anonymity set
//!   create-payment-id       Phase 3: generate ZK proof for merchant
//!   create-payment-request  Phase 5: generate Schnorr proof

use serde::{Deserialize, Serialize};
use std::io::Read;

use veiled::core::beneficiary::Beneficiary;
use veiled::core::credential::MasterCredential;
use veiled::core::crs::Crs;
use veiled::core::payment_identity::serialize_payment_identity_registration_proof;
use veiled::core::request::create_payment_request;
use veiled::core::types::{
    BlindingKey, ChildRandomness, Commitment, FriendlyName, MasterSecret, Name,
};

// ── Serializable credential (MasterCredential doesn't impl Serialize) ──

#[derive(Serialize, Deserialize)]
struct CredentialJson {
    phi: String,
    sk: String,
    r: String,
    k: String,
    friendly_name: String,
}

impl CredentialJson {
    fn from_credential(c: &MasterCredential) -> Self {
        Self {
            phi: hex::encode(c.phi.as_bytes()),
            sk: hex::encode(c.sk.as_bytes()),
            r: hex::encode(c.r.as_bytes()),
            k: hex::encode(c.k.as_bytes()),
            friendly_name: c.friendly_name.as_str().to_string(),
        }
    }

    fn to_credential(&self) -> Result<MasterCredential, String> {
        let phi_bytes = hex_to_33(&self.phi)?;
        let sk_bytes = hex_to_32(&self.sk)?;
        let r_bytes = hex_to_32(&self.r)?;
        let k_bytes = hex_to_32(&self.k)?;
        Ok(MasterCredential {
            phi: Commitment(phi_bytes),
            sk: MasterSecret(sk_bytes),
            r: ChildRandomness(r_bytes),
            k: BlindingKey(k_bytes),
            friendly_name: FriendlyName::new(&self.friendly_name),
        })
    }
}

fn hex_to_32(h: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(h).map_err(|e| format!("hex decode: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "expected 32 bytes".to_string())
}

fn hex_to_33(h: &str) -> Result<[u8; 33], String> {
    let bytes = hex::decode(h).map_err(|e| format!("hex decode: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "expected 33 bytes".to_string())
}

// ── Command structs ──

#[derive(Deserialize)]
struct Command {
    command: String,
    #[serde(flatten)]
    params: serde_json::Value,
}

#[derive(Deserialize)]
struct CreateCredentialParams {
    crs_hex: String,
    name: String,
}

#[derive(Deserialize)]
struct RegisterLocallyParams {
    credential: CredentialJson,
    commitments_hex: Vec<String>,
}

#[derive(Deserialize)]
struct CreatePaymentIdParams {
    credential: CredentialJson,
    crs_hex: String,
    commitments_hex: Vec<String>,
    index: usize,
    set_id: u64,
    merchant_id: usize,
}

#[derive(Deserialize)]
struct CreatePaymentRequestParams {
    credential_r_hex: String,
    merchant_name: String,
    crs_g_hex: String,
    amount: u64,
}

// ── Response structs ──

#[derive(Serialize)]
struct CreateCredentialResponse {
    credential: CredentialJson,
}

#[derive(Serialize)]
struct RegisterLocallyResponse {
    index: usize,
}

#[derive(Serialize)]
struct CreatePaymentIdResponse {
    pseudonym: String,
    nullifier: String,
    proof_hex: String,
    friendly_name: String,
    service_index: usize,
    set_id: u64,
}

#[derive(Serialize)]
struct CreatePaymentRequestResponse {
    pseudonym: String,
    proof_r: String,
    proof_s: String,
    amount: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ── Handlers ──

fn handle_create_credential(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreateCredentialParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    eprintln!("[core] create-credential for '{}'", p.name);
    let crs_bytes = hex::decode(&p.crs_hex).map_err(|e| format!("crs hex: {e}"))?;
    eprintln!("[core] CRS decoded: {} bytes", crs_bytes.len());
    let crs = Crs::from_bytes(&crs_bytes).map_err(|e| e.to_string())?;
    eprintln!("[core] CRS parsed: {} merchants, {} generators", crs.merchants.len(), crs.generators.len());
    let ben = Beneficiary::new(&crs, &p.name);
    eprintln!("[core] credential created for '{}': phi = {}...", p.name, hex::encode(&ben.credential.phi.0[..8]));
    let resp = CreateCredentialResponse {
        credential: CredentialJson::from_credential(&ben.credential),
    };
    serde_json::to_value(resp).map_err(|e| e.to_string())
}

fn handle_register_locally(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: RegisterLocallyParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let credential = p.credential.to_credential()?;
    eprintln!("[core] register-locally: phi = {}..., set size = {}", hex::encode(&credential.phi.0[..8]), p.commitments_hex.len());
    let commitments: Vec<Commitment> = p
        .commitments_hex
        .iter()
        .map(|h| hex_to_33(h).map(Commitment))
        .collect::<Result<_, _>>()?;
    let index = commitments
        .iter()
        .position(|c| *c == credential.phi)
        .ok_or("commitment not found in set")?;
    eprintln!("[core] register-locally: found own commitment at index {}", index);
    let resp = RegisterLocallyResponse { index };
    serde_json::to_value(resp).map_err(|e| e.to_string())
}

fn handle_create_payment_id(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreatePaymentIdParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    eprintln!(
        "[core] create-payment-id: merchant_id={}, set_id={}, index={}, set_size={}",
        p.merchant_id, p.set_id, p.index, p.commitments_hex.len()
    );
    let credential = p.credential.to_credential()?;
    let crs_bytes = hex::decode(&p.crs_hex).map_err(|e| format!("crs hex: {e}"))?;
    let crs = Crs::from_bytes(&crs_bytes).map_err(|e| e.to_string())?;
    let commitments: Vec<Commitment> = p
        .commitments_hex
        .iter()
        .map(|h| hex_to_33(h).map(Commitment))
        .collect::<Result<_, _>>()?;

    // Reconstruct beneficiary with the required state
    let mut ben = Beneficiary {
        credential,
        set_id: Some({
            let mut bytes = [0u8; 32];
            bytes[..8].copy_from_slice(&p.set_id.to_le_bytes());
            bytes
        }),
        index: Some(p.index),
        anonymity_set: Some(commitments),
        registrations: std::collections::HashMap::new(),
    };

    eprintln!("[core] generating ZK proof for merchant_id={}...", p.merchant_id);
    let start = std::time::Instant::now();
    let reg = ben
        .create_payment_registration(&crs, p.merchant_id)
        .map_err(|e| e.to_string())?;
    let proof_bytes = serialize_payment_identity_registration_proof(&reg.proof);
    let elapsed = start.elapsed();

    eprintln!(
        "[core] ZK proof generated in {:.1}ms: pseudonym={}..., nullifier={}..., proof={} bytes",
        elapsed.as_secs_f64() * 1000.0,
        hex::encode(&reg.pseudonym[..8]),
        hex::encode(&reg.public_nullifier[..8]),
        proof_bytes.len()
    );

    let resp = CreatePaymentIdResponse {
        pseudonym: hex::encode(reg.pseudonym),
        nullifier: hex::encode(reg.public_nullifier),
        proof_hex: hex::encode(proof_bytes),
        friendly_name: reg.friendly_name,
        service_index: reg.service_index,
        set_id: u64::from_le_bytes(reg.set_id[..8].try_into().unwrap()),
    };
    serde_json::to_value(resp).map_err(|e| e.to_string())
}

fn handle_create_payment_request(params: serde_json::Value) -> Result<serde_json::Value, String> {
    let p: CreatePaymentRequestParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    eprintln!(
        "[core] create-payment-request: merchant='{}', amount={} sats",
        p.merchant_name, p.amount
    );
    let r_bytes = hex_to_32(&p.credential_r_hex)?;
    let child_randomness = ChildRandomness(r_bytes);
    let name = Name::new(&p.merchant_name);
    let g_bytes = hex_to_33(&p.crs_g_hex)?;
    let g = veiled::core::utils::point_from_bytes(&g_bytes)
        .ok_or("invalid generator point")?;

    let req = create_payment_request(&child_randomness, &name, &g, p.amount);

    eprintln!(
        "[core] payment request: pseudonym={}..., proof_r={}..., {} sats",
        hex::encode(&req.pseudonym[..8]),
        hex::encode(&req.proof.r[..8]),
        req.amount
    );

    let resp = CreatePaymentRequestResponse {
        pseudonym: hex::encode(req.pseudonym),
        proof_r: hex::encode(req.proof.r),
        proof_s: hex::encode(req.proof.s),
        amount: req.amount,
    };
    serde_json::to_value(resp).map_err(|e| e.to_string())
}

// ── Command dispatch ──

fn dispatch(cmd: Command) -> Result<serde_json::Value, String> {
    let cmd_name = cmd.command.clone();
    let start = std::time::Instant::now();
    eprintln!("[core] ── {} ──", cmd_name);
    let result = match cmd.command.as_str() {
        "create-credential" => handle_create_credential(cmd.params),
        "register-locally" => handle_register_locally(cmd.params),
        "create-payment-id" => handle_create_payment_id(cmd.params),
        "create-payment-request" => handle_create_payment_request(cmd.params),
        "ping" => Ok(serde_json::json!({"status": "ok"})),
        other => Err(format!("unknown command: {other}")),
    };
    let elapsed = start.elapsed();
    match &result {
        Ok(_) => eprintln!("[core] ── {} OK ({:.1}ms) ──", cmd_name, elapsed.as_secs_f64() * 1000.0),
        Err(e) => eprintln!("[core] ── {} FAILED ({:.1}ms): {} ──", cmd_name, elapsed.as_secs_f64() * 1000.0, e),
    }
    result
}

// ── Main ──

fn main() {
    use std::io::BufRead;

    let daemon = std::env::args().any(|a| a == "--daemon");

    if daemon {
        eprintln!("[core] daemon started (pid: {})", std::process::id());
        use std::io::Write;
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = stdin.lock();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[core] stdin read error: {e}");
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match serde_json::from_str::<Command>(&line) {
                    Ok(cmd) => match dispatch(cmd) {
                        Ok(val) => serde_json::to_string(&val).unwrap(),
                        Err(e) => serde_json::to_string(&ErrorResponse { error: e }).unwrap(),
                    },
                    Err(e) => serde_json::to_string(&ErrorResponse {
                        error: format!("invalid JSON: {e}"),
                    })
                    .unwrap(),
                }
            }));
            let output = match output {
                Ok(s) => s,
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    eprintln!("[core] PANIC caught: {}", msg);
                    serde_json::to_string(&ErrorResponse {
                        error: format!("internal panic: {msg}"),
                    })
                    .unwrap()
                }
            };
            if let Err(e) = writeln!(stdout.lock(), "{output}") {
                eprintln!("[core] stdout write error (client gone?): {e}");
                break;
            }
        }
        eprintln!("[core] daemon shutting down");
    } else {
        // Single-command mode (backwards compatible)
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .expect("failed to read stdin");

        let cmd: Command = match serde_json::from_str(&input) {
            Ok(c) => c,
            Err(e) => {
                let err = ErrorResponse {
                    error: format!("invalid JSON: {e}"),
                };
                println!("{}", serde_json::to_string(&err).unwrap());
                std::process::exit(1);
            }
        };

        match dispatch(cmd) {
            Ok(val) => {
                println!("{}", serde_json::to_string(&val).unwrap());
            }
            Err(e) => {
                let err = ErrorResponse { error: e };
                println!("{}", serde_json::to_string(&err).unwrap());
                std::process::exit(1);
            }
        }
    }
}
