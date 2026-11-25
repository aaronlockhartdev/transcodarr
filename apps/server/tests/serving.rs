use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::CACHE_CONTROL;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use tower::ServiceExt;

/// One function, one directory fixture. `app` takes its build directory as an argument, so the test
/// depends on neither the process environment nor a built app.
#[tokio::test]
async fn api_paths_are_refused_and_app_paths_are_served() {
    let dir = std::env::temp_dir().join(format!("transcodarr-serving-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        b"<!doctype html>\n<html lang=\"en\">the app</html>",
    )
    .unwrap();
    let app = transcodarr::app(dir.clone());

    // The contract for these paths is an empty 404. Status alone would still pass while a shell body
    // came back, and the body is what a client reads. `/api/` is the form a `nest` misses.
    for path in ["/api", "/api/", "/api/transcodes"] {
        let res = call(&app, path).await;
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "{path} must not reach the app"
        );
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert!(
            body.is_empty(),
            "{path} returned a body, not an empty 404: {body:?}"
        );
    }

    // Paths the guard does not match get the shell. These three are made up on purpose: the app has exactly
    // one route (`/`), and the guarantee is about the server not knowing app routes, so pinning a real one
    // here would imply a URL decision this unit never made.
    for path in ["/a/path/with/no/file", "/apix", "/api.js"] {
        let res = call(&app, path).await;
        assert_eq!(res.status(), StatusCode::OK, "{path} must get the shell");
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(
            &body[..],
            b"<!doctype html>\n<html lang=\"en\">the app</html>",
            "{path} returned something else"
        );
    }

    // Nothing outside the served directory may be readable through it, whatever the status code.
    // An encoded `..` lands on the SPA fallback today and answers 200 with the shell (#22), so the
    // invariant worth pinning is "no escape" rather than the status the fallback happens to return.
    let secret = format!("transcodarr-secret-{}.txt", std::process::id());
    std::fs::write(
        dir.parent().unwrap().join(&secret),
        b"this file is outside the served directory",
    )
    .unwrap();
    for path in [format!("/../{secret}"), format!("/%2e%2e/{secret}")] {
        let res = call(&app, &path).await;
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert!(
            !std::str::from_utf8(&body)
                .unwrap_or("")
                .contains("outside the served directory"),
            "{path} read a file outside the served directory: {body:?}"
        );
    }
    std::fs::remove_file(dir.parent().unwrap().join(&secret)).ok();

    std::fs::remove_dir_all(&dir).ok();
}

/// A redeploy removes chunk files. The shell names them, so the two things that decide whether an upgrade
/// breaks an open app are where the shell is cached and what a missing chunk answers (#25).
#[tokio::test]
async fn hashed_assets_are_immutable_and_a_missing_one_is_a_404() {
    let dir = std::env::temp_dir().join(format!("transcodarr-cache-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("_app/immutable/chunks")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        b"<!doctype html>\n<html lang=\"en\">the app</html>",
    )
    .unwrap();
    std::fs::write(
        dir.join("_app/immutable/chunks/0.Abc123.js"),
        b"export const a=1",
    )
    .unwrap();
    // Fixed name, same parent directory: this is what keeps the immutable prefix at `immutable`.
    std::fs::write(dir.join("_app/version.json"), b"{\"version\":\"x\"}").unwrap();

    let app = transcodarr::app(dir.clone());

    let cases = [
        (
            "/_app/immutable/chunks/0.Abc123.js",
            "public, max-age=31536000, immutable",
        ),
        ("/_app/version.json", "no-cache"),
        ("/", "no-cache"),
        ("/a/path/with/no/file", "no-cache"),
    ];
    for (path, want) in cases {
        let res = call(&app, path).await;
        assert_eq!(res.status(), StatusCode::OK, "{path} must be served");
        let got = res
            .headers()
            .get(CACHE_CONTROL)
            .map(|v| v.to_str().unwrap().to_string());
        assert_eq!(
            got.as_deref(),
            Some(want),
            "{path} carried the wrong cache-control"
        );
    }

    // A chunk name from a build that is no longer on disk. The old answer was 200 with the HTML shell,
    // which a module import reads as a syntax error several layers away from the real cause.
    let res = call(&app, "/_app/immutable/chunks/0.DeadBeef.js").await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "a missing hashed chunk must not be answered by the shell"
    );
    let cc = res
        .headers()
        .get(CACHE_CONTROL)
        .map(|v| v.to_str().unwrap().to_string());
    assert_eq!(
        cc.as_deref(),
        Some("public, max-age=0, must-revalidate"),
        "a missing chunk's 404 must be neither cacheable nor left to heuristic freshness"
    );
    let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
    assert!(body.is_empty(), "missing chunk returned a body: {body:?}");

    std::fs::remove_dir_all(&dir).ok();
}

async fn call(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}
