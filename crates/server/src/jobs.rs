//! Library scanning and the job runner (DESIGN §8, §13).
//!
//! `scan_library` walks a library's path, probes changed files,
//! evaluates them against the library's flow, and enqueues jobs.
//! `run_job` executes one job: ffmpeg encode to a temp file, verify,
//! then atomically swap (original quarantined). `run_loop` claims
//! queued jobs and dispatches them to a blocking thread.
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use rusqlite;
use tokio::sync::watch;

use transcodarr_core::device::{Device, DeviceKind};
use transcodarr_core::evaluate::{self, Evaluation};
use transcodarr_core::facts::FileFacts;
use transcodarr_core::flow::Flow;
use transcodarr_core::plan::FfmpegPlan;
use transcodarr_core::registry::{FactExtractor, Registry};

use crate::api::AppState;
use crate::db;
use crate::dbhandle::Db;
use crate::probe::FfprobeFactExtractor;
use crate::verify::{decode_file, probe_to_facts};

/// Job lease: a job whose lease expires without finishing is
/// reclaimable (crash resilience; the `claimed_by` field carries the
/// owner tag). A live encode keeps its lease fresh with a heartbeat
/// (see `lease_heartbeat`), so expiry means the worker is actually
/// gone.
const JOB_LEASE: i64 = 3600;

/// How often the in-flight encode refreshes its job lease.
const LEASE_HEARTBEAT_S: u64 = 60;

/// System-wide ceiling on concurrent encodes (in addition to each
/// device's own `max_concurrent`). v1 default: 4.
pub const MAX_CONCURRENT_JOBS: usize = 4;

/// Media extensions the scanner picks up (DESIGN §13.6).
const MEDIA_EXTS: &[&str] = &["mkv", "mp4", "m2ts", "ts", "mov", "avi", "webm", "m4v"];

/// Monotonic sequence for quarantine names: the same file can be
/// quarantined more than once in a single second (a crash-looping
/// job), and the name must not overwrite a previous artifact.
static QUARANTINE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A unique quarantine file name (`base.tag.nanos.seq`).
fn quarantine_name(base: &str, tag: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = QUARANTINE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{base}.{tag}.{nanos}.{seq}")
}

/// FNV-1a over the first and last 8KB — the change-detection key.
/// Cheap enough to run on every scan of every file, and stable for a
/// given content version (DESIGN §13.6: a changed sample hash
/// triggers a re-probe, which triggers a re-evaluation).
fn sample_hash(path: &Path) -> Result<u64> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let sample = 8 * 1024;
    let mut h: u64 = 0xcbf29ce484222325;
    let mut buf = [0u8; 65536];
    let mut n = f.read(&mut buf)?;
    h = fnv1a(h, &buf[..n.min(sample)]);
    if size > 2 * sample as u64 {
        f.seek(SeekFrom::End(-(sample as i64)))?;
        n = f.read(&mut buf)?;
        h = fnv1a(h, &buf[..n.min(sample)]);
    }
    Ok(h)
}

fn fnv1a(mut h: u64, data: &[u8]) -> u64 {
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(unix)]
fn file_dev_ino(path: &Path) -> Result<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    Ok((m.dev() as i64, m.ino() as i64))
}

#[cfg(not(unix))]
fn file_dev_ino(path: &Path) -> Result<(i64, i64)> {
    let m = std::fs::metadata(path)?;
    Ok((
        m.len() as i64,
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0),
    ))
}

