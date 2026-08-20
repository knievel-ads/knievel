# Contributing to Knievel

Read [AGENTS.md](AGENTS.md) for repository rules and
[CODEMAP.md](CODEMAP.md) for source ownership before changing code.

## Development setup

Rust development uses the compiler and components pinned by
[`rust-toolchain.toml`](rust-toolchain.toml):

```sh
rustup show
cargo build --workspace --locked
```

The admin application owns its Node and pnpm requirements in
[`web/admin/package.json`](web/admin/package.json):

```sh
pnpm --dir web/admin install --frozen-lockfile
pnpm --dir web/admin build
```

DB-backed tests need PostgreSQL. The reference service uses PostgreSQL 16:

```sh
docker compose -f examples/compose/compose.yaml up -d knievel-postgres
export DATABASE_URL='postgres://knievel_app:dev@localhost:5432/knievel?sslmode=disable'
```

`testlib::db::ephemeral` creates per-test databases. The application role must
be `NOSUPERUSER`; otherwise PostgreSQL bypasses RLS and tenant tests give false
confidence. Many tests self-skip if `DATABASE_URL` is absent, so inspect test
output rather than treating exit zero alone as a DB pass.

## Branches, commits, and releases

All changes use a branch and pull request against `main`, including maintainer
changes. Keep each PR coherent and reviewable; do not bundle unrelated cleanup.

Use conventional commit subjects, for example:

```text
docs: clarify snapshot refresh limitations
fix: bind project only after org ownership check
```

Historical `Phase X.Y` labels in old commits and documents are not the current
commit convention. Release tags and publication workflows are created only for
an explicitly requested release; ordinary feature or maintenance PRs do not tag.

## Contract rules

### API and generated files

The Rust `#[oai]` declarations and the matching API tuples in
[`src/server.rs`](src/server.rs) and [`src/lib.rs`](src/lib.rs) generate the
OpenAPI contract.

- Do not edit [`openapi.yaml`](openapi.yaml) by hand. Regenerate with
  `cargo xtask openapi`.
- Keep the marked canonical operation table in [API.md](API.md) exact. The gate
  compares normalized HTTP method/path pairs in both directions.
- JSON properties and query parameters are `snake_case`.
- Do not edit
  [`web/admin/src/api/generated.ts`](web/admin/src/api/generated.ts) by hand.
  Regenerate it with `cargo xtask ui-client`.
- Direct routes (`/openapi.json`, `/admin/config.json`, `/admin/*`, `/e/*`) are
  documented separately because they are not generated OpenAPI operations.

### Tenancy and migrations

Every project-scoped handler must preserve the standard flow:

1. authenticate the bearer;
2. bind `org_id` only;
3. verify the path project belongs to that org;
4. bind `project_id`; and
5. execute tenant SQL under a non-superuser role.

`knievel_loader` is reserved for cross-tenant background snapshot and rollup
transactions. Request handlers may not assume it.

Migrations are additive and forward-only. Add a numbered migration rather than
editing one that may already have run. Tenant tables require enabled and forced
RLS plus tenant-bound policies. New project-scoped operations require both an
executable cross-tenant negative test and a registry entry in
[`tests/cross_tenant_manifest.toml`](tests/cross_tenant_manifest.toml).

## Test expectations

Start with a focused test, then run every gate affected by the change. Common
Rust and contract checks are:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask lint-migrations
cargo xtask check-cross-tenant
cargo xtask test-shape
cargo xtask check-doc-fences
cargo xtask check-api-doc
cargo xtask openapi --check
cargo xtask check-snake-case
cargo xtask ui-client --check
```

For frontend changes, run the affected full set:

```sh
pnpm --dir web/admin typecheck
pnpm --dir web/admin lint
pnpm --dir web/admin format:check
pnpm --dir web/admin test --run
pnpm --dir web/admin build
```

Render deployment artifacts when they change:

```sh
docker compose -f examples/compose/compose.yaml config
helm lint --strict charts/knievel
helm template knievel charts/knievel \
  --set database.host=db.example \
  --set database.name=knievel \
  --set database.existingSecret=knievel-db \
  --set api.publicBaseUrl=https://ads.example >/tmp/knievel-rendered.yaml
```

The CI DAG in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) is the final
merge gate. Do not claim a command was run if it self-skipped or was not run.

## Documentation expectations

A PR description must state documentation impact. Update current docs whenever
a change affects the API, config, migrations, deployment, admin browser trust
boundary, reporting schema, or release artifacts.

Fenced JSON, YAML, SQL, and Rust examples are parsed by
`cargo xtask check-doc-fences`. Repository links are checked offline with
lychee. Use relative links in Markdown. Design and historical records may retain
their audit history, but must not be presented as the current shipped contract.

## Security reports

Use GitHub's private vulnerability reporting flow described in
[SECURITY.md](SECURITY.md). Do not put a suspected vulnerability in a public
issue.
