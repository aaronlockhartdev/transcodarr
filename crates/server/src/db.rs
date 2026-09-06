//! SQLite persistence (DESIGN §10): libraries, files, jobs, devices,
//! settings. All access is synchronous (rusqlite); from async contexts
//! use [`crate::dbhandle::Db`] (a fresh short-lived connection per
//! operation — rusqlite's `Connection` is `!Send`). WAL mode.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use transcodarr_core::device::{Device, DeviceKind};

/// The database file name inside the data dir.
pub const DB_FILE: &str = "transcodarr.db";

/// The quarantine sub-directory name (failed outputs, replaced
/// originals).
pub const QUARANTINE_DIR: &str = "quarantine";

pub fn open(data_dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("create data dir {}", data_dir.display()))?;
    let db_path = data_dir.join(DB_FILE);
    let conn = Connection::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("enable WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Matches dbhandle::with — needed for the schema's ON DELETE
    // CASCADE / SET NULL to actually fire.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS flows (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            flow_json  TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS libraries (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            name           TEXT NOT NULL UNIQUE,
            path           TEXT NOT NULL,
            lifecycle_mode TEXT NOT NULL DEFAULT 'manual',
            flow_id        INTEGER REFERENCES flows(id) ON DELETE SET NULL,
            auto_queue     INTEGER NOT NULL DEFAULT 1,
            retention_days INTEGER NOT NULL DEFAULT 7,
            auto_delete    INTEGER NOT NULL DEFAULT 0,
            scan_schedule  TEXT,
            watchable      INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS files (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            library_id     INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
            path           TEXT NOT NULL UNIQUE,
            dev            INTEGER NOT NULL,
            inode          INTEGER NOT NULL,
            size           INTEGER NOT NULL DEFAULT 0,
            mtime          INTEGER NOT NULL DEFAULT 0,
            sample_hash    INTEGER,
            facts_json     TEXT,
            status         TEXT NOT NULL DEFAULT 'unscanned',
            last_probed    INTEGER,
            last_evaluated INTEGER,
            input_size     INTEGER,
            output_size    INTEGER,
            applied_filters TEXT
        );

        CREATE TABLE IF NOT EXISTS jobs (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id          INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            library_id       INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
            flow_version     INTEGER NOT NULL,
            plan_json        TEXT NOT NULL,
            state            TEXT NOT NULL DEFAULT 'queued',
            device_id        TEXT,
            claimed_by       TEXT,
            lease_expires    INTEGER,
            started          INTEGER,
            ended            INTEGER,
            exit_kind        TEXT,
            log_path         TEXT,
            log_zstd         BLOB,
            quarantine_path  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_jobs_state ON jobs(state);

        CREATE TABLE IF NOT EXISTS devices (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            key             TEXT NOT NULL UNIQUE,
            kind            TEXT NOT NULL,
            name            TEXT NOT NULL,
            encoders_json   TEXT NOT NULL DEFAULT '[]',
            max_concurrent  INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )
    .context("init schema")?;
    migrate_flows(conn)?;
    migrate_job_logs(conn)?;
    migrate_applied_filters(conn)?;
    Ok(())
}

/// One-shot migration for databases created before flows were
/// first-class (DESIGN §10): each distinct `libraries.flow_json`
/// becomes a `flows` row, libraries gain `flow_id`, and the old
/// column is dropped. No-op on fresh databases.
fn migrate_flows(conn: &Connection) -> Result<()> {
    let has_old_column: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('libraries') WHERE name = 'flow_json'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .context("inspect libraries schema")?;
    if !has_old_column {
        return Ok(());
    }
    let now = now_s();
    conn.execute_batch("BEGIN")?;
    let distinct: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT DISTINCT flow_json FROM libraries ORDER BY flow_json")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("list old flow JSONs")?
    };
    for (i, fj) in distinct.iter().enumerate() {
        let is_default = fj == "{}" || fj == r#"{"flow_version":1,"steps":[]}"#;
        let name = if is_default {
            "Default".into()
        } else {
            format!("Imported {}", i + 1)
        };
        conn.execute(
            "INSERT OR IGNORE INTO flows (name, flow_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![name, fj, now],
        )
        .with_context(|| format!("import flow {name}"))?;
    }
    conn.execute(
        "ALTER TABLE libraries
         ADD COLUMN flow_id INTEGER REFERENCES flows(id) ON DELETE SET NULL",
        [],
    )
    .context("add libraries.flow_id")?;
    conn.execute(
        "UPDATE libraries SET flow_id =
             (SELECT f.id FROM flows f WHERE f.flow_json = libraries.flow_json LIMIT 1)",
        [],
    )
    .context("backfill libraries.flow_id")?;
    conn.execute("ALTER TABLE libraries DROP COLUMN flow_json", [])
        .context("drop libraries.flow_json")?;
    conn.execute_batch("COMMIT")?;
    Ok(())
}

// ── Libraries ─────────────────────────────────────────────────────────

/// A library row as read from / written to SQLite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryRow {
    pub id: i64,
    pub name: String,
    pub path: String,
    /// `manual` | `auto`.
    pub lifecycle_mode: String,
    /// The assigned flow (NULL = no flow → files are unmatched).
    pub flow_id: Option<i64>,
    /// The assigned flow's name (NULL when unassigned); joined in.
    pub flow_name: Option<String>,
    pub auto_queue: bool,
    pub retention_days: i64,
    pub auto_delete: bool,
    pub scan_schedule: Option<String>,
    pub watchable: bool,
}

