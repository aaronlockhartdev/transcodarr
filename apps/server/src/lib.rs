//! Serves the built SPA, and answers 404 rather than HTML under `/api/*`.
//!
//! No `/api/*` path has a handler yet, and `contracts/openapi.json` asserts no paths. A request below
//! `/api/` must not receive `index.html`: an HTML body behind a request that expected JSON is how a
//! broken client ships silently, which is what the split below is for.
//! Caching is decided here as well: hashed build assets are immutable, everything else revalidates.

use std::path::PathBuf;

use axum::Router;
use axum::extract::Request;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::any;
use tower_http::services::{ServeDir, ServeFile};

/// Names under `/_app/immutable/**` are content hashes, so a name never changes bytes. The unhashed
/// `/_app/version.json` sits beside that directory with a fixed name, which is where the prefix stops.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The shell references chunk names from the build that shipped it, so a cached shell after an upgrade
/// asks for chunks that no longer exist. Revalidate, and `etag` makes the revalidation a 304.
const REVALIDATE: &str = "no-cache";

/// The answer for a hashed asset that is absent. SvelteKit uses this value for the same case, with the
/// comment that some adapters cache by default, so it is copied rather than invented.
const MUST_REVALIDATE: &str = "public, max-age=0, must-revalidate";

/// Routes: `/api/*` is refused, every other path is the built app with SPA fallback, so a deep link gets the
/// shell and the app's own router decides what it means. The server knows no app routes, and names none here.
pub fn app(web_build: PathBuf) -> Router {
    let spa = ServeDir::new(&web_build).fallback(ServeFile::new(web_build.join("index.html")));

    Router::new()
        // `nest` matches `/api` and `/api/anything`, but hands the inner router an empty remainder
        // for the bare `/api/`, which falls through to the SPA. The boundary form gets its own route.
        .route("/api/", any(no_handler_yet))
        .nest("/api", Router::new().fallback(no_handler_yet))
        // The hashed prefix is `ServeDir` with no SPA fallback, so a chunk this build does not have is a
        // 404 rather than the shell served as JavaScript (#25). `nest` hands the inner router the target
        // with the prefix removed, so the directory here is the prefix's own directory, not the build root.
        .nest(
            "/_app/immutable",
            Router::new().fallback_service(ServeDir::new(web_build.join("_app/immutable"))),
        )
        .fallback_service(spa)
        .layer(middleware::from_fn(cache_control))
}

/// No handler exists for any `/api/*` path yet, so the answer is a status and nothing else. The body is
/// what a client reads, so there is nothing here to read. ADR-0003 owns the workflow for changing this.
async fn no_handler_yet() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// `Cache-Control` by path class, because nothing else sets it. `ServeDir` adds no caching opinion, and
/// adapter-static's `_headers` file is not something a file server reads. `or_insert_with` so a response
/// that already carries the header keeps its own verdict.
///
/// A miss under `/_app/immutable/` gets an opinion too, not silence: leaving the header off leaves a cache
/// free to apply heuristic freshness to the 404, which is how a chunk that exists again after an upgrade
/// stays missing. A 404 elsewhere keeps the header-free answer the `/api` boundary wants.
async fn cache_control(req: Request, next: Next) -> Response {
    let hashed = req.uri().path().starts_with("/_app/immutable/");
    let mut res = next.run(req).await;
    let served = res.status().is_success() || res.status().is_redirection();
    let value = if hashed {
        if served { IMMUTABLE } else { MUST_REVALIDATE }
    } else if served {
        REVALIDATE
    } else {
        return res;
    };
    res.headers_mut()
        .entry(CACHE_CONTROL)
        .or_insert_with(|| HeaderValue::from_static(value));
    res
}
