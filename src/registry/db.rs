//! SQLite persistence for the registry.
//!
//! Stores merchants, anonymity sets, and beneficiary commitments
//! so the registry can be reconstructed on restart.

use bitcoin::hashes::Hash;
use bitcoin::{OutPoint, Txid};
use rusqlite::{params, Connection, Result as SqlResult};

use crate::core::types::Commitment;

/// State loaded from the database for reconstruction.
pub struct DbState {
    pub merchants: Vec<MerchantRow>,
    pub sets: Vec<SetRow>,
    pub commitments: Vec<CommitmentRow>,
}

pub struct MerchantRow {
    pub name: String,
    pub origin: String,
    pub email: String,
    pub phone: String,
}

pub struct SetRow {
    pub set_id: u64,
    pub beneficiary_capacity: usize,
    pub sats_per_user: u64,
    pub finalized: bool,
    pub merchant_names: Vec<String>,
}

pub struct CommitmentRow {
    pub set_id: u64,
    pub idx: usize,
    pub phi: Commitment,
    pub txid: Txid,
    pub vout: u32,
    pub value: u64,
}

/// Open (or create) the SQLite database and ensure all tables exist.
pub fn open_db(path: &str) -> SqlResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    create_tables(&conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS merchants (
            name    TEXT PRIMARY KEY,
            origin  TEXT NOT NULL,
            email   TEXT NOT NULL DEFAULT '',
            phone   TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS sets (
            set_id              INTEGER PRIMARY KEY,
            beneficiary_capacity INTEGER NOT NULL,
            sats_per_user       INTEGER NOT NULL,
            finalized           INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS set_merchants (
            set_id        INTEGER NOT NULL REFERENCES sets(set_id),
            position      INTEGER NOT NULL,
            merchant_name TEXT NOT NULL REFERENCES merchants(name),
            PRIMARY KEY (set_id, position)
        );

        CREATE TABLE IF NOT EXISTS commitments (
            set_id INTEGER NOT NULL REFERENCES sets(set_id),
            idx    INTEGER NOT NULL,
            phi    BLOB NOT NULL,
            txid   BLOB NOT NULL,
            vout   INTEGER NOT NULL,
            value  INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (set_id, idx)
        );

        CREATE TABLE IF NOT EXISTS wallet (
            id         INTEGER PRIMARY KEY CHECK (id = 1),
            secret_key BLOB NOT NULL
        );
        ",
    )?;
    Ok(())
}

// ── Write operations ──────────────────────────────────────────────────────────

pub fn save_merchant(
    conn: &Connection,
    name: &str,
    origin: &str,
    email: &str,
    phone: &str,
) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO merchants (name, origin, email, phone) VALUES (?1, ?2, ?3, ?4)",
        params![name, origin, email, phone],
    )?;
    Ok(())
}

pub fn save_set(
    conn: &Connection,
    set_id: u64,
    beneficiary_capacity: usize,
    sats_per_user: u64,
    merchant_names: &[String],
) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sets (set_id, beneficiary_capacity, sats_per_user) VALUES (?1, ?2, ?3)",
        params![set_id as i64, beneficiary_capacity as i64, sats_per_user as i64],
    )?;
    for (pos, name) in merchant_names.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO set_merchants (set_id, position, merchant_name) VALUES (?1, ?2, ?3)",
            params![set_id as i64, pos as i64, name],
        )?;
    }
    Ok(())
}

pub fn save_commitment(
    conn: &Connection,
    set_id: u64,
    idx: usize,
    phi: &Commitment,
    outpoint: &OutPoint,
    value: u64,
) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO commitments (set_id, idx, phi, txid, vout, value) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            set_id as i64,
            idx as i64,
            &phi.0[..],
            &outpoint.txid.to_byte_array()[..],
            outpoint.vout,
            value as i64,
        ],
    )?;
    Ok(())
}

pub fn save_wallet_key(conn: &Connection, secret_key: &[u8; 32]) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO wallet (id, secret_key) VALUES (1, ?1)",
        params![&secret_key[..]],
    )?;
    Ok(())
}

