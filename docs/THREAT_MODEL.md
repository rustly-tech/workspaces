# Git ingress threat model

## Status and scope

The Smart HTTP path is **IMPLEMENTED** and exercised with the real Git client.
The service as a public multi-tenant deployment is **EXPERIMENTAL**.

The protected assets are learner source, repository integrity, credentials,
host disk capacity, and the host process. The attacker controls every URL,
header, request byte, Git object, ref name, and commit payload.

## Enforced controls

| Threat | Control | Evidence |
| --- | --- | --- |
| Path traversal | Owners and workspace names are parsed as lowercase slugs; `.git` is stripped as a fixed suffix | protocol and HTTP tests |
| Command injection | Service names map to a two-variant enum; no shell is invoked | protocol tests and source review |
| Cross-account access | Credential principal must equal the owner in the route | HTTP authorization tests |
| Arbitrary Git CGI dispatch | Only `git-upload-pack` and `git-receive-pack` are accepted | protocol tests |
| Oversized push request | Axum rejects bodies over 32 MiB | configured middleware |
| Stuck Git process | Each CGI child has a 30-second deadline and is killed on drop | configured process boundary |
| Child environment attacks | The Git child environment is cleared and rebuilt from fixed names | source review |
| Host file reads through repository names | Git project root is fixed; path segments are validated | path tests |
| Plaintext token retention | Static credentials are BLAKE3-hashed on construction | authentication tests |

Repositories created by the service contain no hooks, and normal Git pushes
cannot create hooks. Operators must treat the repository root as service-owned;
writing hooks there grants code execution as the service account.

## Open production blockers

1. Static tokens need short-lived credentials issued by Rustly identity, plus
   revocation and rotation.
2. Rate limits and per-account storage/object quotas need to be enforced before
   the service is exposed publicly.
3. Request and response bodies are buffered. Large-repository support needs a
   streaming bridge with measured memory bounds.
4. Bare repositories currently live on one filesystem. Backup, replication,
   garbage collection, and recovery have not been qualified.
5. Audit events need a durable sink and privacy/retention policy.
6. Fuzzing is needed for CGI header parsing and route-to-environment mapping.

Until those close, bind to loopback or place the service behind a trusted,
rate-limited gateway. Do not describe it as production-ready.