/// Walk `library`, probe what changed, evaluate against its flow,
/// and enqueue jobs for files whose plan is non-identity.
///
/// Returns (files_scanned, jobs_queued). A file that already has a
/// live job (queued/running/verifying) is never re-queued — one job
/// per file at a time.
pub fn scan_library(
    db: &Db,
    library_id: i64,
    registry: &Registry,
    probe: &FfprobeFactExtractor,
    _ffprobe: &str,
    reevaluate: bool,
) -> Result<(u32, u32)> {
    let lib = db
        .with(|c| db::get_library(c, library_id))?
        .with_context(|| format!("library {library_id}"))?;
    let flow: Flow = serde_json::from_str(&lib.flow_json)
        .with_context(|| format!("library {} flow JSON", library_id))?;

    let root = PathBuf::from(&lib.path);
    if !root.is_dir() {
        anyhow::bail!("library path is not a directory: {}", root.display());
    }

    // One read of the library's file rows, keyed by path, for the
    // per-file change check (avoids N queries for N files).
    let files_by_path: HashMap<String, db::FileRow> = db
        .with(|c| db::list_files(c, library_id))?
        .into_iter()
        .map(|r| (r.path.clone(), r))
        .collect();

    let mut scanned: u32 = 0;
    let mut queued: u32 = 0;
    for path in walkdir(&root) {
        if !is_media(&path) {
            continue;
        }
        scanned += 1;

        // Cheap change detection: dev+inode+size+mtime first.
        let (dev, ino) = file_dev_ino(&path)?;
        let meta = path.metadata()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let size = meta.len() as i64;
        let path_str = path.display().to_string();

        let existing = files_by_path.get(&path_str).cloned();
        let unchanged = existing.as_ref().is_some_and(|e| {
            e.dev == dev
                && e.inode == ino
                && e.size == size
                && e.mtime == mtime
                && e.sample_hash != Some(0)
        });
        if unchanged {
            if !reevaluate {
                continue;
            }
            // Flow changed: re-evaluate every unchanged file with cached facts.
            if let Some(mut e) = existing {
                reevaluate_cached(db, &mut e, library_id, registry, &flow, &path)?;
            }
            continue;
        }

        let hash = sample_hash(&path).unwrap_or(0);
        let mut row = db::FileRow {
            id: 0,
            library_id,
            path: path_str,
            dev,
            inode: ino,
            size,
            mtime,
            sample_hash: Some(hash as i64),
            facts_json: None,
            status: "scanning".into(),
            last_probed: Some(mtime),
            last_evaluated: None,
            input_size: Some(size),
            output_size: None,
        };
        // `files.path` is globally UNIQUE (one library per file):
        // if another library already tracks this exact path, the
        // upsert hits the integrity check. Skip the file rather than
        // aborting the whole scan (first-registered-library-wins).
        let file_id = match db.with(|c| db::upsert_file(c, &row)) {
            Ok(id) => id,
            Err(e) => {
                let unique_violation = e
                    .root_cause()
                    .downcast_ref::<rusqlite::Error>()
                    .is_some_and(|s| {
                        matches!(
                            s,
                            rusqlite::Error::SqliteFailure(sql_err, _)
                                if sql_err.code == rusqlite::ErrorCode::ConstraintViolation
                        )
                    });
                if unique_violation {
                    tracing::warn!(
                        path = %row.path,
                        "file already tracked by another library — skipping"
                    );
                    continue;
                }
                return Err(e);
            }
        };
        row.id = file_id;

        // Probe (fresh ffprobe; the change check already told us
        // something moved).
        let facts = match probe.probe(&path) {
            Ok(f) => f,
            Err(e) => {
                let _ = db.with(|c| db::set_file_status(c, file_id, "failed"));
                tracing::warn!("probe failed for {}: {e:?}", path.display());
                continue;
            }
        };
        let facts_json = serde_json::to_string(&facts)?;
        row.facts_json = Some(facts_json);

        // Evaluate against the library's flow.
        let now = unix_now();
        let status = match evaluate::evaluate(registry, &flow, &facts) {
            Ok(Evaluation::Identity) => "compliant",
            Ok(Evaluation::Plan(plan)) => {
                // Enqueue at most one live job per file. A direct
                // query on the jobs table — correct for any history
                // depth, unlike a fixed row window.
                let has_live = db.with(|c| db::has_live_job(c, file_id))?;
                if !has_live {
                    // GPU preference is resolved at dispatch
                    // (run_loop); the job stays device-agnostic here
                    // so CPU can always take it.
                    let plan_json = serde_json::to_string(&plan)?;
                    let job = db::JobRow {
                        id: 0,
                        file_id,
                        library_id,
                        flow_version: flow.flow_version as i64,
                        plan_json,
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
                    db.with(|c| db::insert_job(c, &job))?;
                    queued += 1;
                }
                "queued"
            }
            Ok(Evaluation::NoMatch) => {
                // Never silent (DESIGN §13.6).
                "unmatched"
            }
            Err(e) => {
                tracing::warn!("evaluating {} failed: {e}", path.display());
                "failed"
            }
        };
        row.status = status.into();
        row.last_evaluated = Some(now);
        // Persist facts + status + evaluation timestamp in one write.
        let _ = db.with(|c| db::upsert_file(c, &row));
    }
    Ok((scanned, queued))
}

/// Re-evaluate an unchanged file against the current flow using its cached
/// facts (a flow change must be applied to files that have not changed).
/// Updates the row in place and persists it; None when facts are missing.
fn reevaluate_cached<'a>(
    db: &Db,
    file: &'a mut db::FileRow,
    library_id: i64,
    registry: &Registry,
    flow: &Flow,
    path: &Path,
) -> Result<Option<&'a str>> {
    let Some(facts) = file
        .facts_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<FileFacts>(j).ok())
    else {
        return Ok(None);
    };
    let now = unix_now();
    let status: &str = match evaluate::evaluate(registry, flow, &facts) {
        Ok(Evaluation::Identity) => "compliant",
        Ok(Evaluation::Plan(plan)) => {
            if !db.with(|c| db::has_live_job(c, file.id))? {
                let job = db::JobRow {
                    id: 0,
                    file_id: file.id,
                    library_id,
                    flow_version: flow.flow_version as i64,
                    plan_json: serde_json::to_string(&plan)?,
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
                db.with(|c| db::insert_job(c, &job))?;
            }
            "queued"
        }
        Ok(Evaluation::NoMatch) => "unmatched",
        Err(err) => {
            // Evaluation error: keep the current status (don't clobber).
            tracing::warn!("re-evaluating {path:?} failed: {err}");
            file.last_evaluated = Some(now);
            return Ok(Some(file.status.as_str()));
        }
    };
    file.status = status.into();
    file.last_evaluated = Some(now);
    db.with(|c| db::upsert_file(c, file))?;
    Ok(Some(status))
}