pub fn insert_library(conn: &Connection, lib: &LibraryRow) -> Result<i64> {
    conn.execute(
        "INSERT INTO libraries (name, path, lifecycle_mode, flow_id, auto_queue,
                                retention_days, auto_delete, scan_schedule, watchable)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            lib.name,
            lib.path,
            lib.lifecycle_mode,
            lib.flow_id,
            i64::from(lib.auto_queue),
            lib.retention_days,
            i64::from(lib.auto_delete),
            lib.scan_schedule,
            i64::from(lib.watchable),
        ],
    )
    .context("insert library")?;
    Ok(conn.last_insert_rowid())
}

pub fn list_libraries(conn: &Connection) -> Result<Vec<LibraryRow>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.name, l.path, l.lifecycle_mode, l.flow_id, f.name,
                l.auto_queue, l.retention_days, l.auto_delete, l.scan_schedule,
                l.watchable
         FROM libraries l LEFT JOIN flows f ON f.id = l.flow_id ORDER BY l.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(LibraryRow {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            lifecycle_mode: r.get(3)?,
            flow_id: r.get(4)?,
            flow_name: r.get(5)?,
            auto_queue: r.get::<_, i64>(6)? != 0,
            retention_days: r.get(7)?,
            auto_delete: r.get::<_, i64>(8)? != 0,
            scan_schedule: r.get(9)?,
            watchable: r.get::<_, i64>(10)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_library(conn: &Connection, id: i64) -> Result<Option<LibraryRow>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.name, l.path, l.lifecycle_mode, l.flow_id, f.name,
                l.auto_queue, l.retention_days, l.auto_delete, l.scan_schedule,
                l.watchable
         FROM libraries l LEFT JOIN flows f ON f.id = l.flow_id WHERE l.id = ?1",
    )?;
    let mut it = stmt.query_map(params![id], |r| {
        Ok(LibraryRow {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            lifecycle_mode: r.get(3)?,
            flow_id: r.get(4)?,
            flow_name: r.get(5)?,
            auto_queue: r.get::<_, i64>(6)? != 0,
            retention_days: r.get(7)?,
            auto_delete: r.get::<_, i64>(8)? != 0,
            scan_schedule: r.get(9)?,
            watchable: r.get::<_, i64>(10)? != 0,
        })
    })?;
    it.next().transpose().context("get library")
}

pub fn update_library(conn: &Connection, lib: &LibraryRow) -> Result<()> {
    let n = conn.execute(
        "UPDATE libraries SET name = ?2, path = ?3, lifecycle_mode = ?4, flow_id = ?5,
                auto_queue = ?6, retention_days = ?7, auto_delete = ?8,
                scan_schedule = ?9, watchable = ?10
         WHERE id = ?1",
        params![
            lib.id,
            lib.name,
            lib.path,
            lib.lifecycle_mode,
            lib.flow_id,
            i64::from(lib.auto_queue),
            lib.retention_days,
            i64::from(lib.auto_delete),
            lib.scan_schedule,
            i64::from(lib.watchable),
        ],
    )?;
    if n == 0 {
        bail!("library {} not found", lib.id);
    }
    Ok(())
}

pub fn delete_library(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM libraries WHERE id = ?1", params![id])?;
    Ok(())
}

// ── Flows ─────────────────────────────────────────────────────────────

/// A flow row as read from / written to SQLite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowRow {
    pub id: i64,
    pub name: String,
    /// The flow (a `transcodarr_core::flow::Flow` as JSON).
    pub flow_json: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
    /// How many libraries currently use this flow.
    pub library_count: i64,
}

pub fn insert_flow(conn: &Connection, name: &str, flow_json: &str) -> Result<i64> {
    let now = now_s();
    conn.execute(
        "INSERT INTO flows (name, flow_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![name, flow_json, now],
    )
    .context("insert flow")?;
    Ok(conn.last_insert_rowid())
}

pub fn list_flows(conn: &Connection) -> Result<Vec<FlowRow>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.name, f.flow_json, f.created_at, f.updated_at,
                (SELECT COUNT(*) FROM libraries l WHERE l.flow_id = f.id)
         FROM flows f ORDER BY f.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(FlowRow {
            id: r.get(0)?,
            name: r.get(1)?,
            flow_json: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
            library_count: r.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_flow(conn: &Connection, id: i64) -> Result<Option<FlowRow>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.name, f.flow_json, f.created_at, f.updated_at,
                (SELECT COUNT(*) FROM libraries l WHERE l.flow_id = f.id)
         FROM flows f WHERE f.id = ?1",
    )?;
    let mut it = stmt.query_map(params![id], |r| {
        Ok(FlowRow {
            id: r.get(0)?,
            name: r.get(1)?,
            flow_json: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
            library_count: r.get(5)?,
        })
    })?;
    it.next().transpose().context("get flow")
}

pub fn update_flow(conn: &Connection, id: i64, name: &str, flow_json: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE flows SET name = ?2, flow_json = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, flow_json, now_s()],
    )?;
    if n == 0 {
        bail!("flow {id} not found");
    }
    Ok(())
}

pub fn delete_flow(conn: &Connection, id: i64) -> Result<()> {
    // Libraries referencing it get flow_id = NULL (ON DELETE SET NULL).
    conn.execute("DELETE FROM flows WHERE id = ?1", params![id])?;
    Ok(())
}

/// The library ids that currently use this flow.
pub fn libraries_using_flow(conn: &Connection, flow_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM libraries WHERE flow_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![flow_id], |r| r.get::<_, i64>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("list libraries")
}

