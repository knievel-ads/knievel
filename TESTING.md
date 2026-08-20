# Knievel Testing

How knievel is tested — unit through end-to-end, plus the security
gates that block release. Companion to `REQUIREMENTS.md`, `API.md`,
`AUTH.md`, and `REPORTING.md`.

This document is the working spec for the test suite. It is meant to
be precise enough that a contributor can answer "where should this
test live?" without guessing, and that a release engineer can
answer "what blocks the tag?" without reading code.

## 1. Goals

1. **Correctness over coverage.** Tests exist to encode invariants we
   refuse to ship without. Line coverage is a side effect, not a
   target.
2. **Fast feedback on the hot path.** Decision-selection and
   snapshot-loader tests are pure-Rust unit tests that run in
   milliseconds. The full suite is parallelizable; `cargo nextest run`
   on a developer laptop completes in under 60 s when the DB harness
   is warm.
3. **Tenant isolation receives explicit checks.** Database/API tests and
   `cargo xtask check-cross-tenant` provide useful evidence. They do not make
   every tenancy claim in this design document automatically true.
4. **The OpenAPI spec is contract-tested.** Every public endpoint is
   exercised through the same surface generated clients use. Spec
   drift between the binary, `openapi.yaml`, and the generated Ruby
   gem is caught in CI.
5. **Acceptance tests describe user journeys**, not endpoints. The
   suite reads like the rollout shape in `MIGRATION_RX.md`: provision,
   sync, decide, observe.
6. **Deferred coverage is named honestly.** Ignored acceptance and chaos
   skeletons remain visible without being counted as executed behavior.

## 2. Non-Goals (v0)

- **100% line coverage.** Coverage above 80 % on the selection
  algorithm and auth layer; coverage of generated boilerplate is
  uninteresting.
- **Mutation testing.** Maybe later (e.g. `cargo-mutants`); not
  release-blocking in v0.
- **Fuzzing.** No fuzz targets or CI lane are active today.
- **Cross-browser coverage.** Nightly exercises Chromium only.
- **Cross-cloud E2E.** The acceptance suite runs against
  containerized Postgres; Aurora-specific behaviors (failover, NOTIFY
  drop on leader change) are simulated, not exercised against a real
  cluster.

## 3. Executed layers

A test belongs at the lowest layer that can prove the property. Current CI runs
library/binary unit tests, `integration_*` and `api_*` targets against Postgres
16, seven active in-process acceptance scenarios, UI checks, and one packaged
image/gem smoke. Counts and timing vary; this document does not invent fixed
coverage totals.

The 23 ignored acceptance scenarios and nine ignored chaos skeletons are design
inventory, not higher layers in an executed pyramid.

## 4. Unit Tests

Pure-Rust, no I/O, no async runtime spin-up beyond `tokio::test`
where genuinely needed. Organized as `#[cfg(test)] mod tests`
co-located with the code under test (Rust convention).

### 4.1 What lives here

| Module | Properties tested |
|---|---|
| `selection::filter` | Site/zone/ad-type predicates; `force.*` overrides; `block.*` exclusions; date-window evaluation. |
| `selection::weighted` | Weighted-random selection with a seeded `StdRng` — same seed → same selection; weight 0 never selected; single-candidate tier always selects. |
| `selection::priority` | Highest non-empty tier wins; empty tier falls through. |
| `hmac::sign` / `hmac::verify` | Round-trip, TTL expiry, payload tampering rejected, base64url encoding. |
| `hmac::rotation` | Two-secret window: previous secret accepted within 8 h, rejected after. |
| `auth::opaque::parse` | Format detection (`kvl_` prefix), scope/env extraction, malformed strings rejected. |
| `auth::opaque::hash` | argon2id round-trip; constant-time comparison; mismatched hash rejected. |
| `auth::jwt::validate` | Algorithm allow-list (asymmetric only); `alg: none` rejected; `kid` lookup; expiry / nbf / iat with skew tolerance; audience and issuer checks; `knievel` claim presence and shape. |
| `auth::jwt::claim_mapping` | First-match-wins; multi-key matches; default reject when no rule matches. |
| `auth::lint` | Boot-time validation from `AUTH.md` "Startup Linting": every hard-fail rule is exercised with a malformed config. |
| `idempotency::key` | Hash stability across body whitespace; replay match keyed on `(project, key, route, body-hash)`. |
| `config::layering` | Defaults < file < env override; `${VAR}` and `${VAR:default}` interpolation; missing required `${VAR}` is a hard error. |
| `partitions::names` | `events_raw_p<YYYY_MM_DD>` parsing/round-trip; rejects malformed leaf names. |
| `events::dedup_key` | Stable per `(kind, signature_nonce)`; truncation length; survives signing-secret rotation. |

### 4.2 Property tests

`proptest` is used where the input space is wider than table-driven
cases comfortably cover:

- **Selection algorithm**: for any (flights, ads, blocklist) input,
  the selected ad is always in the highest non-empty priority tier
  and is not in the blocklist. Run with 10 000 cases per CI.
