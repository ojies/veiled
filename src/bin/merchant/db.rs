//! SQLite persistence for merchant registered identities.
//!
//! Stores payment identity registrations so the merchant can be
//! reconstructed on restart without requiring re-registration.

use rusqlite::{params, Connection, Result as SqlResult};

/// A row from the `identities` table.
pub struct IdentityRow {
    pub pseudonym: [u8; 33],
    pub public_nullifier: [u8; 33],
    pub set_id: [u8; 32],
    pub service_index: usize,
    pub friendly_name: String,
    pub proof_blob: Vec<u8>,
}

/// Open (or create) the merchant SQLite database and ensure the table exists.
pub fn open_db(path: &str) -> SqlResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS identities (
            pseudonym       BLOB PRIMARY KEY,
            public_nullifier BLOB NOT NULL,
            set_id          BLOB NOT NULL,
            service_index   INTEGER NOT NULL,
            friendly_name   TEXT NOT NULL,
            proof_blob      BLOB NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_nullifier ON identities (public_nullifier);",
    )?;
    Ok(conn)
}

/// Persist a registered identity.
pub fn save_identity(
    conn: &Connection,
    pseudonym: &[u8; 33],
    nullifier: &[u8; 33],
    set_id: &[u8; 32],
    service_index: usize,
    friendly_name: &str,
    proof_blob: &[u8],
) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO identities
            (pseudonym, public_nullifier, set_id, service_index, friendly_name, proof_blob)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            &pseudonym[..],
            &nullifier[..],
            &set_id[..],
            service_index as i64,
            friendly_name,
            proof_blob,
        ],
    )?;
    Ok(())
}

/// Load all registered identities from the database.
pub fn load_identities(conn: &Connection) -> SqlResult<Vec<IdentityRow>> {
    let mut stmt = conn.prepare(
        "SELECT pseudonym, public_nullifier, set_id, service_index, friendly_name, proof_blob
         FROM identities",
    )?;
    let rows = stmt.query_map([], |row| {
        let ps: Vec<u8> = row.get(0)?;
        let nl: Vec<u8> = row.get(1)?;
        let si: Vec<u8> = row.get(2)?;
        let idx: i64 = row.get(3)?;

        let pseudonym: [u8; 33] = ps.try_into().map_err(|_| {
            rusqlite::Error::InvalidColumnType(0, "pseudonym".into(), rusqlite::types::Type::Blob)
        })?;
        let public_nullifier: [u8; 33] = nl.try_into().map_err(|_| {
            rusqlite::Error::InvalidColumnType(1, "nullifier".into(), rusqlite::types::Type::Blob)
        })?;
        let set_id: [u8; 32] = si.try_into().map_err(|_| {
            rusqlite::Error::InvalidColumnType(2, "set_id".into(), rusqlite::types::Type::Blob)
        })?;

        Ok(IdentityRow {
            pseudonym,
            public_nullifier,
            set_id,
            service_index: idx as usize,
            friendly_name: row.get(4)?,
            proof_blob: row.get(5)?,
        })
    })?;
    rows.collect()
}
