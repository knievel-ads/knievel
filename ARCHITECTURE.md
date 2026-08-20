# Knievel architecture

This document describes the code that currently ships. Start with
[CODEMAP.md](CODEMAP.md) for file ownership and [API.md](API.md) for the exact
HTTP operation set. Design records in [REQUIREMENTS.md](REQUIREMENTS.md) are not
runtime evidence.

## Process shape

Knievel is one Tokio process plus PostgreSQL:

```text
                       generated OpenAPI handlers
client ── bearer ──> auth / role / tenant transaction
                              │
                              ▼
                    management SQL or RAM selection
                              │
                 ┌────────────┴────────────┐
                 ▼                         ▼
          bounded event channel      HTTP response with
                 │                   signed tracking paths
                 ▼                         │
        per-row PostgreSQL INSERT          ▼
                 │                  GET /e/i or /e/c
                 ▼                         │
        partitioned events_raw <───────────┘
                 │
                 ▼
          hourly events_rollup
```

There is no Redis, Kafka, external snapshot service, persistent object-storage
adapter, metrics exporter, OTel exporter, or Sentry SDK in the running process.

## Boot sequence

[`src/main.rs`](src/main.rs) loads typed configuration and initializes the
`tracing-subscriber` logger. [`src/server.rs`](src/server.rs) then:

1. creates the JWT verifier from configured issuers;
2. connects to PostgreSQL with bounded retries when `database.url` is set;
3. optionally runs additive sqlx migrations;
4. creates an in-memory creative-image store;
5. starts the event flusher, advisory-lock leader loop, partition manager,
   rollup loop, and per-pod snapshot loader;
6. builds OpenAPI and direct poem routes; and
7. mounts the admin bundle when `admin_ui.static_dir` is non-empty.

A production config defaults `database.required` to true. Missing/unusable DB
configuration is fatal. Tests construct `Config::default()` with `required`
false, which preserves an explicit DB-less mode.

## Route and contract assembly

The generated API tuple appears in both [`src/server.rs`](src/server.rs) and
[`src/lib.rs`](src/lib.rs). The first owns live routing; the second owns
`openapi_spec_yaml()` for xtask. Both must change together.

`src/server.rs` also mounts non-OpenAPI routes:

- `/openapi.json` from the live OpenAPI service;
- `/admin/config.json` from [`src/admin_ui.rs`](src/admin_ui.rs);
- `/e/i/:signed` and `/e/c/:signed` from
  [`src/event_endpoints.rs`](src/event_endpoints.rs); and
- `/admin/*` from a static directory with SPA fallback.

Request logging wraps the route tree when enabled and stamps `x-request-id`.
CORS is installed only when `api.allowed_origins` is non-empty; origins are
literal, credentials are disabled, and the middleware allows the admin
application's bearer/header set.

## Authentication and tenant isolation

### Bearer extraction

[`src/auth/security.rs`](src/auth/security.rs) distinguishes JWTs by a valid
three-segment shape whose decoded header has `alg`. A configured JWT verifier
handles that shape. Other bearer strings go through the opaque-token parser.

Opaque lookup has a deliberate bootstrap exception: before the tenant is known,
`db::begin_auth_lookup` binds `knievel.auth_lookup_id`, allowing RLS to reveal
only the named token row. Argon2 verifies the secret and produces a common
`Principal`.

### Project binding

[`handlers::open_project_tx`](src/handlers.rs) enforces role and project scope,
then performs two-stage binding:

1. begin a transaction with only the principal's `knievel.org_id`;
2. query the path project under that org-only policy; and
3. after ownership succeeds, set `knievel.project_id` locally.

Binding the unverified path project before step 2 would let the projects
policy's project-id branch prove itself. This ordering is a security invariant.
Tenant migrations enable and force RLS, but PostgreSQL superusers still bypass
it. Request traffic must use a non-superuser app role.

### Loader role

`knievel_loader` is a separately provisioned `NOLOGIN BYPASSRLS` role granted
to the app role. Only snapshot and rollup transactions execute
`SET LOCAL ROLE knievel_loader`. `SET LOCAL` returns the pooled connection to
its prior role at transaction end. Production grants should give it SELECT on
snapshot inputs and INSERT/UPDATE only on rollup outputs; it is not a request
identity.

## Snapshot lifecycle

Every DB-backed pod creates an empty [`SnapshotStore`](src/snapshot.rs) and
spawns a cold load. `load_snapshot` opens one loader-role transaction and reads
all active projects, flights, ads, sites, zones, creatives/templates, and
project signing state into a fresh map. A complete `Arc<Snapshot>` replaces the
old value in one swap, so a request sees one consistent version.

After cold load, the loop checks the `knievel.config_version` sequence every
five seconds. A larger value triggers another full load. Current boundaries:

