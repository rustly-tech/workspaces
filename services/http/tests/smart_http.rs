use std::{path::Path, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use rustly_git_auth::StaticTokens;
use rustly_git_http::{app, AppState};
use tempfile::TempDir;
use tokio::process::Command;
use tower::ServiceExt as _;

const ALICE_TOKEN: &str = "alice-token-is-at-least-thirty-two-bytes";
const BOB_TOKEN: &str = "bob-token-is-also-at-least-thirty-two";

fn state(temp: &TempDir) -> AppState {
    let auth = StaticTokens::new([
        ("alice".parse().unwrap(), ALICE_TOKEN.to_owned()),
        ("bob".parse().unwrap(), BOB_TOKEN.to_owned()),
    ])
    .unwrap();
    AppState::new(temp.path().to_owned(), Arc::new(auth))
}

fn request(method: &str, uri: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn workspace_creation_is_authenticated_scoped_and_idempotent() {
    let temp = TempDir::new().unwrap();
    let router = app(state(&temp));

    let response = router
        .clone()
        .oneshot(request("POST", "/v1/workspaces/alice/ownership", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()[header::WWW_AUTHENTICATE],
        "Bearer realm=\"rustly-git\""
    );

    let response = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/workspaces/alice/ownership",
            Some(BOB_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/workspaces/alice/ownership",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(temp.path().join("alice/ownership.git/HEAD").is_file());

    let response = router
        .oneshot(request(
            "POST",
            "/v1/workspaces/alice/ownership",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn malformed_paths_services_and_rpc_types_are_rejected_before_git() {
    let temp = TempDir::new().unwrap();
    let router = app(state(&temp));

    let bad_slug = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/workspaces/Upper/ownership",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(bad_slug.status(), StatusCode::BAD_REQUEST);

    let bad_service = router
        .clone()
        .oneshot(request(
            "GET",
            "/git/alice/ownership.git/info/refs?service=git-upload-archive",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(bad_service.status(), StatusCode::BAD_REQUEST);

    let missing_suffix = router
        .oneshot(request(
            "GET",
            "/git/alice/ownership/info/refs?service=git-upload-pack",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(missing_suffix.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_git_can_clone_push_and_clone_again_over_smart_http() {
    let temp = TempDir::new().unwrap();
    let app_state = state(&temp);
    let created = app(app_state.clone())
        .oneshot(request(
            "POST",
            "/v1/workspaces/alice/ownership",
            Some(ALICE_TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app(app_state)).await.unwrap();
    });

    let first = temp.path().join("first-clone");
    let remote = format!("http://{address}/git/alice/ownership.git");
    git(
        temp.path(),
        [
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {ALICE_TOKEN}"),
            "clone",
            &remote,
            first.to_str().unwrap(),
        ],
    )
    .await;

    tokio::fs::write(
        first.join("ownership.rs"),
        b"fn main() { let s = String::from(\"Rust\"); drop(s); }\n",
    )
    .await
    .unwrap();
    git(&first, ["config", "user.name", "Rustly Test"]).await;
    git(&first, ["config", "user.email", "test@rustly.invalid"]).await;
    git(&first, ["add", "ownership.rs"]).await;
    git(&first, ["commit", "-m", "solve ownership trial"]).await;
    git(
        &first,
        [
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {ALICE_TOKEN}"),
            "push",
            "origin",
            "main",
        ],
    )
    .await;

    let second = temp.path().join("second-clone");
    git(
        temp.path(),
        [
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {ALICE_TOKEN}"),
            "clone",
            &remote,
            second.to_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(
        tokio::fs::read_to_string(second.join("ownership.rs"))
            .await
            .unwrap(),
        "fn main() { let s = String::from(\"Rust\"); drop(s); }\n"
    );

    server.abort();
}

async fn git<'a>(cwd: &Path, arguments: impl IntoIterator<Item = &'a str>) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "git failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn health_is_public_and_contains_no_repository_data() {
    let temp = TempDir::new().unwrap();
    let response = app(state(&temp))
        .oneshot(request("GET", "/healthz", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], br#"{"status":"ok"}"#);
}
