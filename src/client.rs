use crate::core::credential::MasterCredential;
use crate::core::crs::Crs;
use crate::core::types::{BlindingKey, ChildRandomness, FriendlyName, MasterSecret};

/// Named group member credentials (deterministic from seed + friendly name).
pub fn make_named_credential(crs: &Crs, seed: u8, friendly_name: &str) -> MasterCredential {
    let sk = MasterSecret([seed; 32]);
    let r = ChildRandomness([seed.wrapping_add(1); 32]);
    let k = BlindingKey([seed.wrapping_add(2); 32]);
    MasterCredential::create(crs, sk, r, k, FriendlyName::new(friendly_name))
}

/// Filler credential using index-encoded secret to avoid seed collisions.
pub fn make_filler_credential(crs: &Crs, index: usize) -> MasterCredential {
    let mut sk_bytes = [0u8; 32];
    sk_bytes[0] = (index >> 8) as u8;
    sk_bytes[1] = (index & 0xFF) as u8;
    sk_bytes[31] = 0xFF; // sentinel — avoids collision with named seeds
    let mut r_bytes = sk_bytes;
    r_bytes[31] = 0xFE;
    let mut k_bytes = sk_bytes;
    k_bytes[31] = 0xFD;
    MasterCredential::create(
        crs,
        MasterSecret(sk_bytes),
        ChildRandomness(r_bytes),
        BlindingKey(k_bytes),
        FriendlyName::new(format!("filler-{index}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::crs::Merchant;
    use crate::core::types::Name;

    fn setup_crs() -> Crs {
        let merchants = vec![Merchant {
            name: Name::new("merchant_1"),
            credential_generator: [0x02; 33],
            origin: "https://merchant_1".to_string(),
        }];
        Crs::setup(merchants)
    }

    #[test]
    fn test_make_named_credential() {
        let crs = setup_crs();
        let cred = make_named_credential(&crs, 0xA0, "alice");
        assert_eq!(cred.friendly_name.as_str(), "alice");
        assert_eq!(cred.sk.0, [0xA0; 32]);
    }

    #[test]
    fn test_make_filler_credential() {
        let crs = setup_crs();
        let cred = make_filler_credential(&crs, 42);
        assert_eq!(cred.friendly_name.as_str(), "filler-42");
        assert_eq!(cred.sk.0[1], 42);
        assert_eq!(cred.sk.0[31], 0xFF);
    }
}
