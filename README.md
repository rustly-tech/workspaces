# rustly-tech/git

Authenticated Git Smart HTTP ingress for Rustly learning workspaces.

Rustly workspaces are ordinary Git repositories. This service lets a learner
create, clone, fetch, and push one with a standard Git client while keeping the
HTTP, identity, and repository-storage boundaries replaceable.

## Status

| Capability | Status |
| --- | --- |
| Create an isolated bare repository | **IMPLEMENTED** |
| Clone, fetch, and push through Git Smart HTTP | **IMPLEMENTED** and tested with real Git |
| Owner-scoped bearer authentication | **IMPLEMENTED** for static self-hosted credentials |
| Rustly identity integration and token rotation | **PLANNED** |
| Durable/object-store-backed repositories | **PLANNED** |
| Production abuse and capacity qualification | **EXPERIMENTAL** |

The service is usable for local development and a trusted self-hosted node. It
is not yet qualified as an Internet-facing production service. See the
[threat model](docs/THREAT_MODEL.md).

## Run locally

Git itself must be installed because the service invokes `git http-backend`
directly without a shell.

```sh
export RUSTLY_GIT_CREDENTIALS='{"learner":"replace-with-at-least-32-random-bytes"}'
cargo run -p rustly-git-http

curl -X POST http://127.0.0.1:8090/v1/workspaces/learner/ownership \
  -H 'Authorization: Bearer replace-with-at-least-32-random-bytes'

git -c 'http.extraHeader=Authorization: Bearer replace-with-at-least-32-random-bytes' \
  clone http://127.0.0.1:8090/git/learner/ownership.git
```

`RUSTLY_GIT_ROOT` changes the bare-repository root and `RUSTLY_GIT_BIND`
changes the listener. The defaults are `./repositories` and `127.0.0.1:8090`.

## Boundaries

- `rustly-git-protocol` owns validated identifiers and the two allowed Git RPCs.
- `rustly-git-auth` owns the provider-neutral `Authenticator` interface.
- `rustly-git-http` authenticates requests and adapts them to Git's CGI
  protocol.

The HTTP service never invokes a shell. It clears the child environment and
constructs `PATH_INFO` only from validated slugs.

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

MIT or Apache-2.0, at your option.

