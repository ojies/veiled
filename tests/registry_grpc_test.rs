use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::pb::{BeneficiaryRequest, FinalizeSetRequest, MerchantRequest};
use veiled::registry::service::RegistryService;
use veiled::registry::store::RegistryStore;

#[tokio::test]
async fn test_registry_integration() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50052".parse()?;
    let store = Arc::new(Mutex::new(RegistryStore::new(2, 1)));
    let service = RegistryService::new(store);

    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(RegistryServer::new(service))
            .serve(addr)
            .await
            .unwrap();
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = RegistryClient::connect("http://[::1]:50052").await?;

    // 1. Register Merchant
    let merchant_req = MerchantRequest {
        name: "Test Merchant".to_string(),
        origin: "http://test.com".to_string(),
        credential_generator: vec![0x02; 33],
        email: "merchant@example.com".to_string(),
        phone: "+987654321".to_string(),
        address: "456 Crypto Ave".to_string(),
    };
    let merchant_res = client.register_merchant(merchant_req).await?.into_inner();
    assert!(merchant_res.success);

    // 1.5 Validate Merchant Selection
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let sk1 = bitcoin::secp256k1::SecretKey::from_slice(&[0x01; 32])?;
    let pk1 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk1);
    let phi = pk1.serialize().to_vec();

    let bad_merchant_req = BeneficiaryRequest {
        set_id: 1,
        phi: phi.clone(),
        name: "InvalidMerchant".to_string(),
        email: "fail@test.com".to_string(),
        phone: "0".to_string(),
        address: "nowhere".to_string(),
        merchant_names: vec!["Non Existent".to_string()],
    };
    let bad_res = client
        .register_beneficiary(bad_merchant_req)
        .await?
        .into_inner();
    assert!(!bad_res.success);
    assert!(bad_res.message.contains("not found in registration pool"));

    let wrong_count_req = BeneficiaryRequest {
        set_id: 1,
        phi: vec![0x03; 33],
        name: "WrongCount".to_string(),
        email: "fail@test.com".to_string(),
        phone: "0".to_string(),
        address: "nowhere".to_string(),
        merchant_names: vec![],
    };
    let count_res = client
        .register_beneficiary(wrong_count_req)
        .await?
        .into_inner();
    assert!(!count_res.success);
    assert!(count_res.message.contains("requires exactly 1 merchants"));

    // 2. Register Beneficiary (Success)
    let beneficiary_req = BeneficiaryRequest {
        set_id: 1,
        phi: phi.clone(),
        name: "AliceBeneficiary".to_string(),
        email: "alice@example.com".to_string(),
        phone: "+123456789".to_string(),
        address: "123 Bitcoin St".to_string(),
        merchant_names: vec!["Test Merchant".to_string()],
    };
    let beneficiary_res = client
        .register_beneficiary(beneficiary_req)
        .await?
        .into_inner();
    assert!(beneficiary_res.success);
    assert_eq!(beneficiary_res.index, 0);

    // 2.5 Register with Inconsistent Merchants
    let inconsistent_req = BeneficiaryRequest {
        set_id: 1,
        phi: vec![0x04; 33],
        name: "Inconsistent".to_string(),
        email: "fail@test.com".to_string(),
        phone: "0".to_string(),
        address: "nowhere".to_string(),
        merchant_names: vec!["Another Merchant".to_string()], // Existing set has "Test Merchant"
    };
    // First we'd need another merchant in the pool for this specific check to trigger correctly if we want to test "exists but different"
    // But since "Another Merchant" doesn't exist, it will trigger the "not found" check first unless it exists.
    // Let's add another merchant to the pool first.
    let merchant_req2 = MerchantRequest {
        name: "Another Merchant".to_string(),
        origin: "http://another.com".to_string(),
        credential_generator: vec![0x03; 33],
        email: "m2@test.com".to_string(),
        phone: "0".to_string(),
        address: "addr".to_string(),
    };
    client.register_merchant(merchant_req2).await?;

    let inconsistent_res = client
        .register_beneficiary(inconsistent_req)
        .await?
        .into_inner();
    assert!(!inconsistent_res.success);
    assert!(inconsistent_res
        .message
        .contains("does not match existing configuration"));

    // 3. Register Duplicate Beneficiary
    let beneficiary_req_dup = BeneficiaryRequest {
        set_id: 1,
        phi: phi,
        name: "AliceDuplicate".to_string(),
        email: "alice@example.com".to_string(),
        phone: "+123456789".to_string(),
        address: "123 Bitcoin St".to_string(),
        merchant_names: vec!["Test Merchant".to_string()],
    };
    let beneficiary_res_dup = client
        .register_beneficiary(beneficiary_req_dup)
        .await?
        .into_inner();
    assert!(!beneficiary_res_dup.success);
    assert!(beneficiary_res_dup.message.contains("already registered"));

    // 3.5 Register Second Beneficiary
    let sk2 = bitcoin::secp256k1::SecretKey::from_slice(&[0x02; 32])?;
    let pk2 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk2);
    let phi2 = pk2.serialize().to_vec();

    let beneficiary_req2 = BeneficiaryRequest {
        set_id: 1,
        phi: phi2.clone(),
        name: "BobBeneficiary".to_string(),
        email: "bob@example.com".to_string(),
        phone: "+987654321".to_string(),
        address: "456 Ethereum St".to_string(),
        merchant_names: vec!["Test Merchant".to_string()],
    };
    let beneficiary_res2 = client
        .register_beneficiary(beneficiary_req2)
        .await?
        .into_inner();
    assert!(beneficiary_res2.success);

    // 4. Finalize Set
    let finalize_req = FinalizeSetRequest { set_id: 1 };
    let finalize_res = client.finalize_set(finalize_req).await?.into_inner();
    assert!(
        finalize_res.success,
        "Finalization failed: {}",
        finalize_res.message
    );
    assert!(finalize_res.message.contains("finalized"));

    // 5. Finalize non-existent set
    let finalize_req_fail = FinalizeSetRequest { set_id: 99 };
    let finalize_res_fail = client.finalize_set(finalize_req_fail).await?.into_inner();
    assert!(!finalize_res_fail.success);

    server_handle.abort();
    Ok(())
}
