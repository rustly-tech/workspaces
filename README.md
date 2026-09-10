# Rustly workspaces

Git-backed coding workspaces for Rustly.

The service creates a learner's repository and lets standard Git clients clone,
fetch, and push it over HTTP. It currently supports local development and
trusted self-hosting. Authentication, quotas, and storage still need further
work before an Internet-facing deployment; see the [threat model](docs/THREAT_MODEL.md).
The [repository provider boundary](docs/REPOSITORY_PROVIDER.md) keeps workspace
identity independent of its current local Git storage.

## Run locally

Git must be installed because the service calls `git http-backend` directly.

```sh
export RUSTLY_GIT_CREDENTIALS='{"learner":"replace-with-at-least-32-random-bytes"}'
cargo run -p rustly-git-http

curl -X POST http://127.0.0.1:8090/v1/workspaces/learner/ownership \
  -H 'Authorization: Bearer replace-with-at-least-32-random-bytes'

git -c 'http.extraHeader=Authorization: Bearer replace-with-at-least-32-random-bytes' \
  clone http://127.0.0.1:8090/git/learner/ownership.git
```

`RUSTLY_GIT_ROOT` changes the repository directory and `RUSTLY_GIT_BIND`
changes the listen address. The defaults are `./repositories` and
`127.0.0.1:8090`.

## Verify

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

MIT or Apache-2.0, at your option.
