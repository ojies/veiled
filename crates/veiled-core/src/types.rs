use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte Ed25519-style public key (raw bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// BLAKE3(pub_key || name) — unique per (identity, name) pair.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nullifier(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// BLAKE3(nullifier || blinding) — hiding, binding commitment to a nullifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Commitment(#[serde(with = "hex_bytes")] pub [u8; 32]);

/// Random 32-byte blinding factor chosen by the user at registration time.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindingKey(#[serde(with = "hex_bytes")] pub [u8; 32]);

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
        let arr: [u8; 32] = bytes.try_into().map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Ok(Self(arr))
    }
}

impl BlindingKey {
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

impl fmt::Debug for PublicKey  { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "PublicKey({})", hex::encode(self.0)) } }
impl fmt::Debug for Nullifier  { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Nullifier({})", hex::encode(self.0)) } }
impl fmt::Debug for Commitment { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Commitment({})", hex::encode(self.0)) } }
impl fmt::Debug for BlindingKey{ fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "BlindingKey(...)") } }

// ── serde hex codec ──────────────────────────────────────────────────────────
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