pub fn load_wallet_key(conn: &Connection) -> SqlResult<Option<[u8; 32]>> {
    let mut stmt = conn.prepare("SELECT secret_key FROM wallet WHERE id = 1")?;
    let mut rows = stmt.query_map([], |row| {
        let blob: Vec<u8> = row.get(0)?;
        let arr: [u8; 32] = blob.try_into().map_err(|_| {
            rusqlite::Error::InvalidColumnType(0, "secret_key".into(), rusqlite::types::Type::Blob)
        })?;
        Ok(arr)
    })?;
    match rows.next() {
        Some(Ok(key)) => Ok(Some(key)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn mark_set_finalized(conn: &Connection, set_id: u64) -> SqlResult<()> {
    conn.execute(
        "UPDATE sets SET finalized = 1 WHERE set_id = ?1",
        params![set_id as i64],
    )?;
    Ok(())
}

// ── Read operations ───────────────────────────────────────────────────────────

pub fn load_state(conn: &Connection) -> SqlResult<DbState> {
    let merchants = load_merchants(conn)?;
    let sets = load_sets(conn)?;
    let commitments = load_commitments(conn)?;
    Ok(DbState {
        merchants,
        sets,
        commitments,
    })
}

fn load_merchants(conn: &Connection) -> SqlResult<Vec<MerchantRow>> {
    let mut stmt = conn.prepare("SELECT name, origin, email, phone FROM merchants")?;
    let rows = stmt.query_map([], |row| {
        Ok(MerchantRow {
            name: row.get(0)?,
            origin: row.get(1)?,
            email: row.get(2)?,
            phone: row.get(3)?,
        })
    })?;
    rows.collect()
}

fn load_sets(conn: &Connection) -> SqlResult<Vec<SetRow>> {
    let mut stmt =
        conn.prepare("SELECT set_id, beneficiary_capacity, sats_per_user, finalized FROM sets")?;
    let set_rows: Vec<(u64, usize, u64, bool)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, bool>(3)?,
            ))
        })?
        .collect::<SqlResult<_>>()?;

    let mut sets = Vec::with_capacity(set_rows.len());
    for (set_id, cap, spu, fin) in set_rows {
        let mut m_stmt = conn.prepare(
            "SELECT merchant_name FROM set_merchants WHERE set_id = ?1 ORDER BY position",
        )?;
        let names: Vec<String> = m_stmt
            .query_map(params![set_id as i64], |row| row.get(0))?
            .collect::<SqlResult<_>>()?;

        sets.push(SetRow {
            set_id,
            beneficiary_capacity: cap,
            sats_per_user: spu,
            finalized: fin,
            merchant_names: names,
        });
    }
    Ok(sets)
}

fn load_commitments(conn: &Connection) -> SqlResult<Vec<CommitmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT set_id, idx, phi, txid, vout, value FROM commitments ORDER BY set_id, idx",
    )?;
    let rows = stmt.query_map([], |row| {
        let set_id: i64 = row.get(0)?;
        let idx: i64 = row.get(1)?;
        let phi_blob: Vec<u8> = row.get(2)?;
        let txid_blob: Vec<u8> = row.get(3)?;
        let vout: u32 = row.get(4)?;
        let value: i64 = row.get(5)?;

        let phi_arr: [u8; 33] = phi_blob
            .try_into()
            .map_err(|_| rusqlite::Error::InvalidColumnType(2, "phi".into(), rusqlite::types::Type::Blob))?;
        let txid_arr: [u8; 32] = txid_blob
            .try_into()
            .map_err(|_| rusqlite::Error::InvalidColumnType(3, "txid".into(), rusqlite::types::Type::Blob))?;

        Ok(CommitmentRow {
            set_id: set_id as u64,
            idx: idx as usize,
            phi: Commitment(phi_arr),
            txid: Txid::from_byte_array(txid_arr),
            vout,
            value: value as u64,
        })
    })?;
    rows.collect()
}

