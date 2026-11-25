//! Serves the built SPA, and answers 404 rather than HTML under `/api/*`.
//!
//! No `/api/*` path has a handler yet, and `contracts/openapi.json` asserts no paths. A request below
//! `/api/` must not receive `index.html`: an HTML body behind a request that expected JSON is how a
//! broken client ships silently, which is what the split below is for.

use std::path::PathBuf;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::any;
use tower_http::services::{ServeDir, ServeFile};

/// Routes: `/api/*` is refused, every other path is the built app with SPA fallback, so a deep link gets the
/// shell and the app's own router decides what it means. The server knows no app routes, and names none here.
pub fn app(web_build: PathBuf) -> Router {
    let spa = ServeDir::new(&web_build).fallback(ServeFile::new(web_build.join("index.html")));

    Router::new()
        // `nest` matches `/api` and `/api/anything`, but hands the inner router an empty remainder
        // for the bare `/api/`, which falls through to the SPA. The boundary form gets its own route.
        .route("/api/", any(no_handler_yet))
        .nest("/api", Router::new().fallback(no_handler_yet))
        .fallback_service(spa)
}

/// No handler exists for any `/api/*` path yet, so the answer is a status and nothing else. The body is
/// what a client reads, so there is nothing here to read. ADR-0003 owns the workflow for changing this.
async fn no_handler_yet() -> StatusCode {
    StatusCode::NOT_FOUND
}
