use veiled::core::Merchant;
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::MerchantRequest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Merchant binary started.");

    let name = "merchant_1";
    let origin = "https://merchant_1.com";
    let email = "merchant_1@example.com";
    let phone = "+1234567890";
    let address = "123 Merchant St";

    // 1. Instantiate the core Merchant logic
    let merchant = Merchant::new(name, origin);

    println!(
        "Successfully instantiated new Merchant: {}",
        merchant.name.as_str()
    );
    println!("Credential generator: {:?}", merchant.credential_generator);

    // 2. Connect to the Registry gRPC server
    let mut client = RegistryClient::connect("http://[::1]:50051").await?;

    // 3. Create the registration request
    let request = tonic::Request::new(MerchantRequest {
        name: merchant.name.as_str().to_string(),
        origin: merchant.origin.clone(),
        credential_generator: merchant.credential_generator.to_vec(),
        email: email.to_string(),
        phone: phone.to_string(),
        address: address.to_string(),
    });

    println!("Sending Merchant registration request to registry...");

    // 4. Send the request
    let response = client.register_merchant(request).await?;

    println!("Response from registry: {:?}", response.into_inner());

    Ok(())
}
