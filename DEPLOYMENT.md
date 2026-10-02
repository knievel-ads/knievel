# Deploying Knievel

This is the current operator guide. For process internals see
[ARCHITECTURE.md](ARCHITECTURE.md); for repository ownership see
[CODEMAP.md](CODEMAP.md). Older requirement and migration documents contain
unimplemented deployment designs and are not runbooks.

## Shipped artifacts

| Artifact | Location / publication |
|---|---|
| Multi-platform server image | `ghcr.io/knievel-ads/knievel` |
| OCI Helm chart | `oci://ghcr.io/knievel-ads/charts/knievel` |
| Reference local stack | [`examples/compose/`](examples/compose/) |
| Server/CLI source build | Rust workspace in this repository |
| Admin bundle | Packaged inside the release image at `/var/lib/knievel/admin` |

Publication happens only from an explicit `v*` Git tag. The release workflow
creates image tags `X.Y.Z`, `X.Y`, `X`, and `sha-<commit>` (without a leading
`v`), signs the merged manifest, and attaches provenance. Use an immutable
manifest digest for controlled environments:

```sh
docker pull ghcr.io/knievel-ads/knievel@sha256:<manifest-digest>
```

The runtime Dockerfile does not compile source. Build a native local image with:

```sh
cargo xtask build-image --tag knievel:local
```

This runs the admin build, locked Rust release build, staging step, and Docker
packaging. `--skip-ui` creates a headless bundle placeholder.

## PostgreSQL requirements and roles

The reference and CI environments use PostgreSQL 16. The application expects a
writable primary endpoint, the `knievel` schema, and permission to run its sqlx
migrations when `database.auto_migrate` is true.

Never connect request traffic as a PostgreSQL superuser. Superusers bypass
`FORCE ROW LEVEL SECURITY`. A representative one-time bootstrap, run by a DB
administrator, is:

```sql
CREATE ROLE knievel_app
  LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'replace-me';
CREATE SCHEMA knievel AUTHORIZATION knievel_app;
ALTER ROLE knievel_app SET search_path = knievel, public;

CREATE ROLE knievel_loader NOLOGIN BYPASSRLS;
GRANT knievel_loader TO knievel_app;
GRANT USAGE ON SCHEMA knievel TO knievel_loader;

-- Tables are created later by migrations owned by knievel_app.
ALTER DEFAULT PRIVILEGES FOR ROLE knievel_app IN SCHEMA knievel
  GRANT SELECT ON TABLES TO knievel_loader;
ALTER DEFAULT PRIVILEGES FOR ROLE knievel_app IN SCHEMA knievel
  GRANT SELECT ON SEQUENCES TO knievel_loader;
```

After migrations create the rollup tables, bound loader writes to only those
outputs:

```sql
GRANT INSERT, UPDATE ON knievel.events_rollup TO knievel_loader;
GRANT UPDATE ON knievel.events_rollup_watermark TO knievel_loader;
```

The snapshot loader needs SELECT across active configuration tables and the
`config_version` sequence. The rollup also needs SELECT on `events_raw`. Keep
`knievel_loader` `NOLOGIN`; request code must never use it.

The app pool is also held by the advisory-lock leader connection. Size
`database.max_connections` for normal request/auth work plus that long-lived
acquisition and background queries. The current code does not create separate
LISTEN or COPY pools despite older sizing documents.

## Configuration loading

Knievel loads, in increasing precedence:

1. Rust defaults;
2. YAML at `KNIEVEL_CONFIG` (default `/etc/knievel/config.yaml`); and
3. `KNIEVEL_` environment overrides, with `__` between nested keys.

Environment placeholders in the YAML are expanded before parse. `${NAME}` is
required; `${NAME:default}` has a fallback. Expansion also sees comments, so do
not put a literal required placeholder in a comment.

Unknown YAML fields are accepted and ignored. A value appearing in a file or
Helm ConfigMap is therefore not proof that the process consumes it.

### Effective fields

