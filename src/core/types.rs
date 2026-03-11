use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::fmt;

/// A 32-byte Ed25519-style public key (raw bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// SHA256(pub_key || name) — unique per (identity, name) pair.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nullifier(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// Pedersen commitment `r·G + v·H` on secp256k1 — hiding, binding commitment to a nullifier.
/// Stored as a 33-byte compressed secp256k1 point.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Commitment(#[serde(with = "hex_bytes_33")] pub [u8; 33]);

/// Random 32-byte blinding factor chosen by the user at registration time.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindingKey(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// 32-byte master secret used to derive per-service-provider nullifiers via HKDF.
///
/// This is the user's root identity secret — must be kept private.
/// A single master secret deterministically produces L different nullifiers
/// (one per user) via `HKDF(master_secret, salt=name)`.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterSecret(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// 32-byte child credential randomness used to derive service-specific
/// authentication keys via HKDF. Independent from `sk` (MasterSecret) so
/// that service registrations don't leak information about nullifiers.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildRandomness(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// Human-readable identity name (name / handle).
///
/// Length is capped at [`Name::MAX_LEN`] bytes (255, fitting in a `u8`).
/// Use [`Name::try_new`] for fallible construction, or [`Name::new`] when
/// the input is known to be valid (panics on overflow — safe for tests and
/// compile-time constants).
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Name(pub String);

impl Name {
    /// Maximum allowed length for a name, in bytes.
    pub const MAX_LEN: usize = u8::MAX as usize; // 255

    /// Create a `Name`, returning an error if `s` exceeds [`Self::MAX_LEN`] bytes.
    pub fn try_new(s: impl Into<String>) -> Result<Self, String> {
        let s = s.into();
        if s.len() > Self::MAX_LEN {
            return Err(format!("name too long: {} bytes (max {})", s.len(), Self::MAX_LEN));
        }
        Ok(Self(s))
    }

    /// Create a `Name`, panicking if `s` exceeds [`Self::MAX_LEN`] bytes.
    ///
    /// Intended for tests and examples where the input is known to be short.
    /// Prefer [`Name::try_new`] in production code paths.
    pub fn new(s: impl Into<String>) -> Self {
        Self::try_new(s).expect("name exceeds MAX_LEN")
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl<'de> Deserialize<'de> for Name {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() > Self::MAX_LEN {
            return Err(D::Error::custom(format!(
                "name too long: {} bytes (max {})", s.len(), Self::MAX_LEN
            )));
        }
        Ok(Self(s))
    }
}

/// A user-chosen global friendly name committed inside Φ.
///
/// This is distinct from `Name` (which represents user names
/// in the CRS). The friendly name is a human-readable global identifier
/// cryptographically bound into the master identity commitment via
/// `name_scalar · h_name`.
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FriendlyName(pub String);

impl FriendlyName {
    /// Maximum allowed length for a friendly name, in bytes.
    pub const MAX_LEN: usize = u8::MAX as usize; // 255

    /// Create a `FriendlyName`, returning an error if `s` exceeds [`Self::MAX_LEN`] bytes.
    pub fn try_new(s: impl Into<String>) -> Result<Self, String> {
        let s = s.into();
        if s.len() > Self::MAX_LEN {
            return Err(format!(
                "friendly name too long: {} bytes (max {})",
                s.len(),
                Self::MAX_LEN
            ));
        }
        Ok(Self(s))
    }

    /// Create a `FriendlyName`, panicking if `s` exceeds [`Self::MAX_LEN`] bytes.
    pub fn new(s: impl Into<String>) -> Self {
        Self::try_new(s).expect("friendly name exceeds MAX_LEN")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive the scalar committed inside Φ: `SHA256(friendly_name)` reduced to Z_q.
    pub fn to_scalar_bytes(&self) -> [u8; 32] {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(self.0.as_bytes());
        hash.into()
    }
}

impl<'de> Deserialize<'de> for FriendlyName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() > Self::MAX_LEN {
            return Err(D::Error::custom(format!(
                "friendly name too long: {} bytes (max {})",
                s.len(),
                Self::MAX_LEN
            )));
        }
        Ok(Self(s))
    }
}

impl fmt::Debug for FriendlyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FriendlyName({:?})", self.0)
    }
}

impl fmt::Display for FriendlyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A fixed-size batch of commitments that together form one anonymity set.
#[derive(Clone, Serialize, Deserialize)]
pub struct AnonymitySet {
    pub id: u64,
    pub commitments: Vec<Commitment>,
    /// Maximum number of commitments before the set is sealed.
    pub capacity: usize,
}

impl AnonymitySet {
    pub fn new(id: u64, capacity: usize) -> Self {
        Self { id, commitments: Vec::new(), capacity }
    }

    pub fn is_full(&self) -> bool {
        self.commitments.len() >= self.capacity
    }

    pub fn push(&mut self, c: Commitment) {
        self.commitments.push(c);
    }
}

// ── hex display helpers ──────────────────────────────────────────────────────

impl PublicKey {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Ok(Self(arr))
    }
}

impl Nullifier {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
    pub fn to_hex(&self) -> String { hex::encode(self.0) }
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Ok(Self(arr))
    }
}

impl Commitment {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
    pub fn to_hex(&self) -> String { hex::encode(self.0) }
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        let arr: [u8; 33] = bytes.try_into().map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Ok(Self(arr))
    }
}

impl BlindingKey {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl MasterSecret {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl ChildRandomness {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl fmt::Debug for PublicKey  { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "PublicKey({})", hex::encode(self.0)) } }
impl fmt::Debug for Nullifier  { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Nullifier({})", hex::encode(self.0)) } }
impl fmt::Debug for Commitment { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Commitment({})", hex::encode(self.0)) } }
impl fmt::Debug for BlindingKey   { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "BlindingKey(...)") } }
impl fmt::Debug for MasterSecret    { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "MasterSecret(...)") } }
impl fmt::Debug for ChildRandomness { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "ChildRandomness(...)") } }
impl fmt::Debug for Name       { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Name({:?})", self.0) } }
impl fmt::Display for Name     { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) } }

// ── serde hex codec (32-byte arrays) ─────────────────────────────────────────
mod hex_bytes {
    use serde::{Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s: &str = serde::Deserialize::deserialize(d)?;
        let bytes = hex::decode(s).map_err(D::Error::custom)?;
        bytes.try_into().map_err(|_| D::Error::custom("expected 32 bytes"))
    }
}

// ── serde hex codec (33-byte arrays, for compressed EC points) ───────────────
mod hex_bytes_33 {
    use serde::{Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 33], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 33], D::Error> {
        let s: &str = serde::Deserialize::deserialize(d)?;
        let bytes = hex::decode(s).map_err(D::Error::custom)?;
        bytes.try_into().map_err(|_| D::Error::custom("expected 33 bytes"))
    }
}
