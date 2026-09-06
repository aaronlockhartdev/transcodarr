//! The HTTP API (DESIGN §8, §13): flows, libraries, files, jobs,
//! devices, settings, the flow schema, the live-update stream, and
//! the embedded-frontend SPA fallback.
//!
//! All state access goes through [`Db`](crate::dbhandle::Db) — a
//! fresh short-lived connection per operation (WAL mode; see that
//! module for why a shared `Connection` is impossible in async).

use std::sync::{Arc, atomic::AtomicUsize};
/// The SSE `Last-Event-ID` request header (this axum/http version
/// has no constant for it).
const LAST_EVENT_ID: axum::http::HeaderName = axum::http::HeaderName::from_static("last-event-id");

use axum::extract::{OriginalUri, Path as PathParam, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use transcodarr_core::device::Device;
use transcodarr_core::evaluate::FLOW_VERSION;
use transcodarr_core::flow::Flow;
use transcodarr_core::registry::Registry;

use crate::db;
use crate::dbhandle::Db;
use crate::events::{self, EventBus, ServerEvent, SseStream};
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
    pub in_flight: Arc<AtomicUsize>,
    pub events: Arc<EventBus>,
}

/// Build the router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/schema/flow", get(schema_flow))
        .route("/api/devices", get(list_devices))
        .route("/api/flows", get(list_flows).post(create_flow))
        .route(
            "/api/flows/{id}",
            get(get_flow).put(update_flow).delete(delete_flow),
        )
        .route("/api/libraries", get(list_libraries).post(create_library))
        .route(
            "/api/libraries/{id}",
            get(get_library).put(update_library).delete(delete_library),
        )
        .route("/api/libraries/{id}/files", get(list_files))
        .route("/api/libraries/{id}/scan", post(scan))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/log", get(job_log))
        .route("/api/events", get(sse_events))
        .route("/api/settings/{key}", get(get_setting).put(set_setting))
        .with_state(state)
        .fallback(spa)
}

// ── Handlers ──────────────────────────────────────────────────────────

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "flow_version": FLOW_VERSION,
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("BUILD_TIME"),
    }))
}

/// `/api/schema/flow` — the single source of truth for the flow
/// editor (DESIGN §5).
async fn schema_flow(State(s): State<AppState>) -> Json<Value> {
    Json(transcodarr_core::schema::flow_schema(
        &s.registry,
        &s.devices,
    ))
}

async fn list_devices(State(s): State<AppState>) -> Json<Vec<Device>> {
    Json(s.devices.clone())
}

// Libraries ─────────────────────────────────────────────────────────

/// `flow_id` needs three states over JSON: omitted, present `null`, or a
/// number. Plain serde collapses omitted and `null` into the same `None` for
/// `Option<..>`, so present fields go through this deserializer (re-wrapping
/// in `Some`) while `#[serde(default)]` supplies the omitted-field case,
/// which `deserialize_with` never sees.
fn opt_opt_i64<'de, D>(d: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(d)?))
}

#[derive(Deserialize)]
struct LibraryBody {
    name: String,
    path: String,
    /// `None` = keep the current assignment (on update) / no flow (on
    /// create); `Some(None)` = explicitly unassign (JSON `null`);
    /// `Some(Some(id))` = use that flow.
    #[serde(default, deserialize_with = "opt_opt_i64")]
    flow_id: Option<Option<i64>>,
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

/// Validate a flow's JSON envelope and version before persisting it.
fn validate_flow(flow_json: &str) -> Result<(), ApiError> {
    let flow: Flow = serde_json::from_str(flow_json)
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("bad flow: {e}")))?;
    if flow.flow_version != FLOW_VERSION {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "flow version {} unsupported (this build: {FLOW_VERSION})",
                flow.flow_version
            ),
        ));
    }
    Ok(())
}

/// A library referencing a flow that doesn't exist.
fn ensure_flow_exists(s: &AppState, flow_id: i64) -> Result<(), ApiError> {
    let found = s.db.with(|c| db::get_flow(c, flow_id))?.is_some();
    if !found {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("flow {flow_id} not found"),
        ));
    }
    Ok(())
}