| Field | Current effect |
|---|---|
| `api.bind_addr` | Listener address. |
| `api.allowed_origins` | Literal CORS allowlist; empty disables CORS middleware. |
| `api.shutdown_drain_timeout_secs` | Poem graceful-drain timeout. |
| `database.url` | PostgreSQL connection URL. |
| `database.max_connections` | Pool maximum. |
| `database.auto_migrate` | Runs bundled sqlx migrations at boot. |
| `database.required` | Missing/unusable DB is fatal when true. |
| `database.connect_retry.*` | Initial exponential retry attempts/backoff. |
| `logging.level`, `logging.format` | `tracing-subscriber` filter and JSON/compact format. |
| `logging.request_log_*` | Request logging enablement, exact skip paths, slow threshold. |
| `events.channel_capacity` | In-process event channel size. |
| `decisions.force_overrides_enabled` | Process-wide force-override gate. |
| `partitions.retention_days` | Age at which attached raw-event leaves are detached. |
| `auth.jwt.issuers` | Enables JWT verification alongside opaque tokens. |
| `admin_ui.static_dir`, `admin_ui.oidc.*` | Static SPA mount and public runtime OIDC metadata. |

### Parsed but ineffective or stubbed

| Field | Current limitation |
|---|---|
| `api.public_base_url` | Required by the typed `api` block but not used to build tracking URLs; decisions return relative `/e/...` paths. |
| `api.shutdown_total_timeout_secs` | Parsed and included in the listener log, but not enforced; only the drain timeout is passed to Poem. |
| `database.schema` | Parsed, but pool setup hard-codes `search_path TO knievel, public`. |
| `tracing.otel.*` | Logs an enablement message; no exporter is installed. |
| `errors.sentry.*` | Logs an enablement message; no Sentry SDK is installed. |

The full, intentionally trimmed example is
[`config.example.yaml`](config.example.yaml).

## Reference Compose stack

Use the repository-root instructions in [README.md](README.md). The important
order is:

1. create writable `tmp/`;
2. start only `knievel-postgres` and `knievel`;
3. wait for `/healthz` so migrations have completed;
4. run `knievel-seed` with the host UID/GID;
5. capture project/site/ad-type IDs from stdout;
6. restart `knievel` for a cold snapshot load; and
7. issue a decision and require a non-empty result.

Starting every Compose service at once races the DB-direct seeder against
migrations and does not refresh the running snapshot. The seeder is a manually
invoked one-shot service, not a durable sidecar.

The default image reference is the mutable major tag. Override it with a local
image or immutable digest:

```sh
KNIEVEL_IMAGE=knievel:local \
  docker compose -f examples/compose/compose.yaml up -d knievel-postgres knievel

KNIEVEL_IMAGE=ghcr.io/knievel-ads/knievel@sha256:<manifest-digest> \
  docker compose -f examples/compose/compose.yaml up -d knievel-postgres knievel
```

The Compose DB bootstrap intentionally grants broader loader default writes
than recommended production provisioning so migrations-created rollup tables
work without a second admin step. Do not copy that convenience grant into a
least-privilege production role unchanged.

## Helm

A minimal render/install needs DB coordinates, a Secret with username/password,
and an explicit externally meaningful API base URL:

```sh
helm upgrade --install knievel \
  oci://ghcr.io/knievel-ads/charts/knievel \
  --set database.host=db.example \
  --set database.name=knievel \
  --set database.existingSecret=knievel-db \
  --set api.publicBaseUrl=https://ads.example
```

`api.publicBaseUrl` must be non-empty because the rendered typed `api` block
otherwise omits a required field and startup fails. The chart default is a
local-development value so `helm lint` can render; production must override it.
The server currently still returns relative tracking paths, so the value is
required configuration hygiene rather than working URL rewriting.

The chart's image helper accepts either:

```yaml
image:
  repository: ghcr.io/knievel-ads/knievel
  tag: "X.Y.Z"            # renders repository:X.Y.Z
```

or:

```yaml
image:
  repository: ghcr.io/knievel-ads/knievel
  tag: "sha256:<digest>"  # renders repository@sha256:<digest>
```

A `sha-<commit>` value is a mutable-looking tag, not a digest. Do not prepend
`v` to semver image tags produced by the current workflow.

### Chart value limitations

