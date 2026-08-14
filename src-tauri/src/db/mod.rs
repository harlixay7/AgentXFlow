pub mod migrations;

use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

#[derive(Debug, Clone)]
pub struct DbPool {
    conn: Arc<Mutex<Connection>>,
}

impl DbPool {
    pub fn new(db_path: &Path) -> SqlResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        info!("Opening SQLite database at {:?}", db_path);
        let mut conn = Connection::open(db_path)?;
        migrations::run_migrations(&mut conn)?;

        let pool = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        Ok(pool)
    }

    pub fn new_in_memory() -> SqlResult<Self> {
        info!("Opening in-memory SQLite database");
        let mut conn = Connection::open_in_memory()?;
        migrations::run_migrations(&mut conn)?;

        let pool = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        Ok(pool)
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}
