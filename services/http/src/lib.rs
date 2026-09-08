//! Authenticated Git Smart HTTP service.

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rustly_git_auth::{AuthError, DynAuthenticator, Principal};
use rustly_git_protocol::{GitService, Slug};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{io::AsyncWriteExt, process::Command};
use tower::ServiceBuilder;
use tower_http::{request_id::MakeRequestUuid, trace::TraceLayer, ServiceBuilderExt as _};

const MAX_PUSH_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    root: Arc<PathBuf>,
    auth: DynAuthenticator,
    git_binary: Arc<PathBuf>,
}

impl AppState {
    pub fn new(root: PathBuf, auth: DynAuthenticator) -> Self {
        Self {
            root: Arc::new(root),
            auth,
            git_binary: Arc::new(PathBuf::from("git")),
        }
    }

    pub fn with_git_binary(mut self, binary: PathBuf) -> Self {
        self.git_binary = Arc::new(binary);
        self
    }
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/workspaces/{owner}/{workspace}", post(create_workspace))
        .route("/git/{owner}/{repository}/info/refs", get(advertise_refs))
        .route("/git/{owner}/{repository}/{service}", post(rpc))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .set_x_request_id(MakeRequestUuid)
                .layer(TraceLayer::new_for_http())
                .propagate_x_request_id(),
        )
        .layer(DefaultBodyLimit::max(MAX_PUSH_BYTES))
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn create_workspace(
    State(state): State<AppState>,
    Path((owner, workspace)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let owner: Slug = owner.parse()?;
    let workspace: Slug = workspace.parse()?;
    authorize(&state, &headers, &owner)?;

    let owner_root = state.root.join(owner.as_str());
    let repository = owner_root.join(format!("{workspace}.git"));
    tokio::fs::create_dir_all(&owner_root).await?;

    if repository.exists() {
        return Ok((
            StatusCode::OK,
            Json(WorkspaceResponse::new(&owner, &workspace, false)),
        ));
    }

    let output = Command::new(state.git_binary.as_ref())
        .args(["init", "--bare", "--initial-branch=main"])
        .arg(&repository)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .await?;
    if !output.status.success() {
        return Err(ApiError::Git(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse::new(&owner, &workspace, true)),
    ))
}

#[derive(Serialize)]
struct WorkspaceResponse {
    owner: String,
    workspace: String,
    clone_path: String,
    created: bool,
}

impl WorkspaceResponse {
    fn new(owner: &Slug, workspace: &Slug, created: bool) -> Self {
        Self {
            owner: owner.to_string(),
            workspace: workspace.to_string(),
            clone_path: format!("/git/{owner}/{workspace}.git"),
            created,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdvertiseQuery {
    service: String,
}

async fn advertise_refs(
    State(state): State<AppState>,
    Path((owner, repository)): Path<(String, String)>,
    Query(query): Query<AdvertiseQuery>,
    request: Request<Body>,
) -> Result<Response<Body>, ApiError> {
    let service = GitService::parse(&query.service)?;
    dispatch(
        state,
        owner,
        repository_name(&repository)?,
        "info/refs",
        service,
        request,
    )
    .await
}

async fn rpc(
    State(state): State<AppState>,
    Path((owner, repository, service)): Path<(String, String, String)>,
    request: Request<Body>,
) -> Result<Response<Body>, ApiError> {
    let service = GitService::parse(&service)?;
    dispatch(
        state,
        owner,
        repository_name(&repository)?,
        service.command(),
        service,
        request,
    )
    .await
}

fn repository_name(repository: &str) -> Result<String, ApiError> {
    repository
        .strip_suffix(".git")
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or(ApiError::InvalidRepositoryPath)
}

async fn dispatch(
    state: AppState,
    owner: String,
    workspace: String,
    suffix: &str,
    service: GitService,
    request: Request<Body>,
) -> Result<Response<Body>, ApiError> {
    let owner: Slug = owner.parse()?;
    let workspace: Slug = workspace.parse()?;
    let principal = authorize(&state, request.headers(), &owner)?;
    let repository = state
        .root
        .join(owner.as_str())
        .join(format!("{workspace}.git"));
    if !repository.is_dir() {
        return Err(ApiError::NotFound);
    }

    if request.method() == http::Method::POST {
        let expected = format!("application/x-{}-request", service.command());
        if request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            != Some(expected.as_str())
        {
            return Err(ApiError::UnsupportedMediaType);
        }
    }

    let git_protocol = request
        .headers()
        .get("git-protocol")
        .and_then(|value| value.to_str().ok())
        .filter(|value| *value == "version=2")
        .map(str::to_owned);
    let query = request.uri().query().unwrap_or_default().to_owned();
    let method = request.method().as_str().to_owned();
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = axum::body::to_bytes(request.into_body(), MAX_PUSH_BYTES).await?;

    let mut command = Command::new(state.git_binary.as_ref());
    command
        .arg("http-backend")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_PROJECT_ROOT", state.root.join(owner.as_str()))
        .env("PATH_INFO", format!("/{workspace}.git/{suffix}"))
        .env("REQUEST_METHOD", method)
        .env("QUERY_STRING", query)
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("REMOTE_USER", principal.account.as_str())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(protocol) = git_protocol {
        command.env("HTTP_GIT_PROTOCOL", protocol);
    }

    let mut child = command.spawn()?;
    child
        .stdin
        .take()
        .ok_or(ApiError::MissingPipe)?
        .write_all(&body)
        .await?;
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| ApiError::GitTimeout)??;
    if !output.status.success() {
        tracing::warn!(
            owner = %owner,
            workspace = %workspace,
            service = service.command(),
            stderr = %String::from_utf8_lossy(&output.stderr),
            "git http-backend failed"
        );
        return Err(ApiError::Git("git backend rejected the request".to_owned()));
    }

    parse_cgi_response(Bytes::from(output.stdout))
}

fn authorize(state: &AppState, headers: &HeaderMap, owner: &Slug) -> Result<Principal, ApiError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthenticated)?;
    let bearer = header
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthenticated)?;
    let principal = state.auth.authenticate(bearer)?;
    if principal.account != *owner {
        return Err(ApiError::Forbidden);
    }
    Ok(principal)
}