- there is no active `PgListener`/LISTEN/NOTIFY connection;
- there is no incremental diff load;
- five seconds is a constant, not a config field;
- management handlers do not call `nextval(config_version)`; and
- a write therefore remains invisible until an external bump or process cold
  load.

The demo quickstart restarts the server after seeding for this reason.

## Decision flow

`POST /v1/projects/{project_id}/decisions` first authenticates and opens a
reader-level project transaction. It then takes one snapshot pointer and:

1. resolves `site_id` or `site_url`/alias (not `site_external_id`);
2. filters active flights and ads by dates, site/zone/ad-type, and blocklists;
3. keeps the lowest numeric priority tier;
4. chooses weighted ads without replacement for that placement;
5. optionally applies the admin-only, project-enabled force path;
6. builds the typed creative response, rendering Liquid for `templated`; and
7. signs one payload used under relative `/e/c/...` and `/e/i/...` paths.

`api.public_base_url` is not consulted by this code. The selection core is pure
RAM work, but bearer verification/project authorization may touch PostgreSQL.
Force requests also write an audit row before selection. A successful pick
composes one decision event; channel saturation makes the decision request
return 503.

The explainer follows the same snapshot and force gate but emits no event or
audit row and uses placeholder tracking paths.

## Event, rollup, and partition flow

[`src/events.rs`](src/events.rs) owns a bounded MPSC channel (default 8192), a
one-second drain tick, and a 5000-row in-memory batch cap. `flush_batch` does not
use COPY: it opens one transaction and, for each event, changes the org GUC and
executes one `INSERT ... ON CONFLICT`. A failed flush logs and drops the whole
in-memory batch; there is no durable queue.

Event kinds are `0=decision`, `1=impression`, and `2=click`. The uniqueness
constraint includes `ts`, so replay dedup is timestamp-sensitive; see
[REPORTING.md](REPORTING.md).

The advisory-lock leader starts two hourly loops:

- [`src/rollup.rs`](src/rollup.rs) aggregates fully settled hours under the
  loader role into `events_rollup`, replacing counts on conflict and advancing
  a watermark in the same transaction. Null dimensions become the `0`
  unattributed sentinel.
- [`src/partitions.rs`](src/partitions.rs) attempts to pre-create four daily
  leaves, then detach leaves older than retention. It does not archive or drop
  detached tables. During 2026, migration `0010`'s already-attached year-wide
  seed leaf overlaps those daily bounds; the first CREATE fails and the pass
  returns before reaching its detach sweep.

The leader lock uses a connection acquired from the configured pool. Followers
retry every five seconds. A four-hour watchdog releases a stale leader session;
that budget is currently a constant.

## Creative images

The upload handler validates size, declared/sniffed MIME, and a fixed raster
allowlist, then writes through `ImageStore`. Server boot always installs
`InMemoryStore`. Uploads therefore return `memory://` identifiers, disappear on
restart, and are not replica-shared. Object-storage structs in older designs do
not select a runtime backend.

## Admin UI trust boundary

The image includes `web/admin/dist` and defaults
`KNIEVEL_ADMIN_UI__STATIC_DIR=/var/lib/knievel/admin`. The SPA can authenticate
through OIDC PKCE or a pasted opaque bearer. `oidc-client-ts` and the fallback
both use `window.sessionStorage`; OIDC takes precedence when both exist.

This avoids first-party authentication cookies but makes same-origin script
execution a bearer compromise. TLS, CSP/reverse-proxy policy, admin reachability,
and upstream token TTL are operator responsibilities. `require_oidc` hides the
paste-token path but does not move tokens out of browser storage.

## Observability and health

Working behavior:

- JSON or compact `tracing-subscriber` output;
- per-request method/path/status/latency/request-id logging;
- configurable request-log skips and slow threshold;
- `/healthz` process liveness;
- `/readyz` DB reachability (or explicit DB-less 200); and
- `/version` build fields plus effective auth modes/issuer summaries.

Parsed-but-stubbed behavior:

- OTel enablement logs a message but installs no exporter;
- Sentry enablement logs a message but installs no SDK; and
- there is no `/metrics` endpoint.

## Deployment and release topology

The runtime [`Dockerfile`](Dockerfile) packages already-built server/CLI
binaries and admin assets into a distroless non-root image. Local orchestration
lives in `cargo xtask build-image`; release orchestration lives in
[`.github/workflows/release.yml`](.github/workflows/release.yml).

Reference runtime manifests are [`examples/compose/`](examples/compose/) and
[`charts/knievel/`](charts/knievel/). Some chart values are retained because
existing templates reference them even though the Rust config ignores them;
[DEPLOYMENT.md](DEPLOYMENT.md) classifies that boundary.

A tag release builds two native image platforms, three CLI archive targets, an
OCI Helm package, a GitHub Release, and a regenerated external Ruby client. See
[CODEMAP.md](CODEMAP.md) for the DAG and generated-file owners.
