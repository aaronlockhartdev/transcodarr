//! The HTTP API (DESIGN §8, §13): libraries, files, jobs, devices,
//! settings, the flow schema, and the embedded-frontend SPA fallback.
//!
//! All state access goes through [`Db`](crate::dbhandle::Db) — a
//! fresh short-lived connection per operation (WAL mode; see that
//! module for why a shared `Connection` is impossible in async).

use std::sync::Arc;

use axum::extract::{OriginalUri, Path as PathParam, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use transcodarr_core::device::Device;
use transcodarr_core::evaluate::FLOW_VERSION;
use transcodarr_core::flow::Flow;
use transcodarr_core::registry::Registry;

use crate::db;
use crate::dbhandle::Db;
use crate::probe::FfprobeFactExtractor;

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub registry: Arc<Registry>,
    pub probe: FfprobeFactExtractor,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub data_dir: std::path::PathBuf,
    pub devices: Vec<Device>,
}

/// Build the router.
#[must_use]
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/schema/flow", get(schema_flow))
        .route("/api/devices", get(list_devices))
        .route("/api/libraries", get(list_libraries).post(create_library))
        .route(
            "/api/libraries/:id",
            get(get_library).put(update_library).delete(delete_library),
        )
        .route(
            "/api/libraries/:id/files",
            get(list_files),
        )
        .route("/api/libraries/:id/scan", post(scan))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/:id", get(get_job))
        .route("/api/jobs/:id/log", get(job_log))
        .route("/api/settings/:key", get(get_setting).put(set_setting))
        .with_state(state)
        .fallback(spa)
}

// ── Handlers ──────────────────────────────────────────────────────────

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "flow_version": FLOW_VERSION,
    }))
}

/// `/api/schema/flow` — the single source of truth for the flow
/// editor (DESIGN §5).
async fn schema_flow(State(s): State<AppState>) -> Json<Value> {
    Json(transcodarr_core::schema::flow_schema(&s.registry, &s.devices))
}

async fn list_devices(State(s): State<AppState>) -> Json<Vec<Device>> {
    Json(s.devices.clone())
}

// Libraries ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LibraryBody {
    name: String,
    path: String,
    #[serde(default)]
    flow_json: Option<String>,
    #[serde(default)]
    lifecycle_mode: Option<String>,
    #[serde(default)]
    auto_queue: Option<bool>,
    #[serde(default)]
    retention_days: Option<i64>,
    #[serde(default)]
    auto_delete: Option<bool>,
    scan_schedule: Option<String>,
    #[serde(default)]
    watchable: Option<bool>,
}

fn default_flow_json() -> String {
    r#"{"flow_version":1,"steps":[]}"#.into()
}

async fn list_libraries(State(s): State<AppState>) -> Result<Json<Vec<db::LibraryRow>>, ApiError> {
    let libs = s.db.with(|c| db::list_libraries(c))?;
    Ok(Json(libs))
}

