use axum::Router;
use axum::body::{Body, to_bytes};
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

    std::fs::remove_dir_all(&dir).ok();
}

async fn call(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}