The workload templates predate the current Rust config shape. Values retained
for template compatibility but not honored by the binary are labeled in
[`charts/knievel/values.yaml`](charts/knievel/values.yaml). In particular:

- `events.retentionDays`, `flushIntervalMs`, and `flushBatchSize` render under
  `events`, but only `channelCapacity` maps to a Rust field. Runtime retention
  comes from `partitions.retention_days`, which the chart does not expose.
- `logging.decisionsSampleRate` is ignored.
- the chart's Sentry and OTel blocks render at unsupported top-level keys and,
  even in the Rust-recognized nesting, those integrations are stubbed;
- no persistent image backend is configurable; and
- `serviceMonitor.enabled` renders a `/metrics` scrape even though the server
  has no `/metrics` route. Leave it false.

These are disclosed limitations, not promises to implement them in the chart.
The workload templates are unchanged by documentation-only corrections.

## Admin surface

The release image sets `KNIEVEL_ADMIN_UI__STATIC_DIR` to the bundled admin
assets, so `/admin/` is on by default. Unset or empty that variable for a
headless server. `GET /admin/config.json` exposes only issuer, public client ID,
scopes, and the `require_oidc` flag.

Both OIDC and pasted opaque tokens live in browser `sessionStorage`. Put the
admin origin behind TLS and appropriate network/access controls. Same-origin
script execution can steal the bearer; the server does not provide an HttpOnly
cookie boundary. Set `adminUi.oidc.requireOidc=true` to hide the paste-token
fallback after OIDC is configured.

## Probes and observability

- `/healthz` means the process is serving HTTP.
- `/readyz` runs `SELECT 1` when a pool exists. It does not verify snapshot or
  event-flusher health. In allowed DB-less mode it returns 200 with a reason.
- `/version` shows build metadata and effective auth issuer summaries.
- `/openapi.json` serves the live generated spec.
- `/metrics` does not exist.

Use structured stdout/stderr request logs as the working observability surface.
Do not enable chart OTel/Sentry settings expecting export.

## Runtime and data caveats

### Snapshot freshness

Each pod cold-loads independently and polls `config_version` every five seconds.
Writes do not bump that sequence. Restart pods after DB-direct provisioning or
explicitly coordinate a sequence bump; a successful management write alone does
not guarantee refresh.

### Event durability

The queue is process memory. A flush executes per-row INSERTs; a DB failure logs
and drops the current batch. Shutdown passes Poem a request-drain timeout but
does not await the event flusher handle, and the parsed total timeout is not
enforced. Plan capacity and deploy drains accordingly.

### Images

Uploads are process-local memory. They are unsuitable for persistent production
creative hosting and break across restart or replica selection. Store durable
image URLs externally and write those URLs through the creative API instead of
relying on the upload operation.

### Retention

The partition manager is intended to detach old `events_raw` leaves; it does
not drop them. Migration `0010`'s year-wide 2026 seed leaf overlaps the daily
leaves the manager attempts during 2026, so the pass currently errors on CREATE
before it reaches detachment. Repair the partition layout deliberately, then
monitor detached tables and implement an operator-owned archive/drop policy.
`events_rollup` has no automated retention.

## Migrations and rollback

Migrations are bundled and forward-only. Prefer a pre-deploy migration job when
your change-control process requires explicit DB ownership; otherwise
`database.auto_migrate=true` runs them at pod boot. Multiple pods may attempt
the same sqlx migration lock path, so stage rollout conservatively.

Application rollback means redeploying a prior image while leaving additive DB
objects in place. Never edit or manually mark a shipped migration as reverted.
Verify that the prior binary tolerates the additive schema before rollback.

## Validation

Before deploying changed manifests:

```sh
docker compose -f examples/compose/compose.yaml config
helm lint --strict charts/knievel
helm template knievel charts/knievel \
  --set database.host=db.example \
  --set database.name=knievel \
  --set database.existingSecret=knievel-db \
  --set api.publicBaseUrl=https://ads.example >/tmp/knievel-rendered.yaml
```

A syntactically valid render does not prove that a rendered config field is
consumed; compare the ConfigMap with [`src/config.rs`](src/config.rs).