/// Best-effort re-evaluation of one library (fire and forget; the
/// scan loop would pick the change up anyway).
async fn spawn_reeval(s: &AppState, library_id: i64) {
    let probe = s.probe.clone();
    let registry = s.registry.clone();
    let db2 = s.db.clone();
    let ffprobe2 = s.ffprobe.clone();
    let events2 = s.events.clone();
    let _ = tokio::task::spawn_blocking(move || {
        crate::jobs::scan_library(
            &db2, library_id, &registry, &probe, &ffprobe2, true, &events2,
        )
    })
    .await;
}

async fn list_libraries(State(s): State<AppState>) -> Result<Json<Vec<db::LibraryRow>>, ApiError> {
    let libs = s.db.with(db::list_libraries)?;
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
    // The flow is a first-class object; a library only references it,
    // and must point at an existing one (it was validated at save).
    let flow_id = body.flow_id.and_then(|f| f);
    if let Some(fid) = flow_id {
        ensure_flow_exists(&s, fid)?;
    }
    let row = db::LibraryRow {
        id: 0,
        name: body.name,
        path: body.path,
        lifecycle_mode: body.lifecycle_mode.unwrap_or_else(|| "manual".into()),
        flow_id,
        flow_name: None, // display-only; the list/get join fills it
        auto_queue: body.auto_queue.unwrap_or(true),
        retention_days: body.retention_days.unwrap_or(7),
        auto_delete: body.auto_delete.unwrap_or(false),
        scan_schedule: body.scan_schedule,
        watchable: body.watchable.unwrap_or(false),
    };
    let id = s.db.with(|c| db::insert_library(c, &row))?;
    s.events
        .emit(ServerEvent::LibraryChanged { library_id: id });
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn update_library(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
    Json(body): Json<LibraryBody>,
) -> Result<StatusCode, ApiError> {
    let existing =
        s.db.with(|c| db::get_library(c, id))?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "library not found".into()))?;
    // `None` in the body = keep the current assignment.
    let flow_id = match body.flow_id {
        None => existing.flow_id,
        Some(None) => None,
        Some(Some(fid)) => {
            ensure_flow_exists(&s, fid)?;
            Some(fid)
        }
    };
    let row = db::LibraryRow {
        id,
        name: body.name,
        path: body.path,
        lifecycle_mode: body.lifecycle_mode.unwrap_or_else(|| "manual".into()),
        flow_id,
        flow_name: existing.flow_name, // display-only; not written
        auto_queue: body.auto_queue.unwrap_or(true),
        retention_days: body.retention_days.unwrap_or(7),
        auto_delete: body.auto_delete.unwrap_or(false),
        scan_schedule: body.scan_schedule,
        watchable: body.watchable.unwrap_or(false),
    };
    // Any library change re-evaluates everything (a flow swap is the
    // common reason, and the check is cheap).
    s.db.with(|c| db::update_library(c, &row))?;
    s.events
        .emit(ServerEvent::LibraryChanged { library_id: id });
    spawn_reeval(&s, id).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_library(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<StatusCode, ApiError> {
    s.db.with(|c| db::delete_library(c, id))?;
    s.events
        .emit(ServerEvent::LibraryChanged { library_id: id });
    Ok(StatusCode::NO_CONTENT)
}

// Flows ─────────────────────────────────────────────────────────

/// Flows are library-independent (DESIGN §2): any number of
/// libraries can point at the same flow.
#[derive(Deserialize)]
struct FlowBody {
    name: String,
    flow_json: String,
}

async fn list_flows(State(s): State<AppState>) -> Result<Json<Vec<db::FlowRow>>, ApiError> {
    Ok(Json(s.db.with(db::list_flows)?))
}

async fn create_flow(
    State(s): State<AppState>,
    Json(body): Json<FlowBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    validate_flow(&body.flow_json)?;
    let id =
        s.db.with(|c| db::insert_flow(c, &body.name, &body.flow_json))?;
    s.events.emit(ServerEvent::FlowChanged { flow_id: id });
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn get_flow(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<Json<db::FlowRow>, ApiError> {
    let flow = s.db.with(|c| db::get_flow(c, id))?;
    let Some(flow) = flow else {
        return Err(ApiError(StatusCode::NOT_FOUND, "flow not found".into()));
    };
    Ok(Json(flow))
}

async fn update_flow(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
    Json(body): Json<FlowBody>,
) -> Result<StatusCode, ApiError> {
    let Some(existing) = s.db.with(|c| db::get_flow(c, id))? else {
        return Err(ApiError(StatusCode::NOT_FOUND, "flow not found".into()));
    };
    validate_flow(&body.flow_json)?;
    let users = s.db.with(|c| db::libraries_using_flow(c, id))?;
    s.db.with(|c| db::update_flow(c, id, &body.name, &body.flow_json))?;
    s.events.emit(ServerEvent::FlowChanged { flow_id: id });
    // A changed flow re-evaluates every library that uses it.
    if existing.name != body.name || existing.flow_json != body.flow_json {
        for lib_id in users {
            spawn_reeval(&s, lib_id).await;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_flow(
    State(s): State<AppState>,
    PathParam(id): PathParam<i64>,
) -> Result<StatusCode, ApiError> {
    let Some(_) = s.db.with(|c| db::get_flow(c, id))? else {
        return Err(ApiError(StatusCode::NOT_FOUND, "flow not found".into()));
    };
    let users = s.db.with(|c| db::libraries_using_flow(c, id))?;
    // ON DELETE SET NULL leaves the libraries with no flow;
    // re-evaluate so their file states settle to unmatched.
    s.db.with(|c| db::delete_flow(c, id))?;
    s.events.emit(ServerEvent::FlowChanged { flow_id: id });
    for lib_id in users {
        spawn_reeval(&s, lib_id).await;
    }
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
    let events2 = s.events.clone();
    let (scanned, queued) = tokio::task::spawn_blocking(move || {
        crate::jobs::scan_library(&db2, id, &registry, &probe, &ffprobe2, false, &events2)
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
    // The archived (zstd) log is the canonical copy; pre-archive
    // rows fall back to the on-disk file (DESIGN §9.4).
    let text = match s.db.with(|c| db::get_job_log_zstd(c, id))? {
        Some(blob) => Some(
            zstd::decode_all(std::io::Cursor::new(&*blob))
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default(),
        ),
        None => match &job.log_path {
            Some(p) => std::fs::read_to_string(s.data_dir.join(p)).ok(),
            None => None,
        },
    };
    match text {
        Some(text) => {
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
            h.insert(
                header::CONTENT_TYPE,
                "text/plain; charset=utf-8".parse().unwrap(),
            );
            Ok((StatusCode::OK, h, tail))
        }
        None => Ok((
            StatusCode::NOT_FOUND,
            HeaderMap::new(),
            "no log for this job yet".into(),
        )),
    }
}

// Live updates (SSE) ───────────────────────────────────────────────

/// `/api/events` — one SSE connection per client (DESIGN §9.0).
///
/// The browser's `EventSource` reconnects automatically and echoes the
/// last stream id it saw back as `Last-Event-ID`; the bus replays the
/// retained ring (up to [`events::RING_CAP`] events) before joining the
/// live feed, so a dropped connection resumes without losing updates.
/// Initial state still comes from the regular JSON endpoints — this
/// stream carries deltas only.
async fn sse_events(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<events::SseResponse, ApiError> {
    let last = headers
        .get(LAST_EVENT_ID)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let rx = s.events.subscribe();
    let replay = s.events.replay_since(last).into_iter().collect();
    let stream = SseStream::new(rx, replay);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(events::KEEPALIVE)))
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

mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

/// Serve embedded frontend assets; unknown non-API paths fall back to
/// `index.html` (client-side routing).
async fn spa(OriginalUri(url): OriginalUri) -> Response {
    serve_asset(url.path())
}

fn serve_asset(path: &str) -> Response {
    let clean = path.strip_prefix('/').unwrap_or(path);
    // Exact match first, then index.html for client-side routes.
    let asset = assets::ASSETS.iter().find(|a| a.path == clean).or_else(|| {
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
        (
            self.0,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The `flow_id` wire tri-state: omitted = keep, present null =
    /// unassign, present number = assign. The unassign case is the one
    /// plain serde silently breaks (reviewed P0), so pin it here.
    #[test]
    fn library_body_flow_id_tri_state() {
        let b: LibraryBody = serde_json::from_str("{\"name\":\"n\",\"path\":\"/p\"}").unwrap();
        assert_eq!(b.flow_id, None);

        let b: LibraryBody =
            serde_json::from_str("{\"name\":\"n\",\"path\":\"/p\",\"flow_id\":null}").unwrap();
        assert_eq!(b.flow_id, Some(None));

        let b: LibraryBody =
            serde_json::from_str("{\"name\":\"n\",\"path\":\"/p\",\"flow_id\":3}").unwrap();
        assert_eq!(b.flow_id, Some(Some(3)));
    }
}
