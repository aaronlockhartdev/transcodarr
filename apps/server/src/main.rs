use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // shortcut: the bind address and the web build directory come from environment variables. U3c
    // replaces both with the config file plus the per-worktree data dir and port derived from the
    // branch name, so two agents can run this binary at the same time.
    let addr = std::env::var("TRANSCODARR_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let web_build =
        std::env::var("TRANSCODARR_WEB_BUILD").unwrap_or_else(|_| "apps/web/build".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("transcodarr listening on http://{addr}");

    axum::serve(
        listener,
        transcodarr::app(web_build.into()).into_make_service(),
    )
    .await?;
    Ok(())
}