fn parse_cgi_response(output: Bytes) -> Result<Response<Body>, ApiError> {
    let boundary = output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(ApiError::MalformedCgi)?;
    let raw_headers = std::str::from_utf8(&output[..boundary])?;
    let mut status = StatusCode::OK;
    let mut response = Response::builder();

    for line in raw_headers.lines() {
        let (name, value) = line.split_once(':').ok_or(ApiError::MalformedCgi)?;
        if name.eq_ignore_ascii_case("Status") {
            let code = value
                .trim()
                .split_once(' ')
                .map_or(value.trim(), |(code, _)| code);
            status = StatusCode::from_bytes(code.as_bytes())?;
            continue;
        }
        let name = HeaderName::from_bytes(name.as_bytes())?;
        let value = HeaderValue::from_str(value.trim())?;
        response = response.header(name, value);
    }

    Ok(response
        .status(status)
        .body(Body::from(output.slice(boundary + 4..)))?)
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("authentication required")]
    Unauthenticated,
    #[error("this credential does not own the requested workspace")]
    Forbidden,
    #[error("workspace not found")]
    NotFound,
    #[error("unsupported Git service")]
    InvalidService(#[from] rustly_git_protocol::InvalidService),
    #[error("repository paths must end in .git")]
    InvalidRepositoryPath,
    #[error("invalid owner or workspace: {0}")]
    InvalidSlug(#[from] rustly_git_protocol::InvalidSlug),
    #[error("invalid credentials")]
    Authentication(#[from] AuthError),
    #[error("Git RPC content type is invalid")]
    UnsupportedMediaType,
    #[error("Git backend timed out")]
    GitTimeout,
    #[error("Git backend failed: {0}")]
    Git(String),
    #[error("Git backend returned malformed CGI output")]
    MalformedCgi,
    #[error("Git backend pipe was unavailable")]
    MissingPipe,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("request body error: {0}")]
    Body(#[from] axum::Error),
    #[error("invalid CGI UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("invalid status: {0}")]
    Status(#[from] http::status::InvalidStatusCode),
    #[error("invalid header name: {0}")]
    HeaderName(#[from] http::header::InvalidHeaderName),
    #[error("invalid header value: {0}")]
    HeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error("could not construct response: {0}")]
    Response(#[from] http::Error),
    #[error("workspace operation timed out")]
    Elapsed(#[from] tokio::time::error::Elapsed),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response<Body> {
        let (status, code) = match self {
            Self::Unauthenticated | Self::Authentication(_) => {
                (StatusCode::UNAUTHORIZED, "unauthenticated")
            }
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::InvalidService(_) | Self::InvalidSlug(_) | Self::InvalidRepositoryPath => {
                (StatusCode::BAD_REQUEST, "invalid_request")
            }
            Self::UnsupportedMediaType => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type")
            }
            Self::Body(_) => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            _ => (StatusCode::BAD_GATEWAY, "git_backend_error"),
        };
        let mut response = (
            status,
            Json(ErrorBody {
                code,
                message: self.to_string(),
            }),
        )
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"rustly-git\""),
            );
        }
        response
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}