/// The JSON of the library's assigned flow, or the empty flow when it
/// has none (empty flow ⇒ every file evaluates to NoMatch/unmatched,
/// the same as the old empty `flow_json` default).
pub fn flow_json_for_library(conn: &Connection, lib: &LibraryRow) -> Result<String> {
    let json = match lib.flow_id {
        Some(id) => {
            get_flow(conn, id)?
                .ok_or_else(|| anyhow::anyhow!("library {} references missing flow {id}", lib.id))?
                .flow_json
        }
        None => r#"{"flow_version":1,"steps":[]}"#.into(),
    };
    Ok(json)
}

// ── Files ─────────────────────────────────────────────────────────────

/// A file row as read from / written to SQLite.
///
/// Sizes and the sample hash are stored as SQLite `INTEGER` (i64);
/// the hash is used for equality only, so sign is irrelevant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRow {
    pub id: i64,
    pub library_id: i64,
    pub path: String,
    pub dev: i64,
    pub inode: i64,
    pub size: i64,
    /// Unix mtime (seconds).
    pub mtime: i64,
    pub sample_hash: Option<i64>,
    /// Cached `FileFacts` JSON (the unit of evaluation).
    pub facts_json: Option<String>,
    /// `unscanned` | `scanning` | `compliant` | `queued` | `running` |
    /// `verifying` | `completed` | `failed` | `quarantined` |
    /// `unmatched`.
    pub status: String,
    pub last_probed: Option<i64>,
    pub last_evaluated: Option<i64>,
    pub input_size: Option<i64>,
    pub output_size: Option<i64>,
}

pub fn upsert_file(conn: &Connection, row: &FileRow) -> Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM files WHERE library_id = ?1 AND path = ?2",
            params![row.library_id, row.path],
            |r| r.get(0),
        )
        .optional()?;
    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE files SET dev = ?2, inode = ?3, size = ?4, mtime = ?5,
                        sample_hash = ?6, facts_json = ?7, status = ?8,
                        last_probed = ?9, last_evaluated = ?10
                 WHERE id = ?1",
                params![
                    id,
                    row.dev,
                    row.inode,
                    row.size,
                    row.mtime,
                    row.sample_hash,
                    row.facts_json,
                    row.status,
                    row.last_probed,
                    row.last_evaluated,
                ],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO files (library_id, path, dev, inode, size, mtime, sample_hash,
                                    facts_json, status, last_probed, last_evaluated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    row.library_id,
                    row.path,
                    row.dev,
                    row.inode,
                    row.size,
                    row.mtime,
                    row.sample_hash,
                    row.facts_json,
                    row.status,
                    row.last_probed,
                    row.last_evaluated,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn list_files(conn: &Connection, library_id: i64) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, library_id, path, dev, inode, size, mtime, sample_hash,
                facts_json, status, last_probed, last_evaluated,
                input_size, output_size
         FROM files WHERE library_id = ?1 ORDER BY path",
    )?;
    let rows = stmt.query_map(params![library_id], file_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_file(conn: &Connection, id: i64) -> Result<Option<FileRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, library_id, path, dev, inode, size, mtime, sample_hash,
                facts_json, status, last_probed, last_evaluated,
                input_size, output_size
         FROM files WHERE id = ?1",
    )?;
    let mut it = stmt.query_map(params![id], file_from_row)?;
    it.next().transpose().context("get file")
}

fn file_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get(0)?,
        library_id: r.get(1)?,
        path: r.get(2)?,
        dev: r.get(3)?,
        inode: r.get(4)?,
        size: r.get(5)?,
        mtime: r.get(6)?,
        sample_hash: r.get(7)?,
        facts_json: r.get(8)?,
        status: r.get(9)?,
        last_probed: r.get(10)?,
        last_evaluated: r.get(11)?,
        input_size: r.get(12)?,
        output_size: r.get(13)?,
    })
}

pub fn set_file_status(conn: &Connection, id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE files SET status = ?2 WHERE id = ?1",
        params![id, status],
    )
    .context("set file status")?;
    Ok(())
}

/// Rename a file record's path (in-place container adoption,
/// DESIGN §3.1). Errors on a UNIQUE violation of `files.path`.
pub fn rename_file_path(conn: &Connection, id: i64, new_path: &str) -> Result<()> {
    conn.execute(
        "UPDATE files SET path = ?2 WHERE id = ?1",
        params![id, new_path],
    )
    .context("rename file path")?;
    Ok(())
}

// ── Jobs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRow {
    pub id: i64,
    pub file_id: i64,
    pub library_id: i64,
    pub flow_version: i64,
    /// The `FfmpegPlan` JSON this job executes.
    pub plan_json: String,
    /// `queued` | `running` | `verifying` | `completed` | `failed` |
    /// `quarantined` | `canceled`.
    pub state: String,
    pub device_id: Option<String>,
    pub claimed_by: Option<String>,
    pub lease_expires: Option<i64>,
    pub started: Option<i64>,
    pub ended: Option<i64>,
    /// `ok` | `encode_error` | `verify_failed` | `lease_expired` | …
    pub exit_kind: Option<String>,
    pub log_path: Option<String>,
    pub quarantine_path: Option<String>,
}

