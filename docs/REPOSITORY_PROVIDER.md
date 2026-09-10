# Repository provider boundary

Rustly identifies a coding repository with `RepositoryId { owner, name }`.
That value belongs to Rustly and remains stable when storage or clone locations
change. A filesystem path and an external repository URL are derived locations;
neither is a primary key.

`RepositoryProvider` is the boundary between the authenticated workspace HTTP
service and repository storage. `LocalGitProvider` is the current
implementation. Authentication happens before the call, and the provider gets
only the stable repository identity, the authorized account reference, and the
specific Git operation. It never receives a Rustly bearer token.

A future QridBase implementation can sit behind the same boundary using its API
and scoped grants. Rustly account identity and QridBase identity remain separate;
the link would store an external reference and revocable grant. Removing that
link must leave the Rustly account and `RepositoryId` intact. Rustly continues
to use the local provider when the external service is unavailable or unlinked.

No QridBase client, database access, token exchange, or product dependency is
implemented today.
