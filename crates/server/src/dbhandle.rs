//! A shared database handle.
//!
//! rusqlite's [`Connection`] is `!Send`, so it cannot live in shared
//! async state (a `tokio::sync::Mutex<Connection>` is `!Send` and
//! breaks `axum::serve`). [`Db`] solves this by opening a fresh
//! connection per operation instead of sharing one: SQLite opens are
//! sub-millisecond, WAL mode allows many concurrent connections, and
//! writes serialize on the file itself. Each [`Db::with`] call is a
//! short, synchronous critical section.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

#[derive(Clone)]
pub struct Db {
    path: PathBuf,
}

impl Db {
    /// Open (and initialize the schema of) the database in `data_dir`.
    pub fn new(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join(crate::db::DB_FILE);
        crate::db::open(data_dir)?;
        Ok(Self { path })
    }

    /// Run a closure with a fresh connection.
    pub fn with<R>(&self, f: impl FnOnce(&Connection) -> anyhow::Result<R>) -> anyhow::Result<R> {
        let c = Connection::open(&self.path)?;
        c.pragma_update(None, "synchronous", "NORMAL")?;
        // SQLite disables foreign keys by default; with this ON the
        // schema's ON DELETE CASCADE (library deletion removes its
        // file and job rows) actually works.
        c.pragma_update(None, "foreign_keys", "ON")?;
        // Under WAL, concurrent writers (reaper + workers + scan) can
        // hit SQLITE_BUSY briefly; wait instead of erroring.
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        let r = f(&c)?;
        Ok(r)
    }
}