pub fn insert_job(conn: &Connection, job: &JobRow) -> Result<i64> {
    conn.execute(
        "INSERT INTO jobs (file_id, library_id, flow_version, plan_json, state,
                           device_id, log_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            job.file_id,
            job.library_id,
            job.flow_version,
            job.plan_json,
            job.state,
            job.device_id,
            job.log_path,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn claim_job(
    conn: &Connection,
    id: i64,
    device_id: &str,
    claimed_by: &str,
    lease_expires: i64,
    now: i64,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE jobs SET state = 'running', device_id = ?2, claimed_by = ?3,
                lease_expires = ?4, started = ?5
         WHERE id = ?1 AND state = 'queued'",
        params![id, device_id, claimed_by, lease_expires, now],
    )?;
    Ok(n == 1)
}

/// Current unix-epoch seconds (0 on failure — a conservative
/// "already expired" for lease math).
pub fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Put unfinishable jobs back in the queue and return their ids.
/// With `expired_only`, only jobs whose lease has lapsed are
/// touched (the in-process reaper: a hung or panicked worker); with
/// `false`, every `running`/`verifying` job is reclaimed — correct at
/// startup, when a fresh process by definition holds no in-flight
/// work. Affected files drop from `running`/`verifying` back to
/// `queued` so a scan (or the next poll) can re-claim them. A crashed
/// encode only ever left a sibling temp file behind — the original is
/// intact until a verified swap, so re-running is safe.
pub fn reclaim_jobs(conn: &Connection, expired_only: bool) -> Result<Vec<i64>> {
    // Select the reclaim set first so both updates below are scoped
    // to exactly those rows.
    let select = if expired_only {
        "SELECT id FROM jobs\n         WHERE state IN ('running', 'verifying')\n           AND (lease_expires IS NULL OR lease_expires < ?)"
    } else {
        "SELECT id FROM jobs\n         WHERE state IN ('running', 'verifying')"
    };
    let mut stmt = conn.prepare(select)?;
    let ids: Vec<i64> = if expired_only {
        stmt.query_map(params![now_s()], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?
    } else {
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?
    };
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = vec!["?".to_string(); ids.len()].join(",");
    conn.execute(
        &format!(
            "UPDATE jobs SET state = 'queued', claimed_by = NULL, lease_expires = NULL, started = NULL\n             WHERE id IN ({list}) AND state IN ('running','verifying')"
        ),
        params_from_iter(&ids),
    )?;
    // Files still showing a live state whose job just went back to
    // the queue (scoped to the same set).
    conn.execute(
        &format!(
            "UPDATE files SET status = 'queued'\n             WHERE status IN ('running', 'verifying')\n               AND id IN (SELECT file_id FROM jobs WHERE id IN ({list}) AND file_id IS NOT NULL)"
        ),
        params_from_iter(&ids),
    )?;
    Ok(ids)
}

/// Whether this file has a live job that must not be re-queued (one
/// live job per file). A direct query on the jobs table — no
/// arbitrary row window, so it stays correct no matter how much job
/// history has accumulated.
pub fn has_live_job(conn: &Connection, file_id: i64) -> Result<bool> {
    let r: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM jobs\n         WHERE file_id = ?1 AND state IN ('queued', 'running', 'verifying')\n           LIMIT 1",
            params![file_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(r.is_some())
}

/// Whether this file has a running (in-flight) job. Unlike
/// [`has_live_job`], a queued job does not count: queued work can
/// still be cancelled, running work cannot.
pub fn has_running_job(conn: &Connection, file_id: i64) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM jobs WHERE file_id = ?1 AND state IN ('running', 'verifying')",
        params![file_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Cancel (fail) every queued job of a file with a given exit kind.
/// Used when a flow edit (or a re-probe) makes a queued job's plan
/// stale. Returns the ids seen at call time.
///
/// The cancellation itself is one atomic, state-guarded statement: a
/// job a concurrent worker claims (or re-claims) in the meantime
/// keeps running — only rows still `queued` at commit time are
/// touched. The SELECT that collects the ids runs *before* that
/// statement, so the returned list may include a job that was just
/// claimed; nudging clients about it is harmless (the nudge only
/// triggers a jobs refetch).
pub fn cancel_queued_jobs(conn: &Connection, file_id: i64, exit_kind: &str) -> Result<Vec<i64>> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM jobs WHERE file_id = ?1 AND state = 'queued'")?
        .query_map(params![file_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if !ids.is_empty() {
        conn.execute(
            "UPDATE jobs SET state = 'failed', ended = ?2, exit_kind = ?3,
                           claimed_by = NULL, lease_expires = NULL
             WHERE file_id = ?1 AND state = 'queued'",
            params![file_id, now_s(), exit_kind],
        )?;
    }
    Ok(ids)
}

/// Persist a re-evaluated file's status/timestamp without re-inserting:
/// scoped to (id, path), so a row a concurrent job just renamed
/// (in-place container adoption) is updated in place instead of
/// re-created under the old path (a ghost row no scan can ever touch
/// again). Returns the number of rows updated (0 = the row moved or
/// was deleted).
pub fn touch_file_eval(
    conn: &Connection,
    id: i64,
    path: &str,
    status: &str,
    last_evaluated: Option<i64>,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE files SET status = ?3, last_evaluated = ?4 WHERE id = ?1 AND path = ?2",
        params![id, path, status, last_evaluated],
    )?)
}

/// How many jobs are in flight (running or verifying) — overall
/// (`device_key = None`) or on one device. Feeds the concurrency caps
/// in `jobs::run_loop`.
pub fn running_jobs_count(conn: &Connection, device_key: Option<&str>) -> Result<usize> {
    let n: i64 = match device_key {
        Some(key) => conn.query_row(
            "SELECT COUNT(*) FROM jobs\n             WHERE state IN ('running', 'verifying') AND device_id = ?1",
            params![key],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM jobs\n             WHERE state IN ('running', 'verifying')",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(n as usize)
}

/// Refresh a claimed job's lease (the heartbeat of a live encode): a
/// running encode longer than the original lease must not be reclaimed
/// by the in-process reaper. Only refreshes a job still claimed by the
/// same owner; returns false when the job is gone or re-claimed.
pub fn refresh_job_lease(
    conn: &Connection,
    id: i64,
    claimed_by: &str,
    lease_expires: i64,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE jobs SET lease_expires = ?1\n         WHERE id = ?2\n           AND state IN ('running', 'verifying')\n           AND claimed_by = ?3",
        params![lease_expires, id, claimed_by],
    )?;
    Ok(n > 0)
}

pub fn finish_job(
    conn: &Connection,
    id: i64,
    state: &str,
    exit_kind: &str,
    now: i64,
    quarantine_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET state = ?2, ended = ?3, exit_kind = ?4,
                claimed_by = NULL, lease_expires = NULL,
                quarantine_path = COALESCE(?5, quarantine_path)
         WHERE id = ?1",
        params![id, state, now, exit_kind, quarantine_path],
    )?;
    Ok(())
}

/// One-shot migration for databases created before job logs were
/// archived (DESIGN §10): adds `jobs.log_zstd`. No backfill —
/// pre-archive rows serve their on-disk log file instead.
fn migrate_job_logs(conn: &Connection) -> Result<()> {
    let has_column: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('jobs') WHERE name = 'log_zstd'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .context("inspect jobs schema")?;
    if has_column {
        return Ok(());
    }
    conn.execute("ALTER TABLE jobs ADD COLUMN log_zstd BLOB", [])
        .context("add jobs.log_zstd")?;
    Ok(())
}

/// One-shot migration for databases created before filter graphs
/// were recorded (DESIGN §10): adds `files.applied_filters`.
/// No backfill — an empty ledger simply means "no filter has been
/// applied yet", which is also the fresh-database state.
fn migrate_applied_filters(conn: &Connection) -> Result<()> {
    let has_column: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('files') WHERE name = 'applied_filters'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .context("inspect files schema")?;
    if has_column {
        return Ok(());
    }
    conn.execute("ALTER TABLE files ADD COLUMN applied_filters TEXT", [])
        .context("add files.applied_filters")?;
    Ok(())
}

/// The filter graphs a file's *current bytes* have already had
/// applied (DESIGN §6.5): the sample hash the file had when the last
/// filter pass completed, plus the graphs. `None` when no filter has
/// ever been applied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppliedFilters {
    /// `files.sample_hash` at the moment the graphs were applied.
    pub hash: i64,
    pub graphs: Vec<String>,
}

pub fn get_applied_filters(conn: &Connection, file_id: i64) -> Result<Option<AppliedFilters>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT applied_filters FROM files WHERE id = ?1",
            [file_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    match json {
        Some(j) => serde_json::from_str(&j)
            .map(Some)
            .context("parse applied_filters"),
        None => Ok(None),
    }
}

/// Record that these filter graphs have been applied to a file whose
/// bytes now carry sample hash `hash` (idempotent: existing graphs
/// are kept, the newest hash wins).
pub fn add_applied_filters(
    conn: &Connection,
    file_id: i64,
    hash: i64,
    graphs: &[String],
) -> Result<()> {
    if graphs.is_empty() {
        return Ok(());
    }
    let mut entry = get_applied_filters(conn, file_id)?;
    let entry = entry.get_or_insert_with(|| AppliedFilters {
        hash,
        graphs: Vec::new(),
    });
    entry.hash = hash;
    for g in graphs {
        if !entry.graphs.contains(g) {
            entry.graphs.push(g.clone());
        }
    }
    let json = serde_json::to_string(&entry).context("serialize applied_filters")?;
    conn.execute(
        "UPDATE files SET applied_filters = ?2 WHERE id = ?1",
        params![file_id, json],
    )
    .context("record applied filters")?;
    Ok(())
}

/// Replace (or clear) a file's applied-filter ledger. `None` clears
/// it — used when the file's bytes changed without a filter job
/// (the user replaced the file).
pub fn set_applied_filters(
    conn: &Connection,
    file_id: i64,
    entry: Option<&AppliedFilters>,
) -> Result<()> {
    match entry {
        None => {
            conn.execute(
                "UPDATE files SET applied_filters = NULL WHERE id = ?1",
                [file_id],
            )
            .context("clear applied filters")?;
        }
        Some(e) => {
            let json = serde_json::to_string(e).context("serialize applied_filters")?;
            conn.execute(
                "UPDATE files SET applied_filters = ?2 WHERE id = ?1",
                params![file_id, json],
            )
            .context("set applied filters")?;
        }
    }
    Ok(())
}

pub fn set_job_log_path(conn: &Connection, id: i64, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET log_path = ?2 WHERE id = ?1",
        params![id, path],
    )
    .context("set job log path")?;
    Ok(())
}

/// Store a job's zstd-compressed log (the canonical copy for the
/// UI; the on-disk file remains the ops mirror).
pub fn set_job_log_zstd(conn: &Connection, id: i64, bytes: &[u8]) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET log_zstd = ?2 WHERE id = ?1",
        params![id, bytes],
    )
    .context("set job log zstd")?;
    Ok(())
}

