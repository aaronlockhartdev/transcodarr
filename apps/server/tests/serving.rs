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

/// A request target that is not in normal form gets the same empty 404 as `/api`, before any layer
/// decides what the path means (#22). Refusing it inside `ServeDir` instead would leave the answer to
/// whichever layer happened to notice, and that is why a literal `..` and a `%2e%2e` used to disagree.
#[tokio::test]
async fn non_normal_request_targets_are_refused() {
    let dir = std::env::temp_dir().join(format!("transcodarr-targets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        b"<!doctype html>\n<html lang=\"en\">the app</html>",
    )
    .unwrap();
    std::fs::write(dir.join("movie.2024.mkv"), b"dotted").unwrap();
    let app = transcodarr::app(dir.clone());

    // Structure, spelled wrong. `//` is an empty segment, `%2f` and `%5c` are separators, and the dot
    // forms are a segment that *is* `.` or `..`.
    for path in [
        "/a/../b",
        "/x/./y",
        "/..",
        "/%2e%2e/etc/passwd",
        "/%2E%2E/api",
        "/x/%2e%2e/api",
        "//api/x",
        "/api%2f",
        "/api%5cx",
    ] {
        let res = call(&app, path).await;
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "{path} must be refused"
        );
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert!(
            body.is_empty(),
            "{path} returned a body, not an empty 404: {body:?}"
        );
    }

    // Names, not structure. A dot inside a filename is part of the name, in either spelling, and a
    // deep link with a trailing slash is a route the app router decides on, not a defect.
    for path in [
        "/movie.2024.mkv",
        "/movie%2e2024.mkv",
        "/a/path/with/no/file/",
    ] {
        let res = call(&app, path).await;
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "{path} is a name, not a defect"
        );
        std::mem::drop(to_bytes(res.into_body(), 64 * 1024).await.unwrap());
    }

    std::fs::remove_dir_all(&dir).ok();
}

async fn call(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}
