use crate::{auth, Error};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db, Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tenants (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                key_hash TEXT NOT NULL UNIQUE
            );",
        )?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_tenant(&self, name: &str) -> Result<String, Error> {
        if name.trim().is_empty() {
            return Err(Error::BadRequest("tenant name is empty".into()));
        }
        let key = auth::new_key();
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO tenants (name, key_hash) VALUES (?1, ?2)",
            params![name, auth::hash_key(&key)],
        )?;
        Ok(key)
    }

    pub fn find_by_hash(&self, hash: &str) -> Result<Option<i64>, Error> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let id = conn
            .query_row(
                "SELECT id FROM tenants WHERE key_hash = ?1",
                params![hash],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        Ok(id)
    }
}