/// A job's archived (zstd) log, if any.
pub fn get_job_log_zstd(conn: &Connection, id: i64) -> Result<Option<Vec<u8>>> {
    let v: Option<Vec<u8>> = conn
        .query_row(
            "SELECT log_zstd FROM jobs WHERE id = ?1",
            params![id],
            |r| match r.get_ref(0)? {
                rusqlite::types::ValueRef::Null => Err(rusqlite::Error::QueryReturnedNoRows),
                other => Ok(other.as_bytes()?.to_vec()),
            },
        )
        .optional()
        .context("get job log zstd")?;
    Ok(v)
}

pub fn list_jobs(conn: &Connection, limit: i64) -> Result<Vec<JobRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, library_id, flow_version, plan_json, state,
                device_id, claimed_by, lease_expires, started, ended,
                exit_kind, log_path, quarantine_path
         FROM jobs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], job_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_job(conn: &Connection, id: i64) -> Result<Option<JobRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, library_id, flow_version, plan_json, state,
                device_id, claimed_by, lease_expires, started, ended,
                exit_kind, log_path, quarantine_path
         FROM jobs WHERE id = ?1",
    )?;
    let mut it = stmt.query_map(params![id], job_from_row)?;
    it.next().transpose().context("get job")
}

pub fn next_queued_job(conn: &Connection, device_key: Option<&str>) -> Result<Option<JobRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_id, library_id, flow_version, plan_json, state,
                device_id, claimed_by, lease_expires, started, ended,
                exit_kind, log_path, quarantine_path
         FROM jobs
         WHERE state = 'queued' AND (device_id IS NULL OR device_id = ?1)
         ORDER BY id LIMIT 1",
    )?;
    let mut it = stmt.query_map(params![device_key], job_from_row)?;
    it.next().transpose().context("next queued job")
}

