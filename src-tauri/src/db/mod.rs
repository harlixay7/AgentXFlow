pub mod migrations;

#[cfg(test)]
mod tests;

use fs2::FileExt;
use rusqlite::{Connection, Result as SqlResult};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::error::CoordinatorError;

#[derive(Debug, Clone)]
pub struct DbPool {
    conn: Arc<Mutex<Connection>>,
    _lock_file: Option<Arc<File>>,
}

impl DbPool {
    pub fn new(db_path: &Path) -> Result<Self, CoordinatorError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let lock_file = {
            let lock_path = db_path.with_extension("lock");
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(|e| {
                    CoordinatorError::Io(format!(
                        "Could not open coordinator lockfile {:?}: {}",
                        lock_path, e
                    ))
                })?;
            file.try_lock_exclusive().map_err(|e| {
                CoordinatorError::Database(format!(
                    "Another AgentXFlow coordinator instance is already running (lock {:?} is held): {}",
                    lock_path, e
                ))
            })?;
            Some(Arc::new(file))
        };

        info!("Opening SQLite database at {:?}", db_path);
        let mut conn = Connection::open(db_path)?;
        migrations::run_migrations(&mut conn)?;

        let pool = Self {
            conn: Arc::new(Mutex::new(conn)),
            _lock_file: lock_file,
        };

        Ok(pool)
    }

    pub fn new_in_memory() -> SqlResult<Self> {
        info!("Opening in-memory SQLite database");
        let mut conn = Connection::open_in_memory()?;
        migrations::run_migrations(&mut conn)?;

        let pool = Self {
            conn: Arc::new(Mutex::new(conn)),
            _lock_file: None,
        };

        Ok(pool)
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }
}