/// Recursive directory walk (iterative, depth-first).
fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lease heartbeat for an in-flight encode: while the ffmpeg child
/// runs, the job's lease is refreshed every `LEASE_HEARTBEAT_S`.
/// A live long encode can therefore never be reclaimed by the
/// in-process reaper (which only takes jobs whose lease has lapsed),
/// while a genuinely dead worker's lease still expires and is
/// reclaimed. Stops itself when the job row is no longer claimed by
/// us (finished or re-claimed elsewhere).
///
/// The loop checks its stop flag every 5s (not only at each refresh)
/// so `run_job`'s final `join()` never stalls a finished job behind a
/// tick.
fn lease_heartbeat(db: &Db, job_id: i64, stop: &Arc<(Condvar, Mutex<bool>)>) {
    let (cvar, lock) = &**stop;
    let mut stopped = lock.lock().unwrap();
    let mut ticks: u64 = 0;
    loop {
        stopped = cvar.wait_timeout(stopped, std::time::Duration::from_secs(5)).unwrap().0;
        if *stopped {
            return;
        }
        ticks += 1;
        if ticks * 5 < LEASE_HEARTBEAT_S {
            continue;
        }
        ticks = 0;
        let refreshed = db
            .with(|c| db::refresh_job_lease(c, job_id, "transcodarr", unix_now() + JOB_LEASE))
            .unwrap_or(false);
        if !refreshed {
            return;
        }
    }
}

