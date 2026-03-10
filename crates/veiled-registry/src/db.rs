use std::sync::Mutex;

use rusqlite::{Connection, params};
use veiled_core::{AnonymitySet, Commitment, Nullifier};

use crate::store::RegistryStore;

/// SQLite-backed persistence layer.
///
/// The in-memory [`RegistryStore`] is the live source of truth.  `Db` writes
/// every mutation through to a SQLite file so the state survives restarts.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (or create) a SQLite database at `path` and initialise the schema.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS anonymity_sets (
                id       INTEGER PRIMARY KEY,
                capacity INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS commitments (
                set_id     INTEGER NOT NULL,
                idx        INTEGER NOT NULL,
                commitment BLOB    NOT NULL,
                PRIMARY KEY (set_id, idx)
            );
            CREATE TABLE IF NOT EXISTS nullifiers (
                nullifier BLOB PRIMARY KEY
            );",
        )
    }

    /// Reconstruct a [`RegistryStore`] from the persisted data.
    /// Called once at startup.
    pub fn load_store(&self, set_capacity: usize) -> rusqlite::Result<RegistryStore> {
        let (anonymity_sets, nullifiers) = {
            let conn = self.conn.lock().unwrap();

            // Load all sets ordered by id.
            let mut stmt = conn.prepare("SELECT id, capacity FROM anonymity_sets ORDER BY id")?;
            let sets: Vec<(u64, usize)> = stmt
                .query_map([], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as usize)))?
                .collect::<rusqlite::Result<_>>()?;

            let mut anonymity_sets: Vec<AnonymitySet> = Vec::new();
            for (set_id, cap) in &sets {
                let mut aset = AnonymitySet::new(*set_id, *cap);
                let mut cstmt = conn.prepare(
                    "SELECT commitment FROM commitments WHERE set_id = ?1 ORDER BY idx",
                )?;
                let commitments: Vec<Commitment> = cstmt
                    .query_map(params![*set_id as i64], |row| {
                        let blob: Vec<u8> = row.get(0)?;
                        let arr: [u8; 33] = blob.try_into().map_err(|_| {
                            rusqlite::Error::InvalidColumnType(
                                0,
                                "commitment".into(),
                                rusqlite::types::Type::Blob,
                            )
                        })?;
                        Ok(Commitment(arr))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                for c in commitments {
                    aset.push(c);
                }
                anonymity_sets.push(aset);
            }

            // Load all nullifiers.
            let mut nstmt = conn.prepare("SELECT nullifier FROM nullifiers")?;
            let nullifiers: std::collections::HashSet<Nullifier> = nstmt
                .query_map([], |row| {
                    let blob: Vec<u8> = row.get(0)?;
                    let arr: [u8; 32] = blob.try_into().map_err(|_| {
                        rusqlite::Error::InvalidColumnType(
                            0,
                            "nullifier".into(),
                            rusqlite::types::Type::Blob,
                        )
                    })?;
                    Ok(Nullifier(arr))
                })?
                .collect::<rusqlite::Result<_>>()?;

            (anonymity_sets, nullifiers)
            // conn lock dropped here
        };

        // If the DB is empty, seed with one fresh set and persist it.
        let sets = if anonymity_sets.is_empty() {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO anonymity_sets (id, capacity) VALUES (0, ?1)",
                rusqlite::params![set_capacity as i64],
            )?;
            vec![AnonymitySet::new(0, set_capacity)]
        } else {
            anonymity_sets
        };

        Ok(RegistryStore::from_parts(sets, nullifiers, set_capacity))
    }

    /// Persist a newly opened anonymity set.
    pub fn persist_new_set(&self, set_id: u64, capacity: usize) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO anonymity_sets (id, capacity) VALUES (?1, ?2)",
            params![set_id as i64, capacity as i64],
        )?;
        Ok(())
    }

    /// Persist a commitment + nullifier registration.
    pub fn persist_registration(
        &self,
        set_id: u64,
        idx: usize,
        commitment: &Commitment,
        nullifier: &Nullifier,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO commitments (set_id, idx, commitment) VALUES (?1, ?2, ?3)",
            params![set_id as i64, idx as i64, commitment.as_bytes()],
        )?;
        conn.execute(
            "INSERT INTO nullifiers (nullifier) VALUES (?1)",
            params![nullifier.as_bytes()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veiled_core::{Commitment, Nullifier};

    fn open_memory_db() -> Db {
        Db::open(":memory:").expect("in-memory DB failed")
    }

    #[test]
    fn fresh_db_seeds_set_zero() {
        let db = open_memory_db();
        let store = db.load_store(8).expect("load_store failed");
        assert_eq!(store.sets.len(), 1);
        assert_eq!(store.sets[0].id, 0);
        assert_eq!(store.set_capacity, 8);
        assert!(store.nullifiers.is_empty());
    }

    #[test]
    fn persist_and_reload_registration() {
        let db = open_memory_db();
        // Seed set 0.
        let _ = db.load_store(4).unwrap();

        let commitment = Commitment([0xAA; 33]);
        let nullifier = Nullifier([0xBB; 32]);
        db.persist_registration(0, 0, &commitment, &nullifier).unwrap();

        // Reload: the registration must survive the round-trip.
        let store = db.load_store(4).unwrap();
        assert_eq!(store.sets[0].commitments.len(), 1);
        assert!(store.nullifiers.contains(&nullifier));
    }

    #[test]
    fn persist_multiple_sets() {
        let db = open_memory_db();
        let _ = db.load_store(2).unwrap();

        // Fill set 0 (capacity 2) and open set 1.
        db.persist_registration(0, 0, &Commitment([1; 33]), &Nullifier([1; 32])).unwrap();
        db.persist_registration(0, 1, &Commitment([2; 33]), &Nullifier([2; 32])).unwrap();
        db.persist_new_set(1, 2).unwrap();
        db.persist_registration(1, 0, &Commitment([3; 33]), &Nullifier([3; 32])).unwrap();

        let store = db.load_store(2).unwrap();
        assert_eq!(store.sets.len(), 2);
        assert_eq!(store.sets[0].commitments.len(), 2);
        assert_eq!(store.sets[1].commitments.len(), 1);
        assert_eq!(store.nullifiers.len(), 3);
    }
}
