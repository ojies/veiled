use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use veiled::registry::pb::registry_client::RegistryClient;
use veiled::registry::pb::registry_server::RegistryServer;
use veiled::registry::pb::{
    BeneficiaryRequest, CreateSetRequest, FinalizeSetRequest, GetAnonymitySetRequest,
    GetCrsRequest, GetMerchantsRequest, GetRegistryAddressRequest, MerchantRequest,
};
use veiled::registry::service::RegistryService;
use veiled::registry::store::{FeeConfig, RegistryStore};

#[tokio::test]
async fn test_registry_integration() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50052".parse()?;
    let store = Arc::new(Mutex::new(RegistryStore::new(None, FeeConfig::default(), None)));
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
        email: "merchant@example.com".to_string(),
        phone: "+987654321".to_string(),
        funding_txid: vec![0xaa; 32],
        funding_vout: 0,
    };
    client.register_merchant(merchant_req).await?;

    // 1.1 Duplicate merchant should fail
    let dup_merchant_req = MerchantRequest {
        name: "Test Merchant".to_string(),
        origin: "http://test.com".to_string(),
        email: "dup@example.com".to_string(),
        phone: "0".to_string(),
        funding_txid: vec![0xaa; 32],
        funding_vout: 0,
    };
    let dup_err = client.register_merchant(dup_merchant_req).await;
    assert!(dup_err.is_err());

    // 1.2 Query merchants
    let merchants_res = client
        .get_merchants(GetMerchantsRequest {})
        .await?
        .into_inner();
    assert_eq!(merchants_res.merchants.len(), 1);
    assert_eq!(merchants_res.merchants[0].name, "Test Merchant");
    assert_eq!(merchants_res.merchants[0].credential_generator.len(), 33);

    // 1.3 GetRegistryAddress with set_id=0 (global wallet address for merchants)
    let global_addr_res = client
        .get_registry_address(GetRegistryAddressRequest { set_id: 0 })
        .await?
        .into_inner();
    assert!(global_addr_res.address.starts_with("bcrt1p"), "expected bcrt1p address, got: {}", global_addr_res.address);
    assert_eq!(global_addr_res.internal_key.len(), 32);

    // 2. Create Set (Phase 0: CRS setup)
    let create_set_req = CreateSetRequest {
        set_id: 1,
        merchant_names: vec!["Test Merchant".to_string()],
        beneficiary_capacity: 2,
        sats_per_user: 2_000,
    };
    client.create_set(create_set_req).await?;

    // 2.1 Duplicate set should fail
    let dup_set_req = CreateSetRequest {
        set_id: 1,
        merchant_names: vec!["Test Merchant".to_string()],
        beneficiary_capacity: 2,
        sats_per_user: 2_000,
    };
    assert!(client.create_set(dup_set_req).await.is_err());

    // 2.2 Set with unknown merchant should fail
    let bad_set_req = CreateSetRequest {
        set_id: 99,
        merchant_names: vec!["Unknown Merchant".to_string()],
        beneficiary_capacity: 2,
        sats_per_user: 2_000,
    };
    assert!(client.create_set(bad_set_req).await.is_err());

    // 2.3 GetRegistryAddress — returns P2TR address for the set
    let addr_res = client
        .get_registry_address(GetRegistryAddressRequest { set_id: 1 })
        .await?
        .into_inner();
    assert!(addr_res.address.starts_with("bcrt1p"), "expected bcrt1p address, got: {}", addr_res.address);
    assert_eq!(addr_res.internal_key.len(), 32);

    // 2.4 GetRegistryAddress for unknown set should fail
    assert!(client
        .get_registry_address(GetRegistryAddressRequest { set_id: 99 })
        .await
        .is_err());

    // 3. Get CRS (Phase 1: beneficiaries need this)
    let crs_res = client
        .get_crs(GetCrsRequest { set_id: 1 })
        .await?
        .into_inner();
    assert!(!crs_res.crs_bytes.is_empty());

    // 3.1 CRS for unknown set should fail
    assert!(client
        .get_crs(GetCrsRequest { set_id: 99 })
        .await
        .is_err());

    // 4. Register Beneficiaries (Phase 2)
    // No RPC client in test, so payment verification is skipped
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let sk1 = bitcoin::secp256k1::SecretKey::from_slice(&[0x01; 32])?;
    let pk1 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk1);
    let phi1 = pk1.serialize().to_vec();

    let ben_res = client
        .register_beneficiary(BeneficiaryRequest {
            set_id: 1,
            phi: phi1.clone(),
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            phone: "+123456789".to_string(),
            funding_txid: vec![0xaa; 32],
            funding_vout: 0,
        })
        .await?
        .into_inner();
    assert_eq!(ben_res.index, 0);

    // 4.1 Duplicate beneficiary should fail
    assert!(client
        .register_beneficiary(BeneficiaryRequest {
            set_id: 1,
            phi: phi1,
            name: "AliceDup".to_string(),
            email: "".to_string(),
            phone: "".to_string(),
            funding_txid: vec![0xaa; 32],
            funding_vout: 0,
        })
        .await
        .is_err());

    // 4.2 Register second beneficiary
    let sk2 = bitcoin::secp256k1::SecretKey::from_slice(&[0x02; 32])?;
    let pk2 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk2);
    let phi2 = pk2.serialize().to_vec();

    client
        .register_beneficiary(BeneficiaryRequest {
            set_id: 1,
            phi: phi2,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            phone: "+987654321".to_string(),
            funding_txid: vec![0xbb; 32],
            funding_vout: 0,
        })
        .await?;

    // 4.3 Query anonymity set
    let anon_set = client
        .get_anonymity_set(GetAnonymitySetRequest { set_id: 1 })
        .await?
        .into_inner();
    assert_eq!(anon_set.count, 2);
    assert_eq!(anon_set.capacity, 2);
    assert!(!anon_set.finalized);
    assert_eq!(anon_set.commitments.len(), 2);

    // 5. Finalize Set (creates Taproot commitment)
    let finalize_res = client
        .finalize_set(FinalizeSetRequest { set_id: 1 })
        .await?
        .into_inner();
    assert!(finalize_res.message.contains("finalized"));

    // 5.1 Verify set is now finalized
    let anon_set_final = client
        .get_anonymity_set(GetAnonymitySetRequest { set_id: 1 })
        .await?
        .into_inner();
    assert!(anon_set_final.finalized);

    // 5.2 Finalize non-existent set should fail
    assert!(client
        .finalize_set(FinalizeSetRequest { set_id: 99 })
        .await
        .is_err());

    // 6. SubscribeSetFinalization — already finalized set returns immediately
    let response = client
        .subscribe_set_finalization(GetAnonymitySetRequest { set_id: 1 })
        .await?;
    let mut stream = response.into_inner();
    let msg = stream.message().await?.expect("should receive finalized set");
    assert!(msg.finalized);
    assert_eq!(msg.count, 2);
    assert_eq!(msg.capacity, 2);
    assert_eq!(msg.commitments.len(), 2);

    // 6.1 SubscribeSetFinalization — wait for finalization of a new set
    client
        .create_set(CreateSetRequest {
            set_id: 2,
            merchant_names: vec!["Test Merchant".to_string()],
            beneficiary_capacity: 2,
            sats_per_user: 2_000,
        })
        .await?;

    // Subscribe before finalization — should block until finalized
    let mut sub_client = RegistryClient::connect("http://[::1]:50052").await?;
    let sub_handle = tokio::spawn(async move {
        let response = sub_client
            .subscribe_set_finalization(GetAnonymitySetRequest { set_id: 2 })
            .await
            .unwrap();
        let mut stream = response.into_inner();
        stream.message().await.unwrap().unwrap()
    });

    // Small delay to ensure subscriber is connected before we finalize
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Register beneficiaries and finalize while subscriber waits
    let sk3 = bitcoin::secp256k1::SecretKey::from_slice(&[0x03; 32])?;
    let pk3 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk3);
    client
        .register_beneficiary(BeneficiaryRequest {
            set_id: 2,
            phi: pk3.serialize().to_vec(),
            name: "Carol".to_string(),
            email: "".to_string(),
            phone: "".to_string(),
            funding_txid: vec![0xcc; 32],
            funding_vout: 0,
        })
        .await?;

    let sk4 = bitcoin::secp256k1::SecretKey::from_slice(&[0x04; 32])?;
    let pk4 = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk4);
    client
        .register_beneficiary(BeneficiaryRequest {
            set_id: 2,
            phi: pk4.serialize().to_vec(),
            name: "Dave".to_string(),
            email: "".to_string(),
            phone: "".to_string(),
            funding_txid: vec![0xdd; 32],
            funding_vout: 0,
        })
        .await?;

    client
        .finalize_set(FinalizeSetRequest { set_id: 2 })
        .await?;

    // Subscriber should receive the finalized set
    let sub_msg = tokio::time::timeout(std::time::Duration::from_secs(5), sub_handle)
        .await
        .expect("subscription should complete within 5s")
        .expect("subscription task should not panic");
    assert!(sub_msg.finalized);
    assert_eq!(sub_msg.count, 2);
    assert_eq!(sub_msg.commitments.len(), 2);

    // 6.2 Subscribe to non-existent set should fail
    assert!(client
        .subscribe_set_finalization(GetAnonymitySetRequest { set_id: 99 })
        .await
        .is_err());

    server_handle.abort();
    Ok(())
}