/// Execute one job: encode to a sibling temp file, verify the output
/// (metadata gate + full decode), then atomically swap (the original
/// is moved to the quarantine dir, the output is renamed onto the
/// original's path).
///
/// On verify failure or encode failure the original is untouched;
/// the failed output goes to the quarantine dir (DESIGN §13.6).
pub fn run_job(
    db: &Db,
    job: &db::JobRow,
    device: &Device,
    ffmpeg: &str,
    probe: &FfprobeFactExtractor,
    data_dir: &Path,
    registry: &Registry,
) -> Result<()> {
    let file = db
        .with(|c| db::get_file(c, job.file_id))?
        .ok_or_else(|| anyhow!("file {} not found", job.file_id))?;
    let src = PathBuf::from(&file.path);
    let input_facts: FileFacts = file
        .facts_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("parse cached facts")?
        .unwrap_or_default();

    let plan: FfmpegPlan =
        serde_json::from_str(&job.plan_json).with_context(|| format!("job {} plan", job.id))?;

    // Temp name unique per claim generation (job id + claim
    // timestamp). A re-claimed job — lease expiry with a dead worker,
    // or a process restart — must never write into a previous claim's
    // temp file: two encoders on one path is the only way this runner
    // can produce a corrupt file, and it would then be promoted over
    // the original.
    let claim_ts = unix_now();
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let dst = src.with_file_name(format!(
        "{stem}.{}.{}.{}.transcodarr-tmp",
        plan.container, job.id, claim_ts
    ));
    // Sweep temp files left by earlier claims of THIS job (a
    // crash-looping job must not accumulate a full-size orphan per
    // claim), plus our own exact path.
    if let Some(parent) = src.parent() {
        // Every temp file for this job starts with this prefix
        // (the claim timestamp comes after it).
        let old_prefix = format!("{stem}.{}.{}.", plan.container, job.id);
        let own_name = dst.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Ok(rd) = std::fs::read_dir(parent) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == own_name
                    || (name.ends_with(".transcodarr-tmp") && name.starts_with(&old_prefix) && name.get(old_prefix.len()..old_prefix.len() + 1).is_some_and(|c| c.chars().next().is_some_and(|c| c.is_ascii_digit())))
                {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    // ffmpeg stderr goes to a log file (kept on disk for the UI).
    let logs = data_dir.join("logs");
    std::fs::create_dir_all(&logs)?;
    let log_path = logs.join(format!("job-{}.log", job.id));
    if let Ok(rel) = log_path.strip_prefix(data_dir) {
        let rel = rel.display().to_string();
        let _ = db.with(|c| db::set_job_log_path(c, job.id, &rel));
    }
    let log_file =
        File::create(&log_path).with_context(|| format!("log {}", log_path.display()))?;

    let argv = transcodarr_core::plan::to_argv(&plan, device, &src, &dst);
    tracing::info!("job {} on {}: {}", job.id, device.name, argv.join(" "));

    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(&argv)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file));
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn ffmpeg: {ffmpeg}"))?;

    // Keep this job's lease alive while the encode runs (a live
    // multi-hour encode must not be reclaimed as stale).
    let stop = Arc::new((Condvar::new(), Mutex::new(false)));
    let stop_hb = stop.clone();
    let hb_db = db.clone();
    let hb_job = job.id;
    let heartbeat = std::thread::spawn(move || lease_heartbeat(&hb_db, hb_job, &stop_hb));

    let status = child.wait().with_context(|| "wait ffmpeg")?;

    *stop.1.lock().unwrap() = true;
    stop.0.notify_all();
    let _ = heartbeat.join();

    if !status.success() {
        let tail = log_tail(&log_path, 20);
        let exit_code = status.code().unwrap_or(-1);
        let _ =
            db.with(|c| db::finish_job(c, job.id, "failed", "encode_failed", unix_now(), None));
        let _ = db.with(|c| db::set_file_status(c, job.file_id, "failed"));
        let _ = std::fs::remove_file(&dst);
        anyhow::bail!("ffmpeg exit {exit_code}: {tail}");
    }

    // Verify BEFORE touching the original (DESIGN §13.6): the pure
    // metadata comparison from core (container, duration window,
    // stream inventory, codec).
    let out_facts = probe_to_facts(probe, &dst, Some(&plan.container)).with_context(|| "probe output")?;
    let ok = transcodarr_core::verify::verify_output(&plan, &input_facts, &out_facts)
        .map_err(anyhow::Error::from)?;
    if !ok {
        let _ = finish(db, job, "failed", Some("verification_failed"), Some(&dst), data_dir);
        anyhow::bail!("output metadata mismatch (output quarantined)");
    }

    // Decode integrity: the output must also decode cleanly (the
    // metadata gate alone can pass a structurally broken file).
    if !decode_file(ffmpeg, &dst).unwrap_or(false) {
        let _ = finish(db, job, "failed", Some("decode_failed"), Some(&dst), data_dir);
        anyhow::bail!("output failed the decode check (output quarantined)");
    }

    // Swap: original → quarantine, output → original path. If the
    // DESIGN §3.1: in-place mode adopts the true container extension
    // (MKV→MP4 rename), deliberately orphaning *arr DB records until
    // re-scan. Same extension → the file keeps its path.
    let src_ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
    let final_path = if src_ext.eq_ignore_ascii_case(&plan.container) {
        src.clone()
    } else {
        src.with_extension(&plan.container)
    };
    // second rename fails after the first, restore the original so
    // the file is never missing from its path (a failed restore
    // leaves both files in the quarantine dir, reported loudly).
    let quarantine = db::quarantine_dir(data_dir);
    let original_backup = quarantine.join(quarantine_name(
        src.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        "original",
    ));
    std::fs::rename(&src, &original_backup)
        .with_context(|| format!("quarantine original {}", src.display()))?;
    if let Err(e) = std::fs::rename(&dst, &final_path) {
        let restored = std::fs::rename(&original_backup, &src);
        tracing::error!("promote {} failed: {e}", dst.display());
        if restored.is_err() {
            tracing::error!(
                "CRITICAL: could not restore {} from {} — both copies are in the quarantine dir",
                src.display(),
                original_backup.display()
            );
        } else {
            tracing::error!(
                "original restored to {} (failed output kept in {})",
                src.display(),
                original_backup.display()
            );
        }
        let _ = finish(db, job, "failed", Some("swap_failed"), Some(&dst), data_dir);
        anyhow::bail!("swap failed: {e}");
    }

    // The on-disk file may now live under a new extension: move the
    // file record with it (keeps our path identity honest; external
    // *arr DBs are intentionally orphaned by the rename, DESIGN §3.1).
    if final_path != src {
        if let Err(e) = db.with(|c| db::rename_file_path(c, job.file_id, &final_path.to_string_lossy())) {
            tracing::warn!(
                "could not rename file record {} -> {} ({e}); keeping old path",
                src.display(),
                final_path.display()
            );
        }
    }

    // Idempotency gate (DESIGN §13.6): re-evaluate the NEW file. If it
    // still wants to change, the flow is not idempotent for this
    // input — mark failed (non_idempotent), never loop.
    let new_facts = probe_to_facts(probe, &final_path, None)?;
    let lib = db
        .with(|c| db::get_library(c, job.library_id))?
        .ok_or_else(|| anyhow!("library {} not found", job.library_id))?;
    let flow: Flow = serde_json::from_str(&lib.flow_json)?;
    match evaluate::evaluate(registry, &flow, &new_facts)? {
        Evaluation::Identity => {
            let _ = finish(db, job, "completed", None, None, data_dir);
            let _ = db.with(|c| db::set_file_status(c, job.file_id, "completed"));
        }
        Evaluation::Plan(_) => {
            let _ = finish(db, job, "failed", Some("non_idempotent"), None, data_dir);
            let _ = db.with(|c| db::set_file_status(c, job.file_id, "failed"));
            anyhow::bail!("flow not idempotent for {}", final_path.display());
        }
        Evaluation::NoMatch => {
            let _ = finish(db, job, "completed", None, None, data_dir);
            let _ = db.with(|c| db::set_file_status(c, job.file_id, "completed"));
        }
    }
    Ok(())
}

