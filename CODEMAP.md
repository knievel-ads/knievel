# Knievel code map

A source-oriented map for contributors. Repository rules live in
[AGENTS.md](AGENTS.md); current product and operator limits live in
[README.md](README.md).

## Workspace and entry points

| Area | Source | Notes |
|---|---|---|
| Server binary | [`src/main.rs`](src/main.rs) | Loads config, initializes logging, and calls `server::run`. |
| HTTP bootstrap | [`src/server.rs`](src/server.rs) | Builds state, mounts routes/middleware/admin assets, binds the listener. |
| Admin CLI | [`src/bin/knievel_cli.rs`](src/bin/knievel_cli.rs), [`src/cli/`](src/cli/) | Shipped commands are `seed-demo` and `admin create-org`. |
| Library surface | [`src/lib.rs`](src/lib.rs) | Exposes modules and `openapi_spec_yaml()` to xtask. |
| Repository CLI | [`xtask/src/main.rs`](xtask/src/main.rs) | Codegen, contract checks, migration checks, image build, and benchmarks. |
| DB test support | [`testlib/`](testlib/) | Ephemeral databases and tenant-bound fixtures. |
| Admin SPA | [`web/admin/`](web/admin/) | Vite/React application with its own package and lock manifests. |

The root package and the `xtask` and `testlib` members share workspace metadata
from [`Cargo.toml`](Cargo.toml). The development compiler is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml).

## HTTP and OpenAPI ownership

The OpenAPI API tuple is registered twice, intentionally:

- [`src/server.rs`](src/server.rs) builds the live `OpenApiService`.
- [`src/lib.rs`](src/lib.rs) builds the document used by
  `cargo xtask openapi`.

Adding or removing an OpenAPI resource requires changing both lists. Drift of
the generated file is checked against [`openapi.yaml`](openapi.yaml).

Routes mounted directly by `poem::Route`, and therefore outside generated
OpenAPI operations, are also in `src/server.rs`:

| Route | Owner |
|---|---|
| `GET /openapi.json` | `OpenApiService::spec_endpoint()` |
| `GET /admin/config.json` | [`src/admin_ui.rs`](src/admin_ui.rs) |
| `GET /e/i/:signed`, `GET /e/c/:signed` | [`src/event_endpoints.rs`](src/event_endpoints.rs) |
| `/admin/*` static SPA fallback | `mount_admin_ui` in `src/server.rs` |

The exact generated operation set is the marked canonical table in
[API.md](API.md). Direct routes are documented separately there.

## Authentication, authorization, and RLS

A project-scoped request follows this chain:

1. [`src/auth/security.rs`](src/auth/security.rs) extracts the bearer.
   JWT-shaped tokens use the configured `JwtVerifier`; other values use the
   opaque parser and a one-row `auth_lookup_id` transaction.
2. [`src/db.rs`](src/db.rs) owns transaction-local GUC binding.
3. [`src/handlers.rs`](src/handlers.rs) checks role/scope and performs the
   two-stage project bind: `org_id` first, project ownership query second,
   `project_id` last.
4. The resource handler runs SQL in that transaction. Migrations under
   [`migrations/`](migrations/) enforce `FORCE ROW LEVEL SECURITY`.

Org-scoped token and ad-library handlers have their own prologues in current
source; verify them rather than assuming every path calls `open_org_tx`.
Application traffic must use a non-superuser role. The `knievel_loader` role is
separate: snapshot and rollup transactions use transaction-scoped
`SET LOCAL ROLE` for cross-tenant background work.

## Configuration and snapshot flow

[`src/config.rs`](src/config.rs) layers a YAML file and `KNIEVEL_` environment
over Rust defaults. It also interpolates environment placeholders before YAML
parse. Only typed fields that are read elsewhere have runtime effect; unknown
YAML is tolerated.

On a DB-backed boot, [`src/server.rs`](src/server.rs) starts the event flusher,
leader loop, partition manager, rollup loop, and one snapshot loader per pod.
[`src/snapshot.rs`](src/snapshot.rs):

- cold-loads all active projects under `knievel_loader`;
- swaps a complete `Arc<Snapshot>` after a successful load;
- polls the `config_version` sequence on a fixed five-second interval; and
- reloads only when the sequence value increases.

There is no active LISTEN/NOTIFY path, incremental diff loader, configurable
poll interval, or management-write `config_version` bump. Seeded data therefore
requires a server restart (or an explicit external sequence bump) before the
current process sees it.

## Decision and event flow

[`src/decisions.rs`](src/decisions.rs) authenticates and opens a project-bound
DB transaction before taking one snapshot pointer. The pure selection portion
then resolves the site, filters flights/ads, chooses the highest priority tier,
performs weighted selection, builds creative output, signs tracking paths, and
composes decision events from RAM. `decisions:explain` uses the same snapshot
without emitting events.

Events pass through the bounded channel in [`src/events.rs`](src/events.rs).
Despite legacy `COPY` wording in comments and designs, `flush_batch` currently
opens one transaction and executes one `set_config` plus one `INSERT ... ON
CONFLICT` per event row. [`src/rollup.rs`](src/rollup.rs) aggregates canonical
rows into `knievel.events_rollup` one settled hour at a time under the loader
role. [`src/partitions.rs`](src/partitions.rs) attempts to create daily leaves and
**detach**, but not drop, leaves older than retention. Migration `0010`'s broad
2026 seed leaf overlaps daily bounds during 2026, causing the pass to return
before the detach sweep.

