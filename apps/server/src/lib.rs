//! Serves the built SPA, and answers 404 rather than HTML under `/api/*`.
//!
//! No `/api/*` path has a handler yet, and `contracts/openapi.json` asserts no paths. A request below
//! `/api/` must not receive `index.html`: an HTML body behind a request that expected JSON is how a
//! broken client ships silently, which is what the split below is for.

use std::path::PathBuf;

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
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
        // Over the whole router, so it answers before `nest`, before the boundary route, and before
        // `ServeDir`. Refusing inside a layer instead leaves the answer to whichever layer noticed.
        .layer(middleware::from_fn(reject_non_normal_targets))
}

/// Answers an empty 404 for a request target that is not in normal form: an empty segment (`//`), a
/// segment that *is* `.` or `..` in any spelling, or a percent-encoded separator (`%2f`, `%5c`).
/// Before this, `/%2e%2e/etc/passwd` returned the app shell with a 200 while `/x/%2e%2e/api`
/// returned an empty 404: the same shape, two answers, because each inner layer had its own rule.
/// One rule at the edge removes the disagreement instead of documenting it.
///
/// What it allows, on purpose. `%61pi` and `API` are valid spellings of *different* paths that a
/// case-sensitive lookup legitimately misses. `movie%2e2024.mkv` has its dot inside a name. A
/// trailing `/` is a route for the app's own router, not a defect. And overlong UTF-8 (`%c0%ae`)
/// does not decode to a dot under the decoder in use, so it arrives as a miss rather than as a
/// traversal, which is why it is not on the rejection list.
async fn reject_non_normal_targets(req: Request, next: Next) -> Response {
    if is_normal_target(req.uri().path()) {
        next.run(req).await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// The target as sent: `Uri::path()` keeps percent-escapes intact, so this compares spellings and
/// never meanings. Deciding on the decoded form would put the check after the ambiguity it exists
/// to remove.
fn is_normal_target(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    // A decoded `/` cannot be a filename anywhere we serve, so the encoded separators cost nothing
    // to refuse, and `//` is an empty segment by any reading.
    if lowered.contains("%2f") || lowered.contains("%5c") || path.contains("//") {
        return false;
    }
    // The element before the leading `/` is always empty, and a trailing `/` is a route (`/api/` is
    // one), so only an interior empty segment counts. That is what `//` above already caught.
    path.split('/')
        .skip(1)
        .all(|segment| !is_dot_segment(segment))
}

/// True when the whole segment is a dot segment: `.`, `..`, or either one spelled with encoded dots
/// in any hex case or mixed with literal dots. `...` is a filename and stays legal.
fn is_dot_segment(spelling: &str) -> bool {
    let bytes = spelling.as_bytes();
    let mut dots = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'.' => {
                dots += 1;
                i += 1;
            }
            b'%' if encoded_dot_at(bytes, i) => {
                dots += 1;
                i += 3;
            }
            _ => return false,
        }
    }
    dots == 1 || dots == 2
}

/// One percent-encoded `.` (`%2e` or `%2E`) starting at `i`.
fn encoded_dot_at(bytes: &[u8], i: usize) -> bool {
    bytes.get(i..i + 3).is_some_and(|s| {
        s[0] == b'%' && s[1].eq_ignore_ascii_case(&b'2') && s[2].eq_ignore_ascii_case(&b'e')
    })
}

/// No handler exists for any `/api/*` path yet, so the answer is a status and nothing else. The body is
/// what a client reads, so there is nothing here to read. ADR-0003 owns the workflow for changing this.
async fn no_handler_yet() -> StatusCode {
    StatusCode::NOT_FOUND
}
