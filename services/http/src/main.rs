use std::{env, path::PathBuf, sync::Arc};

use anyhow::Context as _;
use rustly_git_auth::StaticTokens;
use rustly_git_http::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustly_git_http=info,tower_http=info".into()),
        )
        .json()
        .init();

    let bind = env::var("RUSTLY_GIT_BIND").unwrap_or_else(|_| "127.0.0.1:8090".to_owned());
    let root =
        PathBuf::from(env::var("RUSTLY_GIT_ROOT").unwrap_or_else(|_| "./repositories".to_owned()));
    let credential_json = env::var("RUSTLY_GIT_CREDENTIALS")
        .context("RUSTLY_GIT_CREDENTIALS must be a JSON object of account-to-token entries")?;
    let auth = StaticTokens::from_json(&credential_json).context("invalid Git credentials")?;

    tokio::fs::create_dir_all(&root)
        .await
        .context("creating repository root")?;
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(%bind, root = %root.display(), "Rustly Git ingress listening");

    axum::serve(
        listener,
        rustly_git_http::app(AppState::new(root, Arc::new(auth))),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serving Git ingress")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("installing Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("installing SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("received SIGINT, shutting down"),
        () = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}