## Admin UI and clients

- [`web/admin/src/api/generated.ts`](web/admin/src/api/generated.ts) is generated
  from `openapi.yaml`; [`web/admin/src/api/client.ts`](web/admin/src/api/client.ts)
  is the hand-written wrapper.
- [`web/admin/src/auth/`](web/admin/src/auth/) owns OIDC and paste-token login.
  Both bearer forms are readable from browser `sessionStorage`.
- [`src/admin_ui.rs`](src/admin_ui.rs) exposes public OIDC runtime metadata; the
  Docker image defaults the static bundle mount to `/var/lib/knievel/admin`.
- The release workflow regenerates the external Ruby client repository from the
  tagged spec. Its generated Apache-2.0 metadata is owned downstream.

## Packaging and deployment

| Artifact | Source |
|---|---|
| Runtime image | [`Dockerfile`](Dockerfile), [`xtask/src/build_image.rs`](xtask/src/build_image.rs) |
| Reference stack | [`examples/compose/`](examples/compose/) |
| Helm chart | [`charts/knievel/`](charts/knievel/) |
| Per-PR gates | [`.github/workflows/ci.yml`](.github/workflows/ci.yml) |
| Tag release | [`.github/workflows/release.yml`](.github/workflows/release.yml) |

The Dockerfile packages prebuilt server/CLI binaries and the prebuilt admin
bundle; it does not compile them. The Helm ConfigMap currently renders some
legacy keys the Rust config ignores. See [DEPLOYMENT.md](DEPLOYMENT.md) before
relying on a chart value.

A pushed `v*` tag starts the release DAG:

- native amd64/arm64 server image builds → manifest merge/sign/attestation;
- three CLI targets (`x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-musl`, `aarch64-apple-darwin`) → GitHub Release;
- image manifest → packaged/signed OCI Helm chart; and
- tagged OpenAPI → regenerated/tagged external Ruby client.

Tags are explicit release actions; ordinary merges do not publish this DAG.

## Test slices

| Slice | Location / gate | What it proves |
|---|---|---|
| Unit | co-located `#[cfg(test)]` modules | Pure selection, auth, config, HMAC, helpers. |
| API | [`tests/api_*.rs`](tests/) | Poem handler behavior, usually with Postgres. |
| Integration | [`tests/integration_*.rs`](tests/) | Migrations, RLS, snapshots, rollup, CLI, OIDC. |
| Acceptance | [`tests/acceptance.rs`](tests/acceptance.rs) | A subset of named journeys; many scenarios remain ignored. |
| Chaos | [`tests/chaos_*.rs`](tests/) | Presently ignored harness skeletons. |
| Contract | xtask checks + `openapi --check` | Generated API/docs/naming/migration invariants. |
| Frontend | `web/admin` pnpm scripts | Types, lint/format, unit tests, build; Playwright is nightly. |

Most DB tests self-skip when `DATABASE_URL` is absent. Read the output; a
self-skip is not evidence that the DB behavior passed.

## Generated and derived files

| File | Owner command |
|---|---|
| [`openapi.yaml`](openapi.yaml) | `cargo xtask openapi` |
| [`web/admin/src/api/generated.ts`](web/admin/src/api/generated.ts) | `cargo xtask ui-client` |
| [`Cargo.lock`](Cargo.lock) | Cargo resolution after manifest changes |
| [`web/admin/pnpm-lock.yaml`](web/admin/pnpm-lock.yaml) | pnpm resolution after package changes |
| `web/admin/dist/`, `docker-context/`, `target/` | Build output; do not commit |

## Change-impact table

| Change | Inspect/update | Minimum focused impact checks |
|---|---|---|
| API operation/schema | Rust `#[oai]`, both API tuples, `API.md`, generated admin client | `cargo xtask openapi --check`; `cargo xtask check-api-doc`; `cargo xtask check-snake-case`; `cargo xtask ui-client --check` |
| Tenant SQL/handler | migration, auth/DB/handler chain, negative test + manifest | relevant DB test; `cargo xtask lint-migrations`; `cargo xtask check-cross-tenant` |
| Migration | next additive SQL file, provisioning/grants, operator docs | migration integration test; `cargo xtask lint-migrations` |
| Config | `src/config.rs`, example, Compose/chart rendering, deployment docs | config unit tests; Compose config; Helm lint/template |
| Admin UI | `web/admin`, API client if schema changed, security/docs impact | pnpm typecheck, lint, format check, test, build |
| Event/reporting | events, migrations, rollup/partition code, `REPORTING.md` | relevant integration tests and capacity math review |
| Release/package | Docker/Helm/workflow/docs and downstream license boundary | local artifact render/build checks; inspect release DAG dependencies |
| Documentation only | source claims, links, fenced examples, API table if applicable | `cargo xtask check-doc-fences`; `cargo xtask check-api-doc`; offline lychee |