fn job_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    Ok(JobRow {
        id: r.get(0)?,
        file_id: r.get(1)?,
        library_id: r.get(2)?,
        flow_version: r.get(3)?,
        plan_json: r.get(4)?,
        state: r.get(5)?,
        device_id: r.get(6)?,
        claimed_by: r.get(7)?,
        lease_expires: r.get(8)?,
        started: r.get(9)?,
        ended: r.get(10)?,
        exit_kind: r.get(11)?,
        log_path: r.get(12)?,
        quarantine_path: r.get(13)?,
    })
}

// ── Devices ───────────────────────────────────────────────────────────

fn kind_str(k: &DeviceKind) -> &str {
    match k {
        DeviceKind::Cpu => "cpu",
        DeviceKind::Gpu => "gpu",
    }
}

pub fn upsert_devices(conn: &Connection, devices: &[Device]) -> Result<()> {
    for d in devices {
        let encoders = serde_json::to_string(&d.encoders)?;
        conn.execute(
            "INSERT INTO devices (key, kind, name, encoders_json, max_concurrent)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key) DO UPDATE SET kind = ?2, name = ?3,
                    encoders_json = ?4, max_concurrent = ?5",
            params![d.id, kind_str(&d.kind), d.name, encoders, d.max_concurrent,],
        )?;
    }
    Ok(())
}

pub fn list_devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut stmt =
        conn.prepare("SELECT key, kind, name, encoders_json, max_concurrent FROM devices")?;
    let rows = stmt.query_map([], |r| {
        let kind_s: String = r.get(1)?;
        let kind = match kind_s.as_str() {
            "cpu" => DeviceKind::Cpu,
            _ => DeviceKind::Gpu,
        };
        let enc: String = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            kind,
            r.get::<_, String>(2)?,
            enc,
            r.get::<_, i64>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (key, kind, name, enc_json, maxc) = row?;
        let encoders: Vec<transcodarr_core::device::Encoder> =
            serde_json::from_str(&enc_json).unwrap_or_default();
        out.push(Device {
            id: key,
            kind,
            name,
            encoders,
            max_concurrent: maxc as u32,
        });
    }
    Ok(out)
}

// ── Settings ──────────────────────────────────────────────────────────

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut it = stmt.query_map(params![key], |r| r.get::<_, String>(0))?;
    it.next().transpose().context("get setting")
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

