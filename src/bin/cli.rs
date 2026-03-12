use clap::{Parser, Subcommand};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use veiled::core::{BlindingKey, Commitment, Name, PublicKey, commit};

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "veiled", about = "Anonymous credential registry client")]
struct Cli {
    /// Registry server URL (e.g. http://localhost:3000)
    #[arg(long, global = true, default_value = "http://localhost:7271")]
    server: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a random 32-byte public key and blinding key.
    GenerateKey,

    /// Register a (pub_key, name) identity with the registry.
    Register {
        /// Hex-encoded 32-byte public key.
        #[arg(long)]
        pub_key: String,

        /// Your chosen name / handle.
        #[arg(long)]
        name: String,

        /// Hex-encoded 32-byte blinding key (random if omitted).
        #[arg(long)]
        blinding: Option<String>,
    },

    /// Derive credentials locally from (pub_key, name, blinding).
    ///
    /// Nothing is sent to the server.  Use the output with `register` when ready.
    Derive {
        /// Hex-encoded 32-byte public key.
        #[arg(long)]
        pub_key: String,

        /// Your chosen name / handle.
        #[arg(long)]
        name: String,

        /// Hex-encoded 32-byte blinding key (random if omitted).
        #[arg(long)]
        blinding: Option<String>,
    },

    /// Check whether a (pub_key, name) identity is registered.
    Has {
        /// Hex-encoded 32-byte public key.
        #[arg(long)]
        pub_key: String,

        /// Name / handle to check.
        #[arg(long)]
        name: String,
    },

    /// List all anonymity sets on the registry.
    Sets,

    /// Show the full contents of a specific anonymity set.
    Set {
        /// Set ID to inspect.
        #[arg(long)]
        id: u64,
    },

    /// Save a public key and blinding key to a JSON keyfile.
    SaveKey {
        /// Hex-encoded 32-byte public key.
        #[arg(long)]
        pub_key: String,

        /// Hex-encoded 32-byte blinding key.
        #[arg(long)]
        blinding: String,

        /// Path to write the keyfile (default: veiled-keys.json).
        #[arg(long, default_value = "veiled-keys.json")]
        out: String,
    },

    /// Load a public key and blinding key from a JSON keyfile.
    LoadKey {
        /// Path to the keyfile (default: veiled-keys.json).
        #[arg(long, default_value = "veiled-keys.json")]
        file: String,
    },

    /// Generate a one-out-of-many membership proof (local computation).
    ///
    /// Fetches the anonymity set from the server, then generates a ZK proof
    /// that you know an opening for one of its commitments — without revealing
    /// which one.  The proof is printed as hex and can be submitted to a future
    /// `/api/v1/verify` endpoint.
    Prove {
        /// Hex-encoded 32-byte public key.
        #[arg(long)]
        pub_key: String,

        /// Your chosen name / handle.
        #[arg(long)]
        name: String,

        /// Hex-encoded 32-byte blinding key used during registration.
        #[arg(long)]
        blinding: String,

        /// The anonymity set ID returned by `register`.
        #[arg(long)]
        set_id: u64,

        /// Your index within that set (returned by `register`).
        #[arg(long)]
        index: usize,
    },
}

// ── API types (mirrors the server's JSON) ────────────────────────────────────

#[derive(Serialize)]
struct RegisterBody {
    commitment: String,
    nullifier: String,
}

#[derive(Deserialize, Debug)]
struct RegisterResponse {
    set_id: u64,
    index: usize,
}

#[derive(Serialize)]
struct HasBody {
    pub_key: String,
    name: String,
}

#[derive(Deserialize, Debug)]
struct HasResponse {
    present: bool,
    nullifier: String,
}

#[derive(Deserialize, Debug)]
struct SetSummary {
    id: u64,
    size: usize,
    capacity: usize,
    full: bool,
}

#[derive(Deserialize, Debug)]
struct SetDetail {
    id: u64,
    commitments: Vec<String>,
    capacity: usize,
    full: bool,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    error: String,
}