/// Terminal state bookkeeping: finish the job row and (if given)
/// move the failed output into the quarantine dir.
fn finish(
    db: &Db,
    job: &db::JobRow,
    state: &str,
    exit_kind: Option<&str>,
    quarantine_target: Option<&Path>,
    data_dir: &Path,
) -> Result<()> {
    let q = quarantine_target.map(|p| {
        let dir = db::quarantine_dir(data_dir);
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("output");
        let dest = dir.join(quarantine_name(name, "failed"));
        std::fs::rename(p, &dest).ok();
        dest
    });
    let q_path = q
        .as_ref()
        .and_then(|p| p.strip_prefix(data_dir).ok())
        .map(|p| p.display().to_string());
    let kind = exit_kind.unwrap_or("");
    db.with(|c| db::finish_job(c, job.id, state, kind, unix_now(), q_path.as_deref()))?;
    // The file-level state tracks the job outcome. A quarantined
    // failed OUTPUT leaves the original intact, so the file is
    // 'failed' (file-level 'quarantined' is reserved for the
    // auto-delete lifecycle).
    let file_status = match state {
        "completed" => "completed",
        _ => "failed",
    };
    db.with(|c| db::set_file_status(c, job.file_id, file_status))?;
    Ok(())
}

/// The job worker: polls for queued jobs, claims one at a time
/// (optimistic concurrency via `claim_job`), and dispatches each to
/// a blocking thread. Concurrency is capped both system-wide
/// ([`MAX_CONCURRENT_JOBS`]) and per device (`Device::max_concurrent`);
/// over-cap jobs simply stay queued. Runs until the shutdown flag is
/// set.
pub async fn run_loop(state: AppState, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("job runner: shutting down");
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
        }

        // Reap jobs whose owner is gone (worker panicked, or lease
        // expired — with the heartbeat in place, expiry here means
        // the encode process really is dead). A crash mid-encode
        // only ever leaves a sibling temp file behind, so re-running
        // is safe.
        if let Ok(n) = state.db.with(|c| db::reclaim_jobs(c, true)) {
            if n > 0 {
                tracing::info!("reclaimed {n} stale job(s)");
            }
        }

        // System-wide concurrency cap.
        if state.in_flight.load(Ordering::Relaxed) >= MAX_CONCURRENT_JOBS {
            continue;
        }

        // Pick the next job. GPU-preferred: a job without an
        // explicit device runs on the first available GPU; CPU is
        // the fallback (never the default when a GPU can take it).
        let job_opt: Option<(Device, db::JobRow)> = state.db.with(|c| {
            for d in state.devices.iter().rev() {
                // Per-device cap: never exceed what this device can
                // handle concurrently.
                let busy = db::running_jobs_count(c, Some(&d.id))?;
                if busy >= d.max_concurrent as usize {
                    continue;
                }
                if let Some(j) = db::next_queued_job(c, Some(&d.id))? {
                    return Ok(Some((d.clone(), j)));
                }
            }
            match db::next_queued_job(c, None)? {
                Some(j) => {
                    let cpu = state
                        .devices
                        .iter()
                        .find(|x| x.kind == DeviceKind::Cpu)
                        .cloned()
                        .unwrap_or_else(|| Device::cpu(1));
                    let busy = db::running_jobs_count(c, Some(&cpu.id))?;
                    if busy >= cpu.max_concurrent as usize {
                        return Ok(None);
                    }
                    Ok(Some((cpu, j)))
                }
                None => Ok(None),
            }
        })?;
        let Some((device, job)) = job_opt else {
            continue;
        };

        // Claim it (optimistic concurrency).
        let claimed = state.db.with(|c| {
            db::claim_job(
                c,
                job.id,
                &device.id,
                "transcodarr",
                unix_now() + JOB_LEASE,
                unix_now(),
            )
        })?;
        if !claimed {
            // Lost the race (another worker claimed it, or a scan
            // superseded the file).
            continue;
        }

        // Count it in flight; the spawned task decrements when done.
        state.in_flight.fetch_add(1, Ordering::Relaxed);
        let in_flight = state.in_flight.clone();
        let db2 = state.db.clone();
        let ffmpeg2 = state.ffmpeg.clone();
        let probe2 = state.probe.clone();
        let data2 = state.data_dir.clone();
        let registry2 = state.registry.clone();
        tokio::task::spawn_blocking(move || {
            let r = run_job(&db2, &job, &device, &ffmpeg2, &probe2, &data2, &registry2);
            in_flight.fetch_sub(1, Ordering::Relaxed);
            r.map_err(|e| {
                tracing::error!("job {} failed: {e:?}", job.id);
                e
            })
        });
    }
}

/// Last N lines of a log file (empty string if unreadable).
fn log_tail(path: &Path, n: usize) -> String {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| {
            t.lines()
                .rev()
                .take(n)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
