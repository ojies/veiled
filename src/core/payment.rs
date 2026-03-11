//! Payment address derivation from public nullifiers (Phase 5).
//!
//! The public nullifier `nul_l = s_l · g` is a valid 33-byte compressed
//! secp256k1 point. This module converts it to a Bitcoin P2TR (Taproot)
//! address so Alice can send money to Bob after verifying his identity.
//!
//! Flow:
//! ```text
//! 1. Bob proves membership → Alice verifies proof
//! 2. Alice extracts nul_l from the proof
//! 3. nullifier_to_address(nul_l) → bc1p...
//! 4. Alice sends BTC to that address
//! ```

use bitcoin::secp256k1;
use bitcoin::{Address, Network};

/// Convert a 33-byte compressed public nullifier to a P2TR (Taproot) address.
///
/// The nullifier `nul_l = s_l · g` is a valid secp256k1 point. We extract
/// the x-only public key (BIP-340) and create a Taproot address from it.
///
/// Returns `bc1p...` on mainnet, `tb1p...` on testnet/signet.
pub fn nullifier_to_address(
    public_nullifier: &[u8; 33],
    network: Network,
) -> Result<Address, String> {
    // Parse the compressed point and extract x-only key (drop sign byte).
    let pk = secp256k1::PublicKey::from_slice(public_nullifier)
        .map_err(|e| format!("invalid public nullifier point: {e}"))?;
    let (xonly, _parity) = pk.x_only_public_key();

    // Create P2TR address with key-path only (no script tree).
    let secp = secp256k1::Secp256k1::new();
    Ok(Address::p2tr(&secp, xonly, None, network))
}

/// Convert a 33-byte compressed pseudonym to a P2TR address.
///
/// The pseudonym `ϕ = csk_l · g` is also a valid secp256k1 point.
pub fn pseudonym_to_address(
    pseudonym: &[u8; 33],
    network: Network,
) -> Result<Address, String> {
    let pk = secp256k1::PublicKey::from_slice(pseudonym)
        .map_err(|e| format!("invalid pseudonym point: {e}"))?;
    let (xonly, _parity) = pk.x_only_public_key();

    let secp = secp256k1::Secp256k1::new();
    Ok(Address::p2tr(&secp, xonly, None, network))
}

/// Verify that a claimed friendly name matches the name_scalar in a proof.
///
/// Bob reveals his friendly_name alongside the proof. Alice checks:
/// ```text
/// SHA256(friendly_name) == proof.name_scalar
/// ```
///
/// This is secure because `name_scalar` is bound to the proof via
/// Fiat-Shamir — if the prover embeds a fake name_scalar, the challenge
/// changes and the proof fails.
pub fn verify_name_revelation(
    proof_name_scalar: &[u8; 32],
    claimed_name: &str,
) -> bool {
    use sha2::{Digest, Sha256};
    let computed: [u8; 32] = Sha256::digest(claimed_name.as_bytes()).into();
    computed == *proof_name_scalar
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::crs::{Crs, User};
    use crate::core::credential::MasterCredential;
    use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret, Name};

    fn make_crs(n: usize) -> Crs {
        let providers: Vec<User> = (0..n)
            .map(|i| User {
                name: Name::new(&format!("user-{i}")),
                credential_generator: [0x02; 33],
                origin: format!("https://user-{i}"),
            })
            .collect();
        Crs::setup(providers)
    }

    fn make_credential(crs: &Crs, seed: u8) -> MasterCredential {
        let sk = MasterSecret([seed; 32]);
        let r = ChildRandomness([seed.wrapping_add(1); 32]);
        let k = BlindingKey([seed.wrapping_add(2); 32]);
        let name = FriendlyName::new(format!("user-{seed:02x}"));
        MasterCredential::create(crs, sk, r, k, name)
    }

    #[test]
    fn nullifier_to_mainnet_address() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0xAA);
        let pub_nul = cred.public_nullifier(&crs, 1);

        let addr = nullifier_to_address(&pub_nul, Network::Bitcoin).unwrap();
        let addr_str = addr.to_string();
        assert!(addr_str.starts_with("bc1p"), "expected bc1p..., got {addr_str}");
    }

    #[test]
    fn nullifier_to_testnet_address() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0xBB);
        let pub_nul = cred.public_nullifier(&crs, 2);

        let addr = nullifier_to_address(&pub_nul, Network::Testnet).unwrap();
        let addr_str = addr.to_string();
        assert!(addr_str.starts_with("tb1p"), "expected tb1p..., got {addr_str}");
    }

    #[test]
    fn different_services_produce_different_addresses() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0xCC);

        let addr1 = nullifier_to_address(&cred.public_nullifier(&crs, 1), Network::Bitcoin).unwrap();
        let addr2 = nullifier_to_address(&cred.public_nullifier(&crs, 2), Network::Bitcoin).unwrap();
        let addr3 = nullifier_to_address(&cred.public_nullifier(&crs, 3), Network::Bitcoin).unwrap();

        assert_ne!(addr1.to_string(), addr2.to_string());
        assert_ne!(addr2.to_string(), addr3.to_string());
        assert_ne!(addr1.to_string(), addr3.to_string());
    }

    #[test]
    fn pseudonym_to_address_works() {
        let crs = make_crs(3);
        let cred = make_credential(&crs, 0xDD);

        use crate::core::child_credential::derive_pseudonym;
        let pseudo = derive_pseudonym(&cred.r, &crs.providers[0].name, &crs.g);

        let addr = pseudonym_to_address(&pseudo, Network::Bitcoin).unwrap();
        assert!(addr.to_string().starts_with("bc1p"));
    }

    #[test]
    fn verify_name_revelation_correct_name() {
        let name = "bob";
        use sha2::{Digest, Sha256};
        let scalar: [u8; 32] = Sha256::digest(name.as_bytes()).into();
        assert!(verify_name_revelation(&scalar, "bob"));
    }

    #[test]
    fn verify_name_revelation_wrong_name() {
        let name = "bob";
        use sha2::{Digest, Sha256};
        let scalar: [u8; 32] = Sha256::digest(name.as_bytes()).into();
        assert!(!verify_name_revelation(&scalar, "alice"));
    }

    #[test]
    fn invalid_point_returns_error() {
        let bad = [0u8; 33]; // not a valid point
        assert!(nullifier_to_address(&bad, Network::Bitcoin).is_err());
    }
}