#[derive(Serialize, Deserialize)]
struct Keyfile {
    pub_key: String,
    blinding: String,
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn random_32() -> [u8; 32] {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

fn parse_pub_key(s: &str) -> Result<PublicKey, String> {
    PublicKey::from_hex(s).map_err(|e| format!("invalid pub_key hex: {e}"))
}

fn parse_blinding(s: &str) -> Result<BlindingKey, String> {
    let bytes = hex::decode(s).map_err(|e| format!("invalid blinding hex: {e}"))?;
    let arr: [u8; 32] = bytes.try_into().map_err(|_| "blinding key must be 32 bytes".to_string())?;
    Ok(BlindingKey(arr))
}

fn check_api_error(status: reqwest::StatusCode, body: &str) {
    if !status.is_success() {
        if let Ok(e) = serde_json::from_str::<ApiError>(body) {
            eprintln!("error: {}", e.error);
        } else {
            eprintln!("error: HTTP {status}");
        }
        std::process::exit(1);
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::new();

    match cli.command {
        Command::GenerateKey => {
            let pub_key = random_32();
            let blinding = random_32();
            println!("pub_key:  {}", hex::encode(pub_key));
            println!("blinding: {}", hex::encode(blinding));
            println!();
            println!("Store these securely. The pub_key is your identity; the blinding key");
            println!("is needed to generate commitments.");
        }

        Command::Derive { .. } => {
            // TODO: `compute_nullifier(PublicKey, Name)` was removed during the
            // MembershipProof → PaymentIdentityProof refactor.  This command
            // needs updating to use the new nullifier derivation API.
            eprintln!("error: derive command not yet migrated to new nullifier API");
            std::process::exit(1);
        }

        Command::Register { .. } => {
            // TODO: `compute_nullifier(PublicKey, Name)` was removed during the
            // MembershipProof → PaymentIdentityProof refactor.  This command
            // needs updating to use the new nullifier derivation API.
            eprintln!("error: register command not yet migrated to new nullifier API");
            std::process::exit(1);
        }

        Command::Has { pub_key, name } => {
            let url = format!("{}/api/v1/has", cli.server);
            let resp = client.post(&url)
                .json(&HasBody { pub_key, name })
                .send()
                .unwrap_or_else(|e| { eprintln!("request failed: {e}"); std::process::exit(1); });

            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            check_api_error(status, &body);

            let r: HasResponse = serde_json::from_str(&body).expect("unexpected response format");
            println!("present:  {}", r.present);
            println!("nullifier: {}", r.nullifier);
        }

        Command::Sets => {
            let url = format!("{}/api/v1/sets", cli.server);
            let resp = client.get(&url).send()
                .unwrap_or_else(|e| { eprintln!("request failed: {e}"); std::process::exit(1); });

            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            check_api_error(status, &body);

            let sets: Vec<SetSummary> = serde_json::from_str(&body).expect("unexpected response format");
            if sets.is_empty() {
                println!("no sets");
            } else {
                println!("{:<6} {:<6} {:<10} {}", "id", "size", "capacity", "full");
                for s in &sets {
                    println!("{:<6} {:<6} {:<10} {}", s.id, s.size, s.capacity, s.full);
                }
            }
        }

        Command::Set { id } => {
            let url = format!("{}/api/v1/sets/{id}", cli.server);
            let resp = client.get(&url).send()
                .unwrap_or_else(|e| { eprintln!("request failed: {e}"); std::process::exit(1); });

            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            check_api_error(status, &body);

            let s: SetDetail = serde_json::from_str(&body).expect("unexpected response format");
            println!("set {}: {}/{} commitments (full={})", s.id, s.commitments.len(), s.capacity, s.full);
            for (i, c) in s.commitments.iter().enumerate() {
                println!("  [{i}] {c}");
            }
        }

        Command::SaveKey { pub_key, blinding, out } => {
            // Validate both keys before writing.
            parse_pub_key(&pub_key).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1); });
            parse_blinding(&blinding).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1); });

            let kf = Keyfile { pub_key, blinding };
            let json = serde_json::to_string_pretty(&kf).expect("serialization failed");
            std::fs::write(&out, &json)
                .unwrap_or_else(|e| { eprintln!("failed to write keyfile: {e}"); std::process::exit(1); });
            println!("keys saved to {out}");
        }

        Command::LoadKey { file } => {
            let json = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| { eprintln!("failed to read keyfile: {e}"); std::process::exit(1); });
            let kf: Keyfile = serde_json::from_str(&json)
                .unwrap_or_else(|e| { eprintln!("invalid keyfile: {e}"); std::process::exit(1); });
            // Validate on load.
            parse_pub_key(&kf.pub_key).unwrap_or_else(|e| { eprintln!("keyfile pub_key invalid: {e}"); std::process::exit(1); });
            parse_blinding(&kf.blinding).unwrap_or_else(|e| { eprintln!("keyfile blinding invalid: {e}"); std::process::exit(1); });
            println!("pub_key:  {}", kf.pub_key);
            println!("blinding: {}", kf.blinding);
        }

        Command::Prove { .. } => {
            // TODO: `prove_membership` was removed during the refactor to
            // PaymentIdentityProof.  This command needs updating.
            eprintln!("error: prove command not yet migrated to PaymentIdentityProof");
            std::process::exit(1);
        }
    }
}