async fn get_library(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<Json<db::LibraryRow>, ApiError> {
    let lib = s.db.with(|c| db::get_library(c, id))?;
    let Some(lib) = lib else {
        return Err(ApiError(StatusCode::NOT_FOUND, "library not found".into()));
    };
    Ok(Json(lib))
}

async fn create_library(
    State(s): State<AppState>,
    Json(body): Json<LibraryBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    // Validate the flow up front: a library with a broken flow is a
    // foot-gun, and the error belongs here, not at first scan.
    let flow_json = body.flow_json.clone().unwrap_or_else(default_flow_json);
    let flow: Flow = serde_json::from_str(&flow_json)
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("bad flow: {e}")))?;
    if flow.flow_version != FLOW_VERSION {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("flow version {} unsupported (this build: {FLOW_VERSION})", flow.flow_version),
        ));
    }
    let row = db::LibraryRow {
        id: 0,
        name: body.name,
        path: body.path,
        lifecycle_mode: body.lifecycle_mode.unwrap_or_else(|| "manual".into()),
        flow_json,
        auto_queue: body.auto_queue.unwrap_or(true),
        retention_days: body.retention_days.unwrap_or(7),
        auto_delete: body.auto_delete.unwrap_or(false),
        scan_schedule: body.scan_schedule,
        watchable: body.watchable.unwrap_or(false),
    };
    let id = s.db.with(|c| db::insert_library(c, &row))?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn update_library(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
    Json(body): Json<LibraryBody>,
) -> Result<StatusCode, ApiError> {
    let flow_json = body.flow_json.clone().unwrap_or_else(default_flow_json);
    let _flow: Flow = serde_json::from_str(&flow_json)
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("bad flow: {e}")))?;
    let row = db::LibraryRow {
        id,
        name: body.name,
        path: body.path,
        lifecycle_mode: body.lifecycle_mode.unwrap_or_else(|| "manual".into()),
        flow_json,
        auto_queue: body.auto_queue.unwrap_or(true),
        retention_days: body.retention_days.unwrap_or(7),
        auto_delete: body.auto_delete.unwrap_or(false),
        scan_schedule: body.scan_schedule,
        watchable: body.watchable.unwrap_or(false),
    };
    // A re-scan is implied by a flow change: evaluate everything again.
    s.db.with(|c| db::update_library(c, &row))?;
    // Best-effort immediate re-scan (the loop would pick it up anyway).
    let probe = s.probe.clone();
    let registry = s.registry.clone();
    let db2 = s.db.clone();
    let ffprobe2 = s.ffprobe.clone();
    let _ = tokio::task::spawn_blocking(move || {
        crate::jobs::scan_library(&db2, id, &registry, &probe, &ffprobe2)
    })
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_library(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<StatusCode, ApiError> {
    s.db.with(|c| db::delete_library(c, id))?;
    Ok(StatusCode::NO_CONTENT)
}

// Files / scan ──────────────────────────────────────────────────────

async fn list_files(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<Json<Vec<db::FileRow>>, ApiError> {
    let files = s.db.with(|c| db::list_files(c, id))?;
    Ok(Json(files))
}

async fn scan(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<Json<Value>, ApiError> {
    let probe = s.probe.clone();
    let registry = s.registry.clone();
    let db2 = s.db.clone();
    let ffprobe2 = s.ffprobe.clone();
    let (scanned, queued) = tokio::task::spawn_blocking(move || {
        crate::jobs::scan_library(&db2, id, &registry, &probe, &ffprobe2)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
    Ok(Json(json!({ "scanned": scanned, "queued": queued })))
}

// Jobs ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct JobsQuery {
    limit: Option<i64>,
}

async fn list_jobs(
    State(s): State<AppState>,
    Query(q): Query<JobsQuery>,
) -> Result<Json<Vec<db::JobRow>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let jobs = s.db.with(|c| db::list_jobs(c, limit))?;
    Ok(Json(jobs))
}

async fn get_job(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<Json<db::JobRow>, ApiError> {
    let job = s.db.with(|c| db::get_job(c, id))?;
    let Some(job) = job else {
        return Err(ApiError(StatusCode::NOT_FOUND, "job not found".into()));
    };
    Ok(Json(job))
}

/// The tail of a job's ffmpeg log.
async fn job_log(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<(StatusCode, HeaderMap, String), ApiError> {
    let job = s.db.with(|c| db::get_job(c, id))?;
    let Some(job) = job else {
        return Err(ApiError(StatusCode::NOT_FOUND, "job not found".into()));
    };
    let Some(p) = &job.log_path else {
        return Ok((
            StatusCode::NOT_FOUND,
            HeaderMap::new(),
            "no log for this job yet".into(),
        ));
    };
    let path = s.data_dir.join(p);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let tail = text
                .lines()
                .rev()
                .take(200)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            let mut h = HeaderMap::new();
            h.insert(header::CONTENT_TYPE, "text/plain; charset=utf-8".parse().unwrap());
            Ok((StatusCode::OK, h, tail))
        }
        Err(_) => Ok((
            StatusCode::NOT_FOUND,
            HeaderMap::new(),
            "log file missing".into(),
        )),
    }
}

// Settings ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SettingBody {
    value: String,
}

async fn get_setting(
    State(s): State<AppState>,
    PathParam(key): PathParam<String>,
) -> Result<Json<Value>, ApiError> {
    let v = s.db.with(|c| db::get_setting(c, &key))?;
    Ok(Json(json!({ "key": key, "value": v })))
}

async fn set_setting(
    State(s): State<AppState>,
    PathParam(key): PathParam<String>,
    Json(body): Json<SettingBody>,
) -> Result<StatusCode, ApiError> {
    s.db.with(|c| db::set_setting(c, &key, &body.value))?;
    Ok(StatusCode::NO_CONTENT)
}

// SPA fallback ──────────────────────────────────────────────────────

#[allow(clippy::include)]
mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

/// Serve embedded frontend assets; unknown non-API paths fall back to
/// `index.html` (client-side routing).
async fn spa(OriginalUri(url): OriginalUri) -> Response {
    serve_asset(&url.path())
}

fn serve_asset(path: &str) -> Response {
    let clean = path.strip_prefix('/').unwrap_or(path);
    // Exact match first, then index.html for client-side routes.
    let asset = assets::ASSETS
        .iter()
        .find(|a| a.path == clean)
        .or_else(|| {
            if path.starts_with("/api/") {
                None
            } else {
                assets::ASSETS.iter().find(|a| a.path == "index.html")
            }
        });
    match asset {
        Some(a) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, a.mime.to_string())],
            a.bytes.to_vec(),
        )
            .into_response(),
        None => {
            if path.starts_with("/api/") {
                (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "application/json")],
                    r#"{"error":"not found"}"#,
                )
                    .into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    "frontend not built (run `pnpm build` in frontend/ and rebuild the server)",
                )
                    .into_response()
            }
        }
    }
}

// Errors ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!("{0}", self.1);
        let body = json!({ "error": self.1 });
        (self.0, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}
