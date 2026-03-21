fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Proto compilation ──
    tonic_build::configure()
        .build_server(true)
        .compile_protos(
            &["proto/registry.proto"],
            &["proto"],
        )?;

    // ── Generate M constant ──
    // Priority: VEILED_M env var > Cargo feature > default (3)
    let m: u32 = if let Ok(val) = std::env::var("VEILED_M") {
        val.parse().expect("VEILED_M must be an integer (2-8)")
    } else if cfg!(feature = "m2") {
        2
    } else if cfg!(feature = "m3") {
        3
    } else if cfg!(feature = "m4") {
        4
    } else if cfg!(feature = "m5") {
        5
    } else {
        3 // default
    };

    assert!(
        (2..=8).contains(&m),
        "M must be between 2 and 8, got {}",
        m
    );

    // Write generated constant
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = std::path::Path::new(&out_dir).join("constants.rs");
    std::fs::write(
        &dest,
        format!(
            "/// Number of bits for anonymity set indexing (compile-time).\n\
             /// Set via Cargo feature (m2/m3/m4/m5) or VEILED_M env var.\n\
             pub const M: usize = {m};\n"
        ),
    )?;

    // Re-run if VEILED_M changes
    println!("cargo:rerun-if-env-changed=VEILED_M");

    Ok(())
}