- **HMAC payload**: any byte mutation of a signed URL fails
  verification.
- **Idempotency hash**: any two requests with identical canonical
  JSON produce identical keys regardless of map ordering.

### 4.3 What does not live here

- Anything that touches a real DB connection, real HTTP, or real
  filesystem — those live at §5 or above.
- Tests of derived `serde` impls. Trust the framework.

## 5. Integration Tests

Narrow scope, but with real moving parts: a real Postgres for DB
tests, a real `tokio` runtime for async-state-machine tests. Each
test owns its preconditions; no cross-test fixture state.

### 5.1 Database harness

[`sqlx::test`](https://docs.rs/sqlx/latest/sqlx/attr.test.html)
attribute creates a fresh, named, throwaway DB per test from a
template, runs migrations, and tears down on completion. Test runs
in transactional isolation when the test only reads/writes data; in
a real database when the test exercises DDL (partitions, RLS).

```rust
#[sqlx::test(migrations = "./migrations")]
async fn snapshot_loads_after_notify(pool: PgPool) -> Result<()> {
    // ...
}
```

A small wrapper (`testlib::db::ephemeral`) handles the cases
`sqlx::test` doesn't cover — multi-connection setups for
`LISTEN/NOTIFY` and advisory-lock leader-election tests.

Containerized Postgres is provided by
[`testcontainers`](https://docs.rs/testcontainers/) when running
locally without a host Postgres. CI uses a service container for
speed.

### 5.2 What lives here

| Subsystem | Properties tested |
|---|---|
| **Migrations** | All migrations apply cleanly to an empty DB; idempotent re-runs; rollback safety where applicable; `_sqlx_migrations` lives in `knievel` schema. |
| **Snapshot loader** | Cold load from a populated DB matches expected `(project_id, resource)` map. `NOTIFY config_changed` triggers a diff-pull and atomic swap. Poll backstop (5 s) catches a missed NOTIFY. Aurora-failover simulation: drop the LISTEN connection mid-test and verify reconnect-with-backoff completes within the budget. |
| **Event flusher** | Channel → `COPY` round-trip lands rows in the correct daily partition. Channel saturation surfaces as an error to the caller, never silent loss. Graceful shutdown drains the channel before exit. |
| **Partition manager** | Premake creates 4 days of future partitions. Retention drops partitions older than `retention_days`. `DETACH PARTITION CONCURRENTLY` succeeds under concurrent inserts. Idempotent — running maintenance twice in a row is a no-op. |
| **Leader election** | `pg_try_advisory_lock` acquires once per cluster. Closing the leader's connection releases the lock; a follower acquires within 30 s. The watchdog exits the process on a missed maintenance window. |
| **HMAC verifier under rotation** | Mint URL with secret v1, rotate to v2, verify within 8 h: ok. Verify after 8 h + 1 s: `400 expired`. Mint with v2, verify with v1 in cache: rejected. |
| **Audit log** | Every `force.*` decision, secret rotation, project-deletion, member-role change, token mint/revoke writes a row with the documented schema. Append-only — `UPDATE` on `audit_log` is denied by RLS policy. |
| **Idempotency cache** | Replay returns the original 200/201; replay with a different body returns `409 idempotency_conflict`; cache-miss after TTL re-executes. |

### 5.3 RLS verification at the DB layer

A targeted suite exercises RLS policies at the SQL level, separate
from the cross-tenant API tests in §6.5:

- For each table in `knievel`, verify `relrowsecurity = true` and
  `relforcerowsecurity = true` (the `FORCE` matters — without it
  table owners bypass policies).
- For each policy, verify the `USING` clause references
  `current_setting('knievel.project_id')` (or the documented
  session-scoped tenant binding).
- A direct-SQL probe with two `SET LOCAL knievel.project_id = ...`
  values confirms isolation: rows inserted under project A are
  invisible to a session bound to project B.

These tests overlap with the migration linter (§ 9.2) but cover the
runtime behavior; the linter covers the SQL text.

## 6. API / Contract Tests

The largest layer by test count. Every endpoint declared in `API.md`
is exercised through `poem::test::TestClient`, which routes against
the same handler stack production uses — same auth extractors, same
idempotency middleware, same OpenAPI surface — but skips the network
hop.

### 6.1 Test client + auth fixtures

```rust,ignore
let app = knievel::test_app(pool).await;
let resp = app
    .post("/v1/projects/pj_demo/decisions")
    .bearer_auth(token::editor_for(org, project))
    .json(&decision_request())
    .send()
    .await;

resp.assert_status_is_ok();
resp.assert_json(&expected);
```

Token fixtures (`token::editor_for`, `token::reader_for`,
`token::wrong_project`, `token::wrong_org`, `token::expired_jwt`,
`token::sa_for_namespace`) cover every (mode × scope × role)
combination so any handler test can ask for the credential it needs
in one line.

### 6.2 Response-shape contracts

Every successful response is asserted with [`insta`](https://docs.rs/insta/)
snapshot tests. A handler-shape change requires an explicit
`cargo insta review` step; accidental field renames or type changes
fail CI loudly.

Error responses are snapshot-tested too — `error.code` is part of
the public contract per `API.md` § 4 "Error body."

### 6.3 OpenAPI spec drift

A single test compiles the binary's runtime OpenAPI document, diffs
it against `openapi.yaml` checked into the repo, and fails if they
disagree. The fix is `cargo xtask openapi`. This is the gate that
keeps the spec, server, and generated client in lockstep.

A second test asserts that `openapi.yaml` validates against the
OpenAPI 3.1 meta-schema — so a malformed spec never reaches the
generator.

### 6.4 Per-resource CRUD contracts

Each project-scoped resource (Advertiser, Campaign, Flight, Ad,
Creative, CreativeTemplate, Site, Zone, plus AdLibraryItem at org
level) has a uniform suite generated from a single `crud_contract!`
macro:

| Test | Property |
|---|---|
| `create_returns_201` | Server-assigned `id`, echoed `external_id`, `etag`, `created_at`/`updated_at`. |
| `create_idempotent_on_external_id` | Second create with same `external_id` is a no-op returning the first row. |
| `create_idempotency_key_replay` | Same `Idempotency-Key` returns cached body with `Idempotent-Replay: true`. |
| `create_idempotency_key_mismatch_body` | Same key + different body → `409 idempotency_conflict`. |
| `read_404_unknown_id` | Unknown `id` and unknown `external_id` both 404. |
| `update_etag_match` | `If-Match: <etag>` succeeds; stale etag → `409 if_match_mismatch`. |
| `list_paginates` | `limit` honored; `next_cursor` returns the next page; cursor stable across writes. |
| `filter_by_external_id` | `?external_id=...` filter narrows to one row. |
| `soft_delete` | `is_active: false` round-trips; `GET` still returns the row. |
| `batch_upsert_atomic` | One bad row in a batch rolls back all rows; `details[]` reports the offending index. |
| `cross_entity_fks_in_batch` | A flight referencing a campaign created earlier in the same batch resolves. |

Adding a new resource means adding one `crud_contract!` invocation;
the macro emits the full table.

### 6.5 Cross-tenant negative tests (release-blocking)

`REQUIREMENTS.md` §7.1.1 gate (1). Every project-scoped endpoint has
a paired test:

```rust
#[knievel_test]
async fn cross_tenant_403_advertiser_get(ctx: TestCtx) {
    let (_org, project_a) = ctx.org_with_project().await;
    let (_org_b, project_b) = ctx.org_with_project().await;

    let advertiser = ctx
        .as_editor_of(&project_a)
        .create_advertiser("acme")
        .await;

    let resp = ctx
        .as_editor_of(&project_b)            // wrong project, same org? no — diff org.
        .get(&format!("/v1/projects/{}/advertisers/{}",
                      project_a.id, advertiser.id))
        .await;

    resp.assert_status(403);
    resp.assert_error_code("wrong_tenant");
}
```

`#[knievel_test]` is a custom attribute that registers the endpoint
under test in a CI manifest. A separate `cargo xtask check-cross-tenant`
binary walks the OpenAPI spec, lists every `/v1/projects/{p}/...`
operation, and **fails the build** if any is missing a paired
cross-tenant test. New endpoints cannot land without one.

The same harness covers `wrong_project` (same org, wrong project for
a project-scoped token) and `role_insufficient` (a `reader` calling
a write endpoint).

### 6.6 Auth path tests

Per `AUTH.md`:

- **Opaque tokens**: valid token → 200; revoked token → 401; wrong
  scope → 403; ip-allowlist mismatch → 403.
- **JWT**: valid signature → 200; bad signature → 401; expired → 401;
  wrong audience → 401; wrong issuer → 401; missing `kid` → 401;
  `alg: none` → 401; `HS256` → 401; missing `knievel` claim → 401;
  malformed `knievel` claim → 401.
- **JWKS rotation**: a new `kid` triggers a JWKS refresh; cache hits
  before TTL; cache refresh on TTL.
- **Claim mapping**: first-rule-wins; no-match → 401.
- **Boot-time lint**: every malformed-config path from
  `AUTH.md` "Startup Linting" hard-fails the binary; happy path
  emits the expected `INFO` line and `/version` payload.

A mocked OIDC provider — `wiremock` standing in for Keycloak and the
Kubernetes API server — provides `/.well-known/openid-configuration`
and `/jwks.json` so JWT tests run hermetically without external
network.

### 6.7 Decision-API specifics

The decision endpoint gets its own focused suite on top of the
generic CRUD harness:

| Test | Property |
|---|---|
| `decisions_empty_when_no_eligible_ads` | `decisions[<id>] = []`, never null, never absent. |
| `decisions_select_by_priority_tier` | Higher tier always wins over lower. |
| `decisions_weighted_random_with_seed` | Deterministic given a seeded RNG; weight 0 never selects. |
| `decisions_block_creative_ids` | Blocked creative excluded post-priority-grouping. |
| `decisions_force_admin_only` | Editor → 403; admin without flag → 403; admin with flag → 200 + audit row. |
| `decisions_force_global_kill_switch` | `decisions.force_overrides_enabled: false` → 403 cluster-wide. |
| `decisions_force_audit_row` | Forced decision writes one and only one `audit_log` row with actor, payload hash, reason. |
| `decisions_site_resolution` | `site_id`, `site_url`, and `site_external_id` all resolve to the same `site_id` in the response. |
| `decisions_url_alias_match` | Site `aliases` resolve identically to canonical `url`. |
| `decisions_signed_urls_round_trip` | Minted impression/click URLs verify with the per-project secret and TTL. |
| `decisions_snapshot_version_stamp` | Response `snapshot_version` matches the snapshot at request time and ends up on the corresponding `events_raw` row. |
| `decisions_explain_no_event_recorded` | `:explain` mints dummy URLs and writes no events. |
| `decisions_explain_evaluation_shape` | Every candidate has a deterministic `evaluation` array. |

### 6.8 Event-tracking specifics

| Test | Property |
|---|---|
| `impression_204_default` | `GET /e/i/<sig>` → 204 on a fresh sig. |
| `impression_gif_when_requested` | `?fmt=gif` → 200 with the 43-byte transparent GIF. |
| `impression_tampered_204_silent` | Tampered sig → 204, internal `tampered` counter increments. |
| `click_302_redirect` | `GET /e/c/<sig>` → 302 to the creative's `click_through_url`. |
| `click_open_redirect_blocked` | `?u=<url>` is honored only when signed in. |
| `dedup_first_hit_countable` | First hit lands `is_duplicate = false`. |
| `dedup_second_hit_marked` | Second hit with same sig lands `is_duplicate = true`; click still 302s. |
| `dedup_spans_secret_rotation` | `dedup_key` is stable across the 8-h rotation overlap. |

### 6.9 Reporting-shape contracts

Per `REPORTING.md`:

- `events_rollup_watermark` advances monotonically.
- A `WHERE NOT is_duplicate` count of `events_raw` for a window
  matches the same window's `events_rollup` total once the watermark
  has caught up.
- `events_rollup` never includes `is_duplicate = true` rows.
- The `knievel_reader` role can `SELECT` from `knievel.*` and cannot
  `INSERT` / `UPDATE` / `DELETE` on any of them.

## 7. E2E Acceptance Suite

`tests/acceptance.rs` currently runs in-process through
`poem::test::TestClient` against the same Postgres 16 service used by the API
and integration lanes. It is not a black-box compose or packaged-image test.

The file declares 30 ACC scenarios. The observed split is **7 active**
(ACC-01, 02, 05, 06, 08, 17, and 30) and **23 `#[ignore]`d** scenarios. Normal
nextest invocation does not execute those ignored bodies. The table below is
the intended scenario catalog; it must not be read as 30 implemented checks.

### 7.1 Scenarios

Each scenario is a top-to-bottom user journey. Tests assert on
external behavior only — HTTP responses, DB rows visible to
`knievel_reader`, files in object storage, log lines, OTel spans.
No reaching into knievel internals.

| ID | Scenario | Source of truth |
|---|---|---|
| ACC-01 | Provision an Org and a Project, mint an Org Editor token, list the empty project. | `API.md` § 2.1, 2.2 |
| ACC-02 | Full demand chain: Advertiser → Campaign → Flight → Ad → Creative; issue a decision; assert the response shape and HMAC URLs. | `API.md` § 1, 3.1–3.5 |
| ACC-03 | Site lookup via `site_url` and `site_external_id` returns the same `site_id`. | `API.md` § 1, 3.7 |
| ACC-04 | URL aliases: a site with two aliases resolves all three URLs identically. | `API.md` § 3.7 |
| ACC-05 | Bulk sync: a single `:batchUpsert` call lands a coherent advertiser/campaign/flight/ad/creative graph. | `API.md` § 4 "Write contract" |
| ACC-06 | Bulk sync failure: one bad row rolls back the whole batch with per-row diagnostics. | `API.md` § 4 |
| ACC-07 | Idempotency: replay returns cached, body-mismatch is `409`. | `API.md` § "Idempotency" |
| ACC-08 | Soft delete via `is_active: false` round-trips. | `API.md` § "Common entity fields" |
| ACC-09 | Hot path: 1 000 sequential decisions; p99 < 50 ms (informational; SLO bench is § 8). | `REQUIREMENTS.md` § 9 |
| ACC-10 | Snapshot refresh: management write → `NOTIFY` → next decision sees new state within 5 s. | `REQUIREMENTS.md` § 7.2 |
| ACC-11 | Snapshot poll backstop: management write with NOTIFY suppressed → next decision sees new state within 6 s (poll interval + slack). | `REQUIREMENTS.md` § 7.2 |
| ACC-12 | Ad Library reference: org item → project ad reference → decision returns library content; mutate item → decision returns updated content within 5 s. | `API.md` § 2.4, `REQUIREMENTS.md` § 5.1 |
| ACC-13 | HMAC rotation: rotate, verify both old and new URLs work for 8 h overlap, old fails after. | `REQUIREMENTS.md` § 6.3 |
| ACC-14 | Impression + click round-trip: minted URL → event row in `events_raw` with the right `snapshot_version` and `dedup_key`. | `API.md` § 4 |
| ACC-15 | Replay dedup: hit the same impression URL twice → two rows, second `is_duplicate = true`. | `API.md` § "Replay, dedup, and counts" |
| ACC-16 | Click replay still redirects: hit click URL twice → both 302, second `is_duplicate = true`. | `API.md` § "Replay, dedup, and counts" |
| ACC-17 | Cross-tenant: project A token → project B → 403, `error.code = wrong_tenant`. | `REQUIREMENTS.md` § 7.1.1 (1) |
| ACC-18 | JWT path (Keycloak stand-in): valid token → 200; expired → 401; wrong audience → 401. | `AUTH.md` § "JWTs" |
| ACC-19 | K8s SA path: valid SA JWT for namespace `rx-prod` → mapped principal → 200; SA from unmapped namespace → 401. | `AUTH.md` § "Kubernetes ServiceAccount Tokens" |
| ACC-20 | Mode coexistence: opaque + JWT both enabled, both succeed in the same test run. | `AUTH.md` § "Mixing Modes During Cutover" |
| ACC-21 | Force overrides: editor → 403; admin with flag off → 403; admin with flag on → 200 + audit row. Global kill-switch overrides project setting. | `API.md` § 1, `AUTH.md` § "Endpoint → minimum role" |
| ACC-22 | Image upload: `POST /creatives/{id}/image` lands an object in MinIO; subsequent `GET /creatives/{id}` returns the URL; SVG upload → 415. | `REQUIREMENTS.md` § 7.9 |
| ACC-23 | Partition lifecycle: today's partition exists; partitions for `today + 4d` are pre-made; retention drop happens at the maintenance tick. | `REQUIREMENTS.md` § 7.4 |
| ACC-24 | Leader election: kill the current leader's container → another pod takes over within 30 s; partition maintenance still runs. | `REQUIREMENTS.md` § 7.5 |
| ACC-25 | Degraded — DB writer unreachable: pause the writer connection; decisions still serve from the snapshot; writes return `503 db_writer_unreachable`; recovery clears the failure. | `REQUIREMENTS.md` § 10.9 |
| ACC-26 | Degraded — snapshot stale: stop refreshing the snapshot; reads carry `X-Knievel-Stale-Snapshot`; > 300 s → `/readyz` returns 503. | `REQUIREMENTS.md` § 10.9 |
| ACC-27 | Degraded — event channel saturated: throttle the flusher; channel fills; decisions return `503 event_channel_saturated`; flusher recovery clears the failure. | `REQUIREMENTS.md` § 10.9 |
| ACC-28 | Reporting: a decision + impression + click chain produces the expected rows in `events_raw`; after a forced rollup tick, `events_rollup` aggregates the non-duplicate count. | `REPORTING.md` § "Schema for Reporters" |
| ACC-29 | `knievel_reader` role: can `SELECT` from `knievel.*`, cannot `INSERT` / `UPDATE` / `DELETE`; new tables created by future migrations are reachable via default privileges. | `REPORTING.md` § "Access Pattern" |
| ACC-30 | OpenAPI spec served at `/openapi.json` matches the committed `openapi.yaml`; `/version` carries the documented `auth` block (no secrets). | `API.md` § 5, `AUTH.md` § "Effective-policy visibility" |

CI partitions the discovered `acceptance` target across four nextest
count shards. Today that partitions the seven active tests; the 23 ignored
tests remain ignored. `cargo xtask test-shape` reports this split but proves
only target classification and discoverability, not behavior or ignored-test
execution.

### 7.2 Generated-client smoke pass

The Ruby gem (`knievel-ruby`, `REQUIREMENTS.md` § 8 item 3) ships
its own RSpec suite hitting the same compose stack. The platform
CI runs the smoke subset (provision, sync, decide, paginate) so
gem-server skew is caught before tag, not at integration time.

### 7.3 What the suite is not

- **Not a load test.** § 8 is.
- **Not a test of operator-supplied infrastructure** (e.g. real
  Aurora). The compose Postgres stands in. Aurora-specific properties
  (failover semantics, NOTIFY drop on leader change) are simulated in
  § 5.
- **Not a chaos suite.** § 9 is.

## 8. Performance and Capacity

`REQUIREMENTS.md` § 9.2 is the source of truth. Summary of how the
test suite plugs in:

- **Macro bench harness** (`bench/macro/`): `vegeta` driving a
  knievel binary built in `--release` against a synthetic project
  with 100 k active flights (`bench/macro/seed.sh`). Operator-run
  on dedicated hardware; not part of CI.
- **Micro + heap bench harness** (`benches/`): criterion (wall-
  clock) + iai-callgrind (deterministic CPU instructions / cache
  misses) + dhat-rs (heap allocations) covering the
  `selection::*` inner loop, `hmac::verify`, and the full
  Postgres-free `decisions::decide_pure` path.
- **Runner**: **Claude Code cloud sessions, not CI.** A session
  invokes `cargo xtask bench-all`, which reads the workspace
  version from `Cargo.toml`, runs the entire suite, captures a
  host fingerprint via `cargo xtask bench-env`, and writes
  `bench/results/v<MAJ>.<MIN>.{md,json}` matching the schema in
  `bench/results/SCHEMA.md`. Procedure documented in
  `bench/README.md`.
- **Trigger**: the release-tagging PR for any release that
  bumps the workspace minor version MUST include the new
  `bench/results/v<X>.json` entry; macro numbers in the
  `macro` slot can be back-filled by an operator before the
  tag fires.
- **Reportable artifact**: pinned by `bench/results/SCHEMA.md`.
  Top-level keys: `env` (CPU, memory, kernel, governor, rustc),
  `micro_criterion` (wall-clock per fixture), `micro_iai`
  (instruction counts per fixture), `heap_dhat` (allocations
  per decision), `macro` (vegeta p50/p95/p99/QPS plus
  concurrent CPU/RSS sampling).

### Regression policy

Per signal:

| Signal | Threshold | Blocking? |
|---|---|---|
| `iai-callgrind` `events.Ir` | > 5% | Issue (deterministic; any drift is real) |
| `criterion` micro `mean_ns` | > 30% | Issue |
| `criterion` macro `p50_ms` / `p99_ms` | > 20% | Release tag |
| `criterion` macro `throughput_qps` | > 20% | Release tag |
| `dhat` `total_bytes` | > 30% | Issue |

`cargo xtask bench-all --against v<prev>` reads the previous
release's JSON and prints a markdown delta table; paste it into
the release-tagging PR. The check is agent-driven, not gated by
a workflow.

The deterministic instruction counter is what makes the
historical record portable across runners — wall-clock is
±20% on cloud runners, but `events.Ir` is bit-identical across
identical source on identical rustc. That's what lets
`bench/results/v<X>.json` deltas survive a runner change.

## 9. Chaos / Degraded-Mode Suite

There is **no executable chaos suite today**. Nine top-level
`tests/chaos_*.rs` files document the intended degraded-mode scenarios, but
each contains one ignored skeleton and `tests/chaos/` has no injection harness.
CI and nightly intentionally select none of them.

`cargo xtask test-shape` accepts `chaos_*` only as a deferred class and fails if
any scenario silently loses `#[ignore]` before a real selector and harness are
added. This is bookkeeping, not chaos coverage. See `tests/chaos/README.md` for
the activation contract and `REQUIREMENTS.md` § 10.9 for intended behavior.

## 10. Security Tests

### 10.1 Migration linter (release-blocking)

`REQUIREMENTS.md` § 7.1.1 gate (2). Implemented as `cargo xtask
lint-migrations`. Test fixtures in `xtask/tests/fixtures/migrations/`
cover every failure mode:

| Fixture | Expected outcome |
|---|---|
| `01_disable_rls.sql` | reject — `DISABLE ROW LEVEL SECURITY` |
| `02_no_force_rls.sql` | reject — `NO FORCE ROW LEVEL SECURITY` |
| `03_table_without_rls.sql` | reject — `CREATE TABLE` in `knievel` without paired `ENABLE ROW LEVEL SECURITY` |
| `04_policy_without_tenant.sql` | reject — `CREATE POLICY` whose `USING` clause doesn't reference `current_setting('knievel.project_id')` |
| `05_table_outside_knievel.sql` | accept — `public.something` is not knievel's concern |
| `06_clean_table.sql` | accept — RLS, FORCE, and tenant-bound policy all present |

The linter is unit-tested separately and called by every `ci.yml` trigger.

### 10.2 Cross-tenant API tests

§ 6.5 and active ACC-17 provide two in-process layers. The API tests cover
endpoint cases; ACC-17 covers a user-journey-shaped path. Neither is a packaged
deployment test.

### 10.3 Release security checklist

The operator checklist lives in `RELEASE_CHECKLIST.md`. It is reviewed in the
release-preparation PR; there is currently no PR-template or workflow enforcer
for checkboxes. The tag workflow instead fails closed on machine-verifiable
ancestry, version, changelog, OpenAPI, Cargo lock, and MIT LICENSE invariants.

The checklist items map to:

- §6.5 + §7 ACC-17 green → "All cross-tenant integration tests pass."
- §10.1 green → "Migration linter passes."
- A diff of `auth/*` and migrations since the last tag, with a
  maintainer's signoff comment in the PR.
- Manual review of handler signatures that take `org_id` / `project_id`
  from `Json<…>` bodies; tenant identity must come from path or token.
- Manual review of new logging calls for raw user agents, IP addresses outside
  `events_raw`, JWT contents, and bearer tokens.

### 10.4 Auth boot lint

§ 6.6 covers the current unit-level cases. Active ACC-30 checks OpenAPI and
version behavior; it does not test malformed-config process exit.

### 10.5 Fuzzing (deferred)

No fuzz targets or fuzz workflow lane are active. HMAC, JWT, and request-body
fuzzing remain candidate future work and must not be counted as current nightly
coverage.

## 11. Test Data and Fixtures

### 11.1 `seed-demo` is the canonical fixture

`knievel-cli seed-demo` (`REQUIREMENTS.md` § 8 item 4) populates a
demo Org / Project / Site / Zone / Advertiser / Campaign / Flight /
Ad / Creative chain. Acceptance tests start from this state — no
duplicate fixture code in tests.

A flag (`--reproducible`) freezes IDs and timestamps so insta
snapshots are stable across runs.

### 11.2 Per-test factories

Inside the test suite, fixtures are constructed by small typed
factories (`testlib::factory::*`) rather than sql files. Each
factory accepts overrides:

```rust,ignore
let advertiser = factory::advertiser(&pool, &project)
    .with_external_id("acme")
    .with_active(false)
    .insert()
    .await;
```

Factories never create cross-test state; everything is scoped to
the test's own schema.

### 11.3 Sample wire payloads

`tests/fixtures/wire/` mirrors the example bodies in `API.md` —
checked into the repo so doc and code review the same shape. A
codegen test asserts the spec's example bodies parse against their
declared schemas.

## 12. CI Pipeline and Gates

GitHub Actions owns three workflows:

- `ci.yml`: `pull_request`, pushes to `main`, and `workflow_dispatch`;
- `nightly.yml`: daily at 07:13 UTC and `workflow_dispatch`;
- `release.yml`: tag pushes matching `v*` only.

Workflow permissions default to `contents: read`; publishing jobs elevate only
the permissions they need. Every checkout disables credential persistence and
every job has a bounded timeout. x64 Linux uses `ubuntu-24.04`, Linux arm64
packaging uses native `ubuntu-24.04-arm`, and Apple Silicon uses native
`macos-15` with `MACOSX_DEPLOYMENT_TARGET=11.0`.

### 12.1 Concurrency

CI cancels superseded runs for the same ref. Nightly does not cancel in flight.
Release uses one repository-wide, non-cancelling `release` group so two tags
cannot race floating image aliases or downstream `main`.

### 12.2 Rust toolchain and caching

`rust-toolchain.toml` is the normal-toolchain authority. The local
`rust-setup` composite activates it, optionally adds target triples, supports
nested workspace/target mappings, and restores one pinned
`Swatinem/rust-cache` layer. There is no sccache wrapper and no repository CI
override of incremental compilation. A separate lane checks the explicit Rust
1.94.1 MSRV.

### 12.3 Reproducible non-Rust tooling

Node application tests use Node 22 and pnpm 10.33.0. Helm is v3.21.4,
kubeconform is v0.8.0 with a checked release SHA-256, and both Kubernetes schema
catalog URLs name immutable commits. OpenAPI Generator v7.24.0 and the
distroless runtime base are selected by multi-architecture image index digest.
External actions use reviewed full commit SHAs. The local image wrapper's
canonical syntax is `cargo xtask build-image --tag knievel:dev`.

### 12.4 CI DAG

`prime` runs formatting, clippy, test compilation, and benchmark compilation.
The required fan-out covers unit/property, Postgres integration, API contract,
xtask linters, documentation links, OpenAPI drift, UI drift/type/lint/test/build,
Helm/kubeconform, gem build plus compose smoke, four acceptance shards, Rust
1.94.1, and a non-publishing Apple Silicon CLI smoke.

The permanently named **`CI result`** job uses `if: always()` and fails unless
every listed lane reports `success`; failed or dependency-skipped lanes cannot
be hidden behind a green aggregate. Repository settings currently have no
branch protection or ruleset, so these jobs are **not claimed as required
checks**. Owners must create a ruleset requiring `CI result` before relying on
it as a merge boundary.

### 12.5 Test slicing

Top-level targets map as follows:

| Class | nextest selector | Service |
|---|---|---|
| library/binary unit | `kind(lib) + kind(bin)` | none |
| `integration_*` | `kind(test) & binary(/^integration_/)` | Postgres 16 |
| `api_*` | `kind(test) & binary(/^api_/)` | Postgres 16 |
| `acceptance.rs` | `kind(test) & binary(acceptance)` | Postgres 16 |
| `chaos_*` | none (explicitly deferred) | none |

`cargo xtask test-shape` rejects unknown top-level targets, missing active
selectors, a nightly chaos selector, or a non-ignored chaos skeleton. It proves
classification/discoverability only. It does not run ignored tests, assess
tenancy safety, or establish meaningful chaos coverage.

### 12.6 Acceptance sharding

Four `count:N/4` nextest shards run `acceptance.rs`. The observed source split
is seven active and 23 ignored, so the shards cover only those seven active
tests. They use in-process Poem endpoints and Postgres 16, not a packaged image.
The separate gem smoke builds the runtime image and exercises compose.

### 12.7 Postgres support exercised by CI

All database CI lanes use Postgres 16 only. There is no inactive 14/15 matrix
and no claim that CI proves multi-version support. The service image is selected
by a reviewed Postgres 16 multi-architecture digest.

### 12.8 Nightly (advisory)

Nightly currently runs Playwright UI e2e and UI bundle-size budgets. It does not
run chaos, fuzzing, Criterion, or a Postgres version matrix. Nightly failures are
visible workflow failures; there is no automated issue-opening lane.

### 12.9 Tag release boundary

Before compiling or executing repository code, `release.yml` fetches complete
`main` and tag history and performs a read-only data check. It accepts only
canonical `vMAJOR.MINOR.PATCH` tags without leading zeros, requires the commit
to be on `origin/main`, requires a version newer than every prior canonical
tag, and checks Cargo workspace/local lock packages, OpenAPI, changelog heading
and links, and consistent MIT metadata plus a top-level `LICENSE`.

Every image, CLI, GitHub Release, Helm, attestation, and downstream Ruby path
depends on that preflight. This is defense in depth, not a substitute for
repository controls: **no main or tag ruleset exists today**. Owners must protect
`main`, require `CI result`, block force pushes, and restrict `v*` tag creation
before the first release under this workflow. A malicious tag can otherwise
supply its own workflow and bypass in-file checks.

Release reruns are not generally idempotent. GHCR uploads or a GitHub Release
may already exist, and the downstream Ruby tag intentionally refuses reuse.
Use `RELEASE_PLAYBOOK.md`; never move or overwrite a published tag.

### 12.10 Workflow file layout

```
.github/
├── workflows/{ci,nightly,release}.yml
└── actions/
    ├── node-setup/action.yml
    └── rust-setup/action.yml
```

## 13. Coverage Policy

There is no percentage threshold and no `cargo llvm-cov` workflow artifact.
Coverage is reviewed through concrete tests and the explicit gaps in §§ 7, 9,
and 15. Adding a skeleton or selector does not count as behavioral coverage.

## 14. Local Developer Workflow

```sh
cargo test --workspace --locked
cargo xtask lint-migrations
cargo xtask check-cross-tenant
cargo xtask test-shape
cargo xtask openapi --check
pnpm --dir web/admin test --run
```

Database-backed targets need a reachable Postgres 16 and the same
`DATABASE_URL`/role provisioning used in `ci.yml`. There are no `just
acceptance` or watch recipes in this repository today.

## 15. What Tests Don't Catch

Honest list, kept maintained, called out in code review:

- **Real Aurora failover semantics.** Simulated, not exercised.
- **Real Keycloak token-mapper edge cases.** Wiremock stand-in.
  Misconfigured mappers in real Keycloak → manifest as `401
  invalid_token / claim_missing` and are caught at integration time
  by the operator, not by us.
- **Browser ad-blocker interactions** with `/e/...` URLs. v0 doesn't
  ship browser-direct mode; this comes back when it does.
- **CDN cache behavior** for impression GIFs. Operator-owned.
- **dbt model correctness against `events_raw`.** We assert the
  schema and the watermark contract; we don't run the consumer's
  dbt. The `examples/dbt/` skeleton is `dbt parse`'d in CI to catch
  syntactic drift, no more.
- **Real S3 bucket policies.** MinIO stand-in; bucket-policy
  configuration is operator-owned.
- **Long-running leak detection.** `tokio-console` is wired in dev
  but not driven by CI.

These live in operator-side smoke tests, not knievel's CI. The doc
that makes them visible is this one — adding to the list is a
deliberate acknowledgement, not a quiet oversight.

## References

- [`sqlx::test`](https://docs.rs/sqlx/latest/sqlx/attr.test.html) — ephemeral test databases
- [`testcontainers-rs`](https://docs.rs/testcontainers/) — managed Postgres containers
- [`poem::test`](https://docs.rs/poem/latest/poem/test/index.html) — handler-level test client
- [`insta`](https://docs.rs/insta/) — snapshot tests
- [`wiremock`](https://docs.rs/wiremock/) — HTTP stand-ins for OIDC/JWKS
- [`proptest`](https://docs.rs/proptest/) — property-based tests
- [`criterion`](https://docs.rs/criterion/) — micro-benchmarks
- [`cargo-nextest`](https://nexte.st) — parallel test runner
- [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) — coverage reports
- [`cargo-fuzz`](https://rust-fuzz.github.io/book/cargo-fuzz.html) — fuzzing
- [`vegeta`](https://github.com/tsenart/vegeta) / [`k6`](https://k6.io) — load generation
- [OpenAPI 3.1 meta-schema](https://spec.openapis.org/oas/3.1/schema/2022-10-07.html)