/// Ensure the quarantine directory exists under the data dir.
pub fn quarantine_dir(data_dir: &Path) -> PathBuf {
    let dir = data_dir.join(QUARANTINE_DIR);
    let _ = std::fs::create_dir_all(&dir);
    dir
}
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> (PathBuf, Connection) {
        let dir =
            std::env::temp_dir().join(format!("transcodarr-db-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir).unwrap();
        (dir, conn)
    }

    fn lib_row() -> LibraryRow {
        LibraryRow {
            id: 0,
            name: "L".into(),
            path: "/l".into(),
            lifecycle_mode: "auto".into(),
            flow_id: None,
            flow_name: None,
            auto_queue: true,
            retention_days: 7,
            auto_delete: false,
            scan_schedule: None,
            watchable: false,
        }
    }

    fn file_row(library_id: i64, path: &str) -> FileRow {
        FileRow {
            id: 0,
            library_id,
            path: path.into(),
            dev: 1,
            inode: 2,
            size: 3,
            mtime: 4,
            sample_hash: None,
            facts_json: None,
            status: "queued".into(),
            last_probed: None,
            last_evaluated: None,
            input_size: None,
            output_size: None,
        }
    }

    #[test]
    fn cancel_queued_jobs_fails_only_queued() {
        let (dir, conn) = temp_db("cancel");
        let lib = insert_library(&conn, &lib_row()).unwrap();
        let fid = upsert_file(&conn, &file_row(lib, "/l/a.mkv")).unwrap();
        let mut job = JobRow {
            id: 0,
            file_id: fid,
            library_id: lib,
            flow_version: 1,
            plan_json: "{}".into(),
            state: "queued".into(),
            device_id: None,
            claimed_by: None,
            lease_expires: None,
            started: None,
            ended: None,
            exit_kind: None,
            log_path: None,
            quarantine_path: None,
        };
        let qid = insert_job(&conn, &job).unwrap();
        job.state = "running".into();
        let rid = insert_job(&conn, &job).unwrap();
        assert_eq!(
            cancel_queued_jobs(&conn, fid, "superseded").unwrap(),
            vec![qid]
        );
        assert_eq!(
            get_job(&conn, qid).unwrap().unwrap().exit_kind.as_deref(),
            Some("superseded")
        );
        assert_eq!(get_job(&conn, qid).unwrap().unwrap().state, "failed");
        assert_eq!(get_job(&conn, rid).unwrap().unwrap().state, "running");
        assert!(has_running_job(&conn, fid).unwrap());
        assert!(!has_running_job(&conn, fid + 1000).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_file_eval_is_scoped_to_id_and_path() {
        let (dir, conn) = temp_db("touch");
        let lib = insert_library(&conn, &lib_row()).unwrap();
        let fid = upsert_file(&conn, &file_row(lib, "/l/a.mkv")).unwrap();
        assert_eq!(
            touch_file_eval(&conn, fid, "/l/a.mkv", "compliant", Some(5)).unwrap(),
            1
        );
        // Row renamed in place (job adoption): the old path no longer matches,
        // so a stale snapshot write is dropped instead of ghosting a new row.
        rename_file_path(&conn, fid, "/l/a.mp4").unwrap();
        assert_eq!(
            touch_file_eval(&conn, fid, "/l/a.mkv", "unmatched", None).unwrap(),
            0
        );
        assert_eq!(get_file(&conn, fid).unwrap().unwrap().status, "compliant");
        assert_eq!(
            touch_file_eval(&conn, fid, "/l/a.mp4", "completed", Some(6)).unwrap(),
            1
        );
        let f = get_file(&conn, fid).unwrap().unwrap();
        assert_eq!(f.status, "completed");
        assert_eq!(f.last_evaluated, Some(6));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn old_library_schema_migrates_to_flows() {
        let dir = std::env::temp_dir().join(format!(
            "transcodarr-db-test-migrate-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Build a pre-flows database by hand.
        {
            let conn = rusqlite::Connection::open(dir.join("transcodarr.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE libraries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    path TEXT NOT NULL,
                    lifecycle_mode TEXT NOT NULL DEFAULT 'manual',
                    flow_json TEXT NOT NULL DEFAULT '{}',
                    auto_queue INTEGER NOT NULL DEFAULT 1,
                    retention_days INTEGER NOT NULL DEFAULT 7,
                    auto_delete INTEGER NOT NULL DEFAULT 0,
                    scan_schedule TEXT,
                    watchable INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
            let default = r#"{"flow_version":1,"steps":[]}"#;
            conn.execute(
                "INSERT INTO libraries (name, path, flow_json) VALUES ('A', '/a', ?1)",
                rusqlite::params![default],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO libraries (name, path, flow_json) VALUES ('B', '/b', ?1)",
                rusqlite::params!["{}"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO libraries (name, path, flow_json) VALUES ('C', '/c', ?1)",
                rusqlite::params![r#"{"flow_version":1,"steps":[{"condition":{}}]}"#],
            )
            .unwrap();
        }
        // Reopening runs the migration.
        let conn = open(&dir).unwrap();
        // "{}" and the full empty flow are both "default" and dedupe to
        // one row; the custom flow becomes "Imported 1" (sort order puts
        // it first).
        let flows = list_flows(&conn).unwrap();
        assert_eq!(flows.len(), 2);
        let lib_a = get_library(&conn, 1).unwrap().unwrap();
        assert_eq!(lib_a.flow_name.as_deref(), Some("Default"));
        let lib_b = get_library(&conn, 2).unwrap().unwrap();
        assert_eq!(lib_b.flow_id, None); // its "{}" deduped into A's Default
        let lib_c = get_library(&conn, 3).unwrap().unwrap();
        assert!(
            lib_c
                .flow_name
                .as_deref()
                .is_some_and(|n| n.starts_with("Imported ")),
            "custom flow should be imported, got {:?}",
            lib_c.flow_name
        );
        let has_old = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('libraries')
                 WHERE name = 'flow_json'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap();
        assert!(!has_old, "flow_json column should be dropped");
        // A fresh open must be a no-op migration.
        drop(conn);
        let _ = open(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flow_crud_and_library_reference() {
        let (dir, conn) = temp_db("flows");
        let fid = insert_flow(&conn, "F", r#"{"flow_version":1,"steps":[]}"#).unwrap();
        let mut lib = lib_row();
        lib.flow_id = Some(fid);
        let lid = insert_library(&conn, &lib).unwrap();
        let got = get_library(&conn, lid).unwrap().unwrap();
        assert_eq!(got.flow_name.as_deref(), Some("F"));
        assert_eq!(get_flow(&conn, fid).unwrap().unwrap().library_count, 1);
        update_flow(&conn, fid, "F2", r#"{"flow_version":1,"steps":[]}"#).unwrap();
        assert_eq!(get_flow(&conn, fid).unwrap().unwrap().name, "F2");
        delete_flow(&conn, fid).unwrap();
        let got = get_library(&conn, lid).unwrap().unwrap();
        assert_eq!(got.flow_id, None); // ON DELETE SET NULL
        assert_eq!(got.flow_name, None);
        let json = flow_json_for_library(&conn, &got).unwrap();
        assert_eq!(json, r#"{"flow_version":1,"steps":[]}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn job_log_zstd_round_trip_and_column_migration() {
        // Fresh databases already carry the column.
        let (dir, conn) = temp_db("joblog");
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('jobs') WHERE name = 'log_zstd'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        assert!(has);

        // Round trip: compress a realistic log, store it, read it
        // back, decompress, compare.
        let lib = insert_library(&conn, &lib_row()).unwrap();
        let fid = upsert_file(&conn, &file_row(lib, "/l/a.mkv")).unwrap();
        let job = JobRow {
            id: 0,
            file_id: fid,
            library_id: lib,
            flow_version: 1,
            plan_json: "{}".into(),
            state: "queued".into(),
            device_id: None,
            claimed_by: None,
            lease_expires: None,
            started: None,
            ended: None,
            exit_kind: None,
            log_path: None,
            quarantine_path: None,
        };
        let jid = insert_job(&conn, &job).unwrap();
        assert!(get_job_log_zstd(&conn, jid).unwrap().is_none());
        let raw: String = (0..500)
            .map(|i| format!("frame={i} pts={i}.000\n"))
            .collect();
        let compressed = zstd::encode_all(std::io::Cursor::new(raw.as_bytes()), 3).unwrap();
        set_job_log_zstd(&conn, jid, &compressed).unwrap();
        let got = get_job_log_zstd(&conn, jid).unwrap().unwrap();
        assert!(got.len() < raw.len()); // it actually compressed
        let decoded = zstd::decode_all(std::io::Cursor::new(&*got)).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), raw);
        let _ = std::fs::remove_dir_all(&dir);

        // Pre-archive databases: a jobs table without the column is
        // migrated on open.
        let dir2 = std::env::temp_dir().join(format!(
            "transcodarr-db-test-oldjoblog-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).unwrap();
        {
            let c = Connection::open(dir2.join(DB_FILE)).unwrap();
            c.execute_batch(
                "CREATE TABLE jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_id INTEGER NOT NULL,
                    library_id INTEGER NOT NULL,
                    flow_version INTEGER NOT NULL,
                    plan_json TEXT NOT NULL,
                    state TEXT NOT NULL DEFAULT 'queued',
                    device_id TEXT,
                    claimed_by TEXT,
                    lease_expires INTEGER,
                    started INTEGER,
                    ended INTEGER,
                    exit_kind TEXT,
                    log_path TEXT,
                    quarantine_path TEXT
                );",
            )
            .unwrap();
        }
        let c2 = open(&dir2).unwrap(); // triggers migrate_job_logs
        let has2: bool = c2
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('jobs') WHERE name = 'log_zstd'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        assert!(has2);
        drop(c2);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn applied_filters_round_trip_and_column_migration() {
        // Fresh databases already carry the column.
        let (dir, conn) = temp_db("applied");
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('files') WHERE name = 'applied_filters'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        assert!(has);

        // Round trip: add is idempotent, get returns the ledger, set
        // replaces, and an empty set clears to NULL.
        let lid = insert_library(&conn, &lib_row()).unwrap();
        let fid = upsert_file(
            &conn,
            &FileRow {
                id: 0,
                library_id: lid,
                path: "/m/a.mkv".into(),
                dev: 1,
                inode: 2,
                size: 3,
                mtime: 4,
                sample_hash: None,
                facts_json: None,
                status: "unscanned".into(),
                last_probed: None,
                last_evaluated: None,
                input_size: None,
                output_size: None,
            },
        )
        .unwrap();
        assert_eq!(get_applied_filters(&conn, fid).unwrap(), None);
        add_applied_filters(&conn, fid, 111, &["crop=1:1".into()]).unwrap();
        add_applied_filters(&conn, fid, 222, &["volume=2".into(), "crop=1:1".into()]).unwrap();
        let got = get_applied_filters(&conn, fid).unwrap().unwrap();
        assert_eq!(got.hash, 222); // the newest application wins
        assert_eq!(
            got.graphs,
            vec!["crop=1:1".to_string(), "volume=2".to_string()]
        );
        set_applied_filters(
            &conn,
            fid,
            Some(&AppliedFilters {
                hash: 333,
                graphs: vec!["loudnorm=I=16".into()],
            }),
        )
        .unwrap();
        let got = get_applied_filters(&conn, fid).unwrap().unwrap();
        assert_eq!(got.hash, 333);
        assert_eq!(got.graphs, vec!["loudnorm=I=16".to_string()]);
        set_applied_filters(&conn, fid, None).unwrap();
        assert_eq!(get_applied_filters(&conn, fid).unwrap(), None);

        // A pre-ledger database (files table without the column) gains
        // it on open, with no data expectations.
        let dir2 = std::env::temp_dir().join(format!(
            "transcodarr-db-test-applied2-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).unwrap();
        {
            let c = rusqlite::Connection::open(dir2.join("transcodarr.db")).unwrap();
            c.execute_batch(
                "CREATE TABLE files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    library_id INTEGER NOT NULL,
                    path TEXT NOT NULL UNIQUE,
                    dev INTEGER NOT NULL,
                    inode INTEGER NOT NULL,
                    size INTEGER NOT NULL DEFAULT 0,
                    mtime INTEGER NOT NULL DEFAULT 0,
                    sample_hash INTEGER,
                    facts_json TEXT,
                    status TEXT NOT NULL DEFAULT 'unscanned',
                    last_probed INTEGER,
                    last_evaluated INTEGER,
                    input_size INTEGER,
                    output_size INTEGER);
                INSERT INTO files (library_id, path, dev, inode) VALUES (1, '/m/b.mkv', 1, 2);",
            )
            .unwrap();
        }
        let c2 = open(&dir2).unwrap();
        let has2: bool = c2
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('files') WHERE name = 'applied_filters'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        assert!(has2);
        // The pre-existing row reads as "no filters applied".
        let gid: i64 = c2
            .query_row("SELECT id FROM files WHERE path = '/m/b.mkv'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(get_applied_filters(&c2, gid).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }
}
