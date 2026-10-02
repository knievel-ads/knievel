# CLAUDE.md

Compatibility entry point for Claude-based tooling and older source comments.
The canonical repository instructions are [AGENTS.md](AGENTS.md); read
[CODEMAP.md](CODEMAP.md) next. Current behavior belongs in source,
[API.md](API.md), [AUTH.md](AUTH.md), [ARCHITECTURE.md](ARCHITECTURE.md), and
[DEPLOYMENT.md](DEPLOYMENT.md), not in this compatibility file.

## Legacy anchors

These short anchors preserve names referenced by existing comments and generated
operation descriptions. They are not a second rule set.

### Sandbox limitations

Verify local capabilities rather than assuming Docker, Postgres, Helm, pnpm, or
network access. DB-backed tests often return success after self-skipping when
`DATABASE_URL` is absent; read their output. The OIDC integration test may also
self-skip when Docker is unavailable.

### Open known gaps

The current, reviewed limitations are listed in [README.md](README.md) and the
source-oriented flow notes in [CODEMAP.md](CODEMAP.md). In particular, do not
infer a shipped feature from old phase notes. A code change that closes a gap
must update the current docs in the same PR.

### Gotcha 4 — generated OpenAPI version

`poem-openapi` currently emits OpenAPI 3.0.0. The generated
[`openapi.yaml`](openapi.yaml), not an aspirational 3.1 statement in a design
record, is authoritative.

### Gotcha 6 — local composite actions

A workflow must run `actions/checkout` before a local action such as
`./.github/actions/rust-setup` or `./.github/actions/node-setup` can be loaded.

### Gotcha 17 — superusers bypass RLS

Postgres superusers bypass `FORCE ROW LEVEL SECURITY`. Request and test traffic
must use a `NOSUPERUSER`, non-`BYPASSRLS` application role. See
[AGENTS.md](AGENTS.md) and [`src/handlers.rs`](src/handlers.rs).

### Cross-cutting risk 2 — Aurora and notification loss

Old comments use this name for Aurora failover and LISTEN/NOTIFY loss. The
current snapshot loader does not establish a listener; it cold-loads and polls
`config_version` every five seconds. Verify any future notification work against
[`src/snapshot.rs`](src/snapshot.rs) and real failover behavior.

### Per-resource fixture duplication

Some `tests/api_*.rs` files intentionally carry local fixture helpers. Do not
perform a broad extraction as drive-by cleanup; make test-structure changes in a
focused PR with the affected slices green.

## Delivery pointer

Use branches and pull requests, conventional commits, proportional tests, and
explicit documentation impact. Never create a release tag unless the user has
specifically requested a release. Full details are in [AGENTS.md](AGENTS.md).
