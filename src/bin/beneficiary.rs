use veiled::core::{Beneficiary, Crs, Merchant};

fn setup_crs() -> Crs {
    let merchants = vec![
        Merchant::new("merchant_1", "https://merchant_1"),
        Merchant::new("merchant_2", "https://merchant_2"),
        Merchant::new("merchant_3", "https://merchant_3"),
    ];

    Crs::setup(merchants, 1024)
}

fn main() {
    println!("Beneficiary binary started.");

    // Setup an environment to simulate registry download
    let crs = setup_crs();

    // Instantiate a core Beneficiary directly
    let name = "alice";
    let beneficiary = Beneficiary::new(&crs, name);
    println!("Successfully instantiated new Beneficiary: {}", name);

    let commitment = beneficiary.credential.phi;
    println!("Commitment generated: {:?}", commitment.0);
}
