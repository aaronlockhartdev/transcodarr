//! `transcodarr` — the server binary.
//!
//! Boot sequence (DESIGN §7, §8):
//! 1. open (and migrate) the SQLite state in the data dir,
//! 2. build the registry: v1 pure entries + the ffprobe fact
//!    extractor + verification checks,
//! 3. detect encoders (one bounded test encode per candidate),
//! 4. start the job worker loop,
//! 5. serve HTTP (API + embedded frontend) until SIGINT/SIGTERM.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info, warn};

use transcodarr_core::registry::Registry;
use transcodarr_server::{api, db, dbhandle, devices, jobs, probe, verify};

#[derive(Parser)]
#[command(
    name = "transcodarr",
    version,
    about = "Local, library-based media transcoding"
)]
struct Cli {
    /// Bind address (host:port) for the HTTP server.
    #[arg(long, default_value = "127.0.0.1:8481")]
    bind: String,

    /// Data directory: SQLite DB, job logs, quarantine.
    /// Default: `$XDG_DATA_HOME/transcodarr` or `~/.local/share/transcodarr`.
    #[arg(long)]
    data_dir: Option<std::path::PathBuf>,

    /// Path to the `ffmpeg` binary (default: `ffmpeg` on `PATH`).
    #[arg(long, default_value = "ffmpeg")]
    ffmpeg: String,

    /// Path to the `ffprobe` binary (default: `ffprobe` on `PATH`).
    #[arg(long, default_value = "ffprobe")]
    ffprobe: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Logging: RUST_LOG aware, defaults to info.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let data_dir = match cli.data_dir.clone() {
        Some(p) => p,
        None => {
            let base = std::env::var("XDG_DATA_HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from))
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            base.join("transcodarr")
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        run(cli, data_dir).await
    })
}

async fn run(cli: Cli, data_dir: std::path::PathBuf) -> Result<()> {
    // 1. State (one short-lived connection per operation; see
    //    `dbhandle` for why a shared connection is impossible here).
    let dbh = dbhandle::Db::new(&data_dir)
        .with_context(|| format!("open database in {}", data_dir.display()))?;

    // 2. Registry: pure v1 entries + I/O-bound entries.
    let probe = probe::FfprobeFactExtractor::new(&cli.ffprobe);
    let registry = Arc::new({
        let mut reg = Registry::v1();
        reg.with_fact_extractor(probe.clone());
        reg.with_verification_check(verify::MetadataCheck);
        reg.with_verification_check(verify::DecodeCheck);
        reg
    });

    // 3. Device detection (bounded; a broken stack just means
    //    "no GPU device" — CPU is always present).
    let ffmpeg = cli.ffmpeg.clone();
    let detected =
        tokio::task::spawn_blocking(move || devices::detect(&ffmpeg))
            .await
            .context("device detection task")?;
    let n_gpu = detected
        .iter()
        .filter(|d| d.kind != transcodarr_core::device::DeviceKind::Cpu)
        .count();
    if n_gpu == 0 {
        warn!("no GPU encoder detected — all encodes will run on CPU");
    }
    dbh.with(|c| db::upsert_devices(c, &detected))
        .context("persist devices")?;
    info!(
        "devices: {} ({} gpu)",
        detected.len(),
        n_gpu
    );
    for d in &detected {
        info!(
            "  {:?} {:?} [{}]",
            d.kind,
            d.name,
            d.encoders.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    // Shared state (the worker gets a clone).
    let state = api::AppState {
        db: dbh,
        registry,
        probe,
        ffmpeg: cli.ffmpeg.clone(),
        ffprobe: cli.ffprobe.clone(),
        data_dir,
        devices: detected,
    };

    // 4. Job worker loop.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(jobs::run_loop(state.clone(), shutdown_rx));
    info!("job worker started");

    // 5. HTTP server.
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(&cli.bind)
        .await
        .with_context(|| format!("bind {}", cli.bind))?;
    info!("listening on http://{}", cli.bind);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown signal received");
        })
        .await
        .context("serve")?;

    // Let the worker finish its in-flight job and exit.
    let _ = shutdown_tx.send(true);
    match worker.await {
        Ok(Ok(())) => info!("worker stopped"),
        Ok(Err(e)) => warn!("worker stopped with error: {e}"),
        Err(e) => error!("worker aborted: {e}"),
    }

    Ok(())
}
