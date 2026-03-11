//! Common Reference String (CRS) for the ASC protocol.
//!
//! Implements `ASCCRS.Setup(λ, L)` from the CRS-ASC specification:
//!
//! 1. Choose group: secp256k1 (implicit via k256 crate).
//! 2. Generate L+1 independent generators via HashToCurve:
//!    - g  = HashToCurve("CRS-ASC-generator-0")
//!    - h_l = HashToCurve("CRS-ASC-generator-{l}") for l = 1..L
//! 3. Register service providers with unique names v_1..v_L.
//!
//! The CRS is published publicly:
//!   crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)

use k256::{
    AffinePoint, ProjectivePoint,
    elliptic_curve::{
        group::GroupEncoding,
        hash2curve::{ExpandMsgXmd, GroupDigest},
        ops::Reduce,
    },
    Scalar, Secp256k1, U256,
};
use sha2::Sha256;

use crate::core::types::{BlindingKey, Commitment, Name, Nullifier};

/// Domain separation tag for CRS generator derivation.
const CRS_DST: &[u8] = b"CRS-ASC-v1";

// ── types ────────────────────────────────────────────────────────────────────

/// A user registered in the CRS.
///
/// Each user has a unique name `v_l` (every user is also a service
/// provider), a credential generator `G_auth_l`, and an origin URL.
#[derive(Debug, Clone)]
pub struct User {
    /// Unique name v_l — every user is also a service provider.
    /// Used as the HKDF salt for nullifier derivation.
    pub name: Name,
    /// Credential generator G_auth_l (compressed secp256k1 point).
    /// Application-specific public key for the service's auth scheme.
    pub credential_generator: [u8; 33],
    /// Origin URL for the service provider.
    pub origin: String,
}

/// The Common Reference String for the ASC protocol.
///
/// Contains all public parameters needed by provers, verifiers, and the
/// identity registry. Generated once during system setup.
///
/// ```text
/// crs = (G, q, g, h_1..h_L, v_1..v_L, G_auth_1..G_auth_L)
/// ```
pub struct Crs {
    /// Security parameter λ (128).
    pub security_param: u32,
    /// Base generator g = HashToCurve("CRS-ASC-generator-0").
    pub g: ProjectivePoint,
    /// Friendly name generator h_name = HashToCurve("CRS-ASC-generator-name").
    /// Used in `Φ = k·g + Σ s_l·h_l + name_scalar·h_name`.
    pub h_name: ProjectivePoint,
    /// Per-service-provider generators h_1..h_L.
    /// `generators[i]` corresponds to provider `providers[i]` (0-indexed internally).
    pub generators: Vec<ProjectivePoint>,
    /// Registered service providers v_1..v_L.
    pub providers: Vec<User>,
    /// Anonymity set size N (1024).
    pub set_size: usize,
}

// ── scalar helper ────────────────────────────────────────────────────────────

fn bytes_to_scalar(b: &[u8; 32]) -> Scalar {
    Scalar::reduce(U256::from_be_slice(b))
}

// ── CRS implementation ──────────────────────────────────────────────────────

impl Crs {
    /// `ASCCRS.Setup(λ, providers)`
    ///
    /// Generates the Common Reference String:
    /// - Step 1: secp256k1 is implicit (k256 crate).
    /// - Step 2: Generate L+1 independent generators via HashToCurve.
    /// - Step 3: Register service providers (already provided as input).
    ///
    /// The generators are derived deterministically from public strings,
    /// guaranteeing nobody knows the discrete log of any generator relative
    /// to any other (NUMS — Nothing Up My Sleeve).
    pub fn setup(providers: Vec<User>) -> Self {
        let l = providers.len();

        // g = HashToCurve("CRS-ASC-generator-0")
        let g = Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[b"CRS-ASC-generator-0"],
            &[CRS_DST],
        )
        .expect("hash_to_curve never fails for secp256k1");

