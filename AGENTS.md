# Repository guidance

This file applies to the entire repository. Start with [CODEMAP.md](CODEMAP.md)
for ownership and change-impact pointers.

## Source-of-truth order

When two artifacts disagree, use this order and repair the lower-priority one in
the same PR:

1. Executed Rust, SQL migrations, Helm templates, Compose manifests, and GitHub
   workflows.
2. Generated artifacts checked against those sources (`openapi.yaml` and the
   admin TypeScript client).
3. Current contract and operator docs: [API.md](API.md), [AUTH.md](AUTH.md),
   [REPORTING.md](REPORTING.md), [ARCHITECTURE.md](ARCHITECTURE.md), and
   [DEPLOYMENT.md](DEPLOYMENT.md).
4. Design and historical records such as [REQUIREMENTS.md](REQUIREMENTS.md),
   [UI.md](UI.md), [E2E.md](E2E.md), [PHASES.md](PHASES.md), and
   [DOCUMENTATION_PLAN.md](DOCUMENTATION_PLAN.md).

A design document is not evidence that a feature ships. Verify public claims in
source and the generated OpenAPI document.

## Toolchain and manifests

- `rust-toolchain.toml` is the development-toolchain authority. CI must install
  the same toolchain.
- A workspace `rust-version`, when declared, plus the CI compatibility jobs is
  the supported compiler floor. This checkout does not currently declare a
  lower floor; do not infer one from dependency MSRVs.
- Workspace dependency and package metadata belongs in `Cargo.toml`; keep
  `Cargo.lock` coherent and use `--locked` in reproducibility gates.
- `web/admin/package.json` owns Node and pnpm requirements and scripts;
  `web/admin/pnpm-lock.yaml` owns resolved frontend dependencies. Run pnpm from
  `web/admin` (or use `pnpm --dir web/admin ...`).

## Generated files and API contract

- Do not hand-edit `openapi.yaml`. Change the Rust `#[oai]` surface, register it
  in both `src/server.rs` and `src/lib.rs`, then run `cargo xtask openapi`.
- Do not hand-edit `web/admin/src/api/generated.ts`. Regenerate with
  `cargo xtask ui-client` after an OpenAPI schema change.
- Keep the single canonical `Verb | Path | Purpose` table in [API.md](API.md)
  exactly synchronized with generated operations. Direct poem routes and
  unshipped designs stay outside its marked block. Run
  `cargo xtask check-api-doc`.
- Public JSON properties and query parameters are `snake_case`. Run
  `cargo xtask check-snake-case`; documentation wire examples follow the same
  rule.
- The separately generated Ruby repository retains its own Apache-2.0 package
  metadata. This repository's MIT license does not relabel it.

## Tenancy and database invariants

- Production and test request traffic must use a `NOSUPERUSER`, non-`BYPASSRLS`
  application role. A superuser silently defeats `FORCE ROW LEVEL SECURITY`.
- Opaque authentication starts in `auth/security.rs` with the single-row
  `auth_lookup_id` bootstrap. Project handlers then use
  `handlers::open_project_tx`.
- Project binding is deliberately two-stage: bind only `org_id`, prove the path
  project belongs to that org, then bind `project_id`. Never bind an unverified
  path project up front.
- `knievel_loader` is a `NOLOGIN BYPASSRLS` background role. Only snapshot and
  rollup transactions may use `SET LOCAL ROLE knievel_loader`; request handlers
  must never assume it. Keep its table grants least-privilege.
- Migrations are forward-only and additive. Never edit a migration that may
  have shipped; add the next numbered migration. Every tenant table needs
  enabled and forced RLS plus a tenant-bound policy. Run
  `cargo xtask lint-migrations` and `cargo xtask check-cross-tenant`.
- Configuration writes currently do not bump `knievel.config_version`.
  Do not claim live write-triggered snapshot refresh unless code and tests land
  with that behavior.

## Tests and frontend gates

Choose the narrowest useful test while iterating, then run the affected gates:

- Rust/API: package or named test, followed by formatting and clippy as needed.
- DB/API changes: relevant `api_*` or `integration_*` slice with a real
  `DATABASE_URL`; self-skipping output is not a successful DB validation.
- OpenAPI: `openapi --check`, `check-api-doc`, `check-snake-case`, and
  `ui-client --check` when schemas affect the admin client.
- Admin UI: `pnpm --dir web/admin typecheck`, `lint`, `format:check`,
  `test --run`, and `build` for affected frontend work.
- Helm/Compose: render or lint the artifact, not just its YAML syntax.

New project-scoped operations require an entry in
`tests/cross_tenant_manifest.toml` and an executable negative test. The manifest
gate proves registration, not that a named test body exists.

## Documentation and delivery

- Every PR states its documentation impact. Update current docs whenever
  behavior, API, config, deployment, UI trust boundaries, migrations, or release
  artifacts change. Preserve historical records behind explicit status banners.
- Use clickable relative links for repository Markdown and run
  `cargo xtask check-doc-fences` plus the offline lychee gate.
- All changes go through a branch and pull request; do not push work directly to
  `main` and do not merge your own PR unless explicitly authorized.
- Use conventional commit subjects (`docs:`, `fix:`, `feat:`, `chore:`). Phase
  numbers are historical labels, not the commit protocol.
- Releases are explicit operator actions. Never create or push a `v*` tag,
  publish an image/chart/gem, or invoke release automation unless the request
  specifically asks for a release.
