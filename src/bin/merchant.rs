use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use veiled::client;

#[derive(Parser)]
#[command(name = "merchant", about = "Veiled Merchant: registers with registry and waits for set finalization")]
struct Args {
    /// Merchant name
    #[arg(short, long)]
    name: String,

    /// Merchant origin URL
    #[arg(short, long)]
    origin: String,

    /// Registry gRPC server address
    #[arg(short, long, default_value = "http://[::1]:50051")]
    registry_server: String,

    /// Set ID to subscribe to (32-byte hex)
    #[arg(short, long)]
    set_id: String,

    /// Funding transaction ID (hex-encoded, 32 bytes) proving payment to registry
    #[arg(long)]
    funding_txid: Option<String>,

    /// Funding output index within the payment transaction
    #[arg(long, default_value = "0")]
    funding_vout: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("merchant=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    let set_id_bytes = hex::decode(&args.set_id)
        .map_err(|e| format!("Invalid set_id hex: {}", e))?;
    if set_id_bytes.len() != 32 {
        return Err("set_id must be 32 bytes (64 hex chars)".into());
    }

    // ── Step 1: Connect ──────────────────────────────────────────────────────
    info!("Connecting to registry at {}", args.registry_server);
    let mut client = client::connect(&args.registry_server).await?;

    // ── Step 2: Query fees and registry address ──────────────────────────────
    let (_, merchant_fee) = client::get_fees(&mut client).await?;
    let (registry_address, _) =
        client::get_registry_address(&mut client, &[0u8; 32]).await?;
    info!("Required merchant fee: {} sats (pay to {})", merchant_fee, registry_address);

    // ── Step 3: Parse funding outpoint ───────────────────────────────────────
    let funding_txid = match &args.funding_txid {
        Some(hex) => {
            let bytes = hex::decode(hex)
                .map_err(|e| format!("Invalid funding_txid hex: {}", e))?;
            if bytes.len() != 32 {
                return Err("funding_txid must be 32 bytes (64 hex chars)".into());
            }
            bytes
        }
        None => {
            eprintln!(
                "ERROR: No funding transaction provided.\n\
                 Pay {} sats to {} and re-run with:\n  \
                 --funding-txid <txid_hex> --funding-vout <vout>",
                merchant_fee, registry_address
            );
            std::process::exit(1);
        }
    };

    // ── Step 4: Register with registry ──────────────────────────────────────
    let message = client::register_merchant(
        &mut client,
        &args.name,
        &args.origin,
        "",
        "",
        funding_txid,
        args.funding_vout,
    )
    .await?;
    info!("Registered with registry: {}", message);

    // ── Step 5: Wait for finalized anonymity set ─────────────────────────────
    info!("Subscribing to set {} finalization...", args.set_id);
    let anonymity_set = client::wait_for_finalization(&mut client, &set_id_bytes).await?;
    info!("Set {} finalized: {} members", args.set_id, anonymity_set.len());

    // ── Step 6: Fetch CRS ────────────────────────────────────────────────────
    let crs = client::get_crs(&mut client, &set_id_bytes).await?;
    info!(
        "Merchant '{}' ready: CRS loaded ({} merchants), anonymity set has {} members",
        args.name,
        crs.num_merchants(),
        anonymity_set.len()
    );

    Ok(())
}