        // h_name = HashToCurve("CRS-ASC-generator-name")
        let h_name = Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
            &[b"CRS-ASC-generator-name"],
            &[CRS_DST],
        )
        .expect("hash_to_curve never fails for secp256k1");

        // h_1..h_L (1-indexed in the spec, 0-indexed in the Vec)
        let generators: Vec<ProjectivePoint> = (1..=l)
            .map(|i| {
                let tag = format!("CRS-ASC-generator-{i}");
                Secp256k1::hash_from_bytes::<ExpandMsgXmd<Sha256>>(
                    &[tag.as_bytes()],
                    &[CRS_DST],
                )
                .expect("hash_to_curve never fails for secp256k1")
            })
            .collect();

        Crs {
            security_param: 128,
            g,
            h_name,
            generators,
            providers,
            set_size: 1024,
        }
    }

    /// Returns the number of registered users L.
    pub fn num_providers(&self) -> usize {
        self.providers.len()
    }

    /// Returns generator h_l (1-indexed, as in the spec).
    ///
    /// # Panics
    /// Panics if `l` is 0 or greater than L.
    pub fn h(&self, l: usize) -> &ProjectivePoint {
        assert!(l >= 1 && l <= self.generators.len(), "generator index out of range");
        &self.generators[l - 1]
    }

    /// Returns the names of all registered users.
    pub fn names(&self) -> Vec<Name> {
        self.providers.iter().map(|p| p.name.clone()).collect()
    }

    /// Compute the multi-value Pedersen commitment (master identity Φ):
    ///
    /// ```text
    /// Φ = k·g + s_1·h_1 + ... + s_L·h_L + name_scalar·h_name
    /// ```
    ///
    /// - `nullifiers`: the L nullifier values (one per service provider, in order).
    /// - `blinding`: the random blinding factor k.
    /// - `name_scalar`: SHA256(friendly_name) — the global name committed inside Φ.
    ///
    /// The friendly name is cryptographically bound into Φ via the dedicated
    /// generator `h_name`. The binding property guarantees it cannot be changed
    /// after commitment.
    pub fn commit_master_identity(
        &self,
        nullifiers: &[Nullifier],
        blinding: &BlindingKey,
        name_scalar: &[u8; 32],
    ) -> Result<Commitment, &'static str> {
        if nullifiers.len() != self.generators.len() {
            return Err("nullifier count must equal number of service providers");
        }

        let r = bytes_to_scalar(&blinding.0);
        let mut point = self.g * r;

        for (i, nul) in nullifiers.iter().enumerate() {
            let v = bytes_to_scalar(&nul.0);
            point += self.generators[i] * v;
        }

        // name_scalar · h_name
        let ns = bytes_to_scalar(name_scalar);
        point += self.h_name * ns;

        Ok(Commitment(point.to_affine().to_bytes().into()))
    }

    /// Serialize the CRS to bytes for distribution / on-chain commitment.
    ///
    /// Format:
    /// - 4 bytes: security_param (u32 BE)
    /// - 4 bytes: L (u32 BE, number of providers)
    /// - 4 bytes: set_size (u32 BE)
    /// - 33 bytes: g (compressed point)
    /// - L × 33 bytes: h_1..h_L (compressed points)
    /// - For each provider:
    ///   - 2 bytes: name length (u16 BE)
    ///   - N bytes: name UTF-8
    ///   - 33 bytes: credential_generator
    ///   - 2 bytes: origin length (u16 BE)
    ///   - N bytes: origin UTF-8
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.extend_from_slice(&self.security_param.to_be_bytes());
        buf.extend_from_slice(&(self.generators.len() as u32).to_be_bytes());
        buf.extend_from_slice(&(self.set_size as u32).to_be_bytes());

        // g
        let g_bytes: [u8; 33] = self.g.to_affine().to_bytes().into();
        buf.extend_from_slice(&g_bytes);

        // h_name
        let h_name_bytes: [u8; 33] = self.h_name.to_affine().to_bytes().into();
        buf.extend_from_slice(&h_name_bytes);

        // h_1..h_L
        for gen in &self.generators {
            let h_bytes: [u8; 33] = gen.to_affine().to_bytes().into();
            buf.extend_from_slice(&h_bytes);
        }

        // providers
        for provider in &self.providers {
            let name_bytes = provider.name.as_str().as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&provider.credential_generator);
            let origin_bytes = provider.origin.as_bytes();
            buf.extend_from_slice(&(origin_bytes.len() as u16).to_be_bytes());
            buf.extend_from_slice(origin_bytes);
        }

        buf
    }

    /// Deserialize a CRS from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 12 {
            return Err("CRS bytes too short for header");
        }

        let mut pos = 0;

        let security_param = u32::from_be_bytes(
            bytes[pos..pos + 4].try_into().map_err(|_| "bad header")?,
        );
        pos += 4;

        let l = u32::from_be_bytes(
            bytes[pos..pos + 4].try_into().map_err(|_| "bad header")?,
        ) as usize;
        pos += 4;

        let set_size = u32::from_be_bytes(
            bytes[pos..pos + 4].try_into().map_err(|_| "bad header")?,
        ) as usize;
        pos += 4;

        // g
        if bytes.len() < pos + 33 {
            return Err("CRS bytes too short for g");
        }
        let g_bytes: [u8; 33] = bytes[pos..pos + 33]
            .try_into()
            .map_err(|_| "bad g bytes")?;
        let g_affine = AffinePoint::from_bytes(&g_bytes.into());
        let g = if g_affine.is_some().into() {
            ProjectivePoint::from(g_affine.unwrap())
        } else {
            return Err("invalid g point");
        };
        pos += 33;

        // h_name
        if bytes.len() < pos + 33 {
            return Err("CRS bytes too short for h_name");
        }
        let h_name_bytes: [u8; 33] = bytes[pos..pos + 33]
            .try_into()
            .map_err(|_| "bad h_name bytes")?;
        let h_name_affine = AffinePoint::from_bytes(&h_name_bytes.into());
        let h_name = if h_name_affine.is_some().into() {
            ProjectivePoint::from(h_name_affine.unwrap())
        } else {
            return Err("invalid h_name point");
        };
        pos += 33;

        // h_1..h_L
        let mut generators = Vec::with_capacity(l);
        for _ in 0..l {
            if bytes.len() < pos + 33 {
                return Err("CRS bytes too short for generators");
            }
            let h_bytes: [u8; 33] = bytes[pos..pos + 33]
                .try_into()
                .map_err(|_| "bad h bytes")?;
            let h_affine = AffinePoint::from_bytes(&h_bytes.into());
            let h = if h_affine.is_some().into() {
                ProjectivePoint::from(h_affine.unwrap())
            } else {
                return Err("invalid h point");
            };
            generators.push(h);
            pos += 33;
        }

        // providers
        let mut providers = Vec::with_capacity(l);
        for _ in 0..l {
            if bytes.len() < pos + 2 {
                return Err("CRS bytes too short for provider name length");
            }
            let name_len = u16::from_be_bytes(
                bytes[pos..pos + 2].try_into().map_err(|_| "bad name len")?,
            ) as usize;
            pos += 2;

            if bytes.len() < pos + name_len {
                return Err("CRS bytes too short for provider name");
            }
            let name_str = std::str::from_utf8(&bytes[pos..pos + name_len])
                .map_err(|_| "invalid UTF-8 in provider name")?;
            let name = Name::try_new(name_str).map_err(|_| "provider name too long")?;
            pos += name_len;

            if bytes.len() < pos + 33 {
                return Err("CRS bytes too short for credential generator");
            }
            let credential_generator: [u8; 33] = bytes[pos..pos + 33]
                .try_into()
                .map_err(|_| "bad credential generator bytes")?;
            pos += 33;

            if bytes.len() < pos + 2 {
                return Err("CRS bytes too short for origin length");
            }
            let origin_len = u16::from_be_bytes(
                bytes[pos..pos + 2].try_into().map_err(|_| "bad origin len")?,
            ) as usize;
            pos += 2;

            if bytes.len() < pos + origin_len {
                return Err("CRS bytes too short for origin");
            }
            let origin = std::str::from_utf8(&bytes[pos..pos + origin_len])
                .map_err(|_| "invalid UTF-8 in origin")?
                .to_string();
            pos += origin_len;

            providers.push(User {
                name,
                credential_generator,
                origin,
            });
        }

        Ok(Crs {
            security_param,
            g,
            h_name,
            generators,
            providers,
            set_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::nullifier_v2::derive_all_nullifiers;
    use crate::core::types::MasterSecret;

    fn make_provider(name: &str) -> User {
        User {
            name: Name::new(name),
            credential_generator: [0x02; 33], // placeholder compressed point
            origin: format!("https://{name}"),
        }
    }

    fn make_providers(count: usize) -> Vec<User> {
        (0..count)
            .map(|i| make_provider(&format!("service-{i}")))
            .collect()
    }

    #[test]
    fn setup_creates_correct_number_of_generators() {
        let providers = make_providers(5);
        let crs = Crs::setup(providers);
        assert_eq!(crs.num_providers(), 5);
        assert_eq!(crs.generators.len(), 5);
    }

    #[test]
    fn generators_are_all_different() {
        let providers = make_providers(10);
        let crs = Crs::setup(providers);

        // g should differ from all h_l
        let g_bytes: [u8; 33] = crs.g.to_affine().to_bytes().into();
        for gen in &crs.generators {
            let h_bytes: [u8; 33] = gen.to_affine().to_bytes().into();
            assert_ne!(g_bytes, h_bytes, "g must differ from all h_l");
        }

        // All h_l must be distinct
        let points: Vec<[u8; 33]> = crs
            .generators
            .iter()
            .map(|p| p.to_affine().to_bytes().into())
            .collect();
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                assert_ne!(points[i], points[j], "h_{} and h_{} must differ", i + 1, j + 1);
            }
        }
    }

    #[test]
    fn setup_is_deterministic() {
        let p1 = make_providers(3);
        let p2 = make_providers(3);
        let crs1 = Crs::setup(p1);
        let crs2 = Crs::setup(p2);

        let g1: [u8; 33] = crs1.g.to_affine().to_bytes().into();
        let g2: [u8; 33] = crs2.g.to_affine().to_bytes().into();
        assert_eq!(g1, g2, "g must be deterministic");

        for i in 0..3 {
            let h1: [u8; 33] = crs1.generators[i].to_affine().to_bytes().into();
            let h2: [u8; 33] = crs2.generators[i].to_affine().to_bytes().into();
            assert_eq!(h1, h2, "h_{} must be deterministic", i + 1);
        }
    }

    #[test]
    fn h_accessor_1_indexed() {
        let crs = Crs::setup(make_providers(3));
        let h1_via_accessor: [u8; 33] = crs.h(1).to_affine().to_bytes().into();
        let h1_direct: [u8; 33] = crs.generators[0].to_affine().to_bytes().into();
        assert_eq!(h1_via_accessor, h1_direct);
    }

    #[test]
    #[should_panic(expected = "generator index out of range")]
    fn h_panics_on_zero_index() {
        let crs = Crs::setup(make_providers(3));
        let _ = crs.h(0);
    }

    const TEST_NAME: [u8; 32] = [0xAA; 32]; // placeholder name scalar for tests

    #[test]
    fn commit_master_identity_deterministic() {
        let crs = Crs::setup(make_providers(3));
        let secret = MasterSecret([0x42u8; 32]);
        let names = crs.names();
        let nullifiers = derive_all_nullifiers(&secret, &names);
        let blinding = BlindingKey([0x07u8; 32]);

        let c1 = crs.commit_master_identity(&nullifiers, &blinding, &TEST_NAME).unwrap();
        let c2 = crs.commit_master_identity(&nullifiers, &blinding, &TEST_NAME).unwrap();
        assert_eq!(c1, c2);
    }

    #[test]
    fn commit_different_blinding_gives_different_commitment() {
        let crs = Crs::setup(make_providers(3));
        let secret = MasterSecret([0x42u8; 32]);
        let names = crs.names();
        let nullifiers = derive_all_nullifiers(&secret, &names);

        let c1 = crs
            .commit_master_identity(&nullifiers, &BlindingKey([0x01u8; 32]), &TEST_NAME)
            .unwrap();
        let c2 = crs
            .commit_master_identity(&nullifiers, &BlindingKey([0x02u8; 32]), &TEST_NAME)
            .unwrap();
        assert_ne!(c1, c2);
    }

    #[test]
    fn commit_different_nullifiers_gives_different_commitment() {
        let crs = Crs::setup(make_providers(3));
        let blinding = BlindingKey([0x07u8; 32]);

        let nuls1 = derive_all_nullifiers(&MasterSecret([0x01u8; 32]), &crs.names());
        let nuls2 = derive_all_nullifiers(&MasterSecret([0x02u8; 32]), &crs.names());

        let c1 = crs.commit_master_identity(&nuls1, &blinding, &TEST_NAME).unwrap();
        let c2 = crs.commit_master_identity(&nuls2, &blinding, &TEST_NAME).unwrap();
        assert_ne!(c1, c2);
    }

    #[test]
    fn commit_different_name_gives_different_commitment() {
        let crs = Crs::setup(make_providers(3));
        let secret = MasterSecret([0x42u8; 32]);
        let nullifiers = derive_all_nullifiers(&secret, &crs.names());
        let blinding = BlindingKey([0x07u8; 32]);

        let name_a = crate::core::types::FriendlyName::new("alice").to_scalar_bytes();
        let name_b = crate::core::types::FriendlyName::new("bob").to_scalar_bytes();
        let c1 = crs.commit_master_identity(&nullifiers, &blinding, &name_a).unwrap();
        let c2 = crs.commit_master_identity(&nullifiers, &blinding, &name_b).unwrap();
        assert_ne!(c1, c2);
    }

    #[test]
    fn commit_wrong_nullifier_count_errors() {
        let crs = Crs::setup(make_providers(3));
        let blinding = BlindingKey([0x07u8; 32]);
        let too_few = vec![Nullifier([0u8; 32]), Nullifier([1u8; 32])];
        assert!(crs.commit_master_identity(&too_few, &blinding, &TEST_NAME).is_err());
    }

    #[test]
    fn commitment_is_33_bytes() {
        let crs = Crs::setup(make_providers(2));
        let secret = MasterSecret([0x42u8; 32]);
        let nullifiers = derive_all_nullifiers(&secret, &crs.names());
        let blinding = BlindingKey([0x07u8; 32]);
        let c = crs.commit_master_identity(&nullifiers, &blinding, &TEST_NAME).unwrap();
        assert_eq!(c.as_bytes().len(), 33);
        assert!(c.as_bytes()[0] == 0x02 || c.as_bytes()[0] == 0x03);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let crs = Crs::setup(make_providers(3));
        let bytes = crs.to_bytes();
        let crs2 = Crs::from_bytes(&bytes).expect("deserialization should succeed");

        assert_eq!(crs.security_param, crs2.security_param);
        assert_eq!(crs.set_size, crs2.set_size);
        assert_eq!(crs.num_providers(), crs2.num_providers());

        let g1: [u8; 33] = crs.g.to_affine().to_bytes().into();
        let g2: [u8; 33] = crs2.g.to_affine().to_bytes().into();
        assert_eq!(g1, g2);

        let hn1: [u8; 33] = crs.h_name.to_affine().to_bytes().into();
        let hn2: [u8; 33] = crs2.h_name.to_affine().to_bytes().into();
        assert_eq!(hn1, hn2);

        for i in 0..crs.num_providers() {
            let h1: [u8; 33] = crs.generators[i].to_affine().to_bytes().into();
            let h2: [u8; 33] = crs2.generators[i].to_affine().to_bytes().into();
            assert_eq!(h1, h2);
            assert_eq!(crs.providers[i].name, crs2.providers[i].name);
            assert_eq!(crs.providers[i].origin, crs2.providers[i].origin);
            assert_eq!(
                crs.providers[i].credential_generator,
                crs2.providers[i].credential_generator
            );
        }
    }

    #[test]
    fn full_flow_crs_setup_derive_commit() {
        // Simulate the full Phase 0 flow:
        // 1. Setup CRS with 4 service providers
        // 2. User generates master secret
        // 3. Derive all nullifiers
        // 4. Commit master identity

        let providers = vec![
            make_provider("twitter.com"),
            make_provider("github.com"),
            make_provider("nostr.com"),
            make_provider("bitcoin.org"),
        ];
        let crs = Crs::setup(providers);
        assert_eq!(crs.num_providers(), 4);

        // User's master secret
        let master_secret = MasterSecret([0xAA; 32]);
        let blinding = BlindingKey([0xBB; 32]);

        // Derive all nullifiers
        let names = crs.names();
        let nullifiers = derive_all_nullifiers(&master_secret, &names);
        assert_eq!(nullifiers.len(), 4);

        // All nullifiers must be unique
        let unique: std::collections::HashSet<_> = nullifiers.iter().map(|n| n.0).collect();
        assert_eq!(unique.len(), 4);

        // Commit master identity
        let phi = crs.commit_master_identity(&nullifiers, &blinding, &TEST_NAME).unwrap();
        assert_eq!(phi.as_bytes().len(), 33);
        assert!(phi.as_bytes()[0] == 0x02 || phi.as_bytes()[0] == 0x03);
    }
}
