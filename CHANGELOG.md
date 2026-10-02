# Changelog

All notable changes to knievel are documented in this file. Format
follows [keep-a-changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows the additive-forever compatibility policy in
`REQUIREMENTS.md` § 6.4.

Release entries are maintained from immutable Git tags and commit
history. `PHASES.md` is historical planning context, not current
release provenance.

## [Unreleased]

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.37] — 2026-08-20

### Changed

- Aligned all workspace packages and OpenAPI metadata on version
  `0.1.37`; the generated admin-UI client is unchanged because it does
  not represent OpenAPI `info.version`.
- Declared Rust 1.94 as the supported floor, enabled resolver 3, and
  pinned the development toolchain to 1.97.1 while preserving the
  Rust 1.94.1 compatibility check.
- Centralized SQLx 0.8.6 feature selection and replaced its deprecated
  combined Tokio/rustls alias with the equivalent explicit Tokio and
  rustls ring/WebPKI features.
- Refreshed the compatible lock graph for `anyhow`,
  `astral-tokio-tar`, `crossbeam-epoch`, `event-listener`, `h2`,
  `quinn-proto`, `spin`, and all three existing `rand` generations.

### Security

- Documented narrow audit exceptions for Poem's unused XML stack and
  inactive SQLx MySQL RSA record. Benchmark-only `bincode` and
  `proc-macro-error2` maintenance warnings remain visible.

## [0.1.36] — 2026-05-30

### Added

- Populated per-pod decision snapshots with transaction-local reads
  through a dedicated `BYPASSRLS` loader role.
- Rendered Liquid templates at decision time and returned typed
  creative payloads with correctly attributed creative IDs.

### Fixed

- Re-seeded missing taxonomy for projects orphaned by an interrupted
  `seed-demo` run.
- Made cross-tenant rollups use the loader role, tolerate null
  dimensions, and clamp stale cold-start watermarks.

## [0.1.35] — 2026-05-29

### Added

- Evaluated per-issuer JWT claim-mapping rules so Kubernetes service
  account tokens without a `knievel` claim can map to principals.

## [0.1.34] — 2026-05-10

### Added

- Added idempotent `knievel-cli admin create-org` production tenant
  bootstrap without demo resources.

## [0.1.33] — 2026-05-10

### Changed

- Removed redundant merge-to-main and image-build CI work already
  covered by the protected pull-request gate.

### Fixed

- Probed for pre-provisioned PostgreSQL schemas and extensions before
  privileged DDL so Aurora deployments can migrate with restricted
  roles.

## [0.1.32] — 2026-05-10

### Added

- Added structured per-request logs, request IDs, slow-request
  reporting, and visible JWT/JWKS rejection reasons.

### Changed

- Made database startup retry with bounded backoff, report complete
  error chains and operator hints, and fail fast when required setup
  cannot complete.

## [0.1.31] — 2026-05-09

### Added

- Wired JWT bearer authentication to OIDC discovery and cached JWKS
  signature verification.
- Added a testcontainer Keycloak round trip with invalid-signature and
  unrelated-key rejection cases.

## [0.1.30] — 2026-05-09

### Fixed

- Pointed OIDC callback and post-logout redirects at the `/admin/`
  mount.

## [0.1.29] — 2026-05-09

### Fixed

- Preserved the serialized admin route in authentication `return_to`
  values by using TanStack Router's `location.href`.

## [0.1.28] — 2026-05-09

### Fixed

- Shared Docker registry credentials between Helm and Cosign so
  published OCI charts can be signed.

## [0.1.27] — 2026-05-08

### Added

- Published versioned, keyless-signed Helm OCI charts to GHCR after
  the matching image is available.

## [0.1.26] — 2026-05-08

### Added

- Exposed admin-UI OIDC issuer, client, scopes, and enforcement values
  through the Helm chart.

## [0.1.25] — 2026-05-08

### Changed

- Trusted the protected pull-request gate for releases and moved
  multi-architecture image builds from QEMU to native runners while
  retaining one canonical Dockerfile build path.

## [0.1.24] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

- **Admin UI loads correctly under `/admin/`.** Vite was emitting
  root-anchored `<script src="/assets/...">` while the poem
  server mounts the SPA bundle under `/admin/`, so every asset
  404'd and the page rendered blank. Set `base: '/admin/'` in
  `web/admin/vite.config.ts` and `basepath: '/admin'` on the
  TanStack Router so client-side `navigate({ to: '/login' })`
  produces `/admin/login`, matching the server-side mount in
  `mount_admin_ui` (`src/server.rs`). Regression-guarded by a
  post-build asset-path check (`web/admin/scripts/check-base-path.mjs`)
  and a Playwright smoke test.

## [0.1.23] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.22] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.21] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.20] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.19] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.18] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.17] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.16] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.15] — 2026-05-08

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.14] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.13] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.12] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.11] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.10] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.9] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.8] — 2026-05-07

### Added

(none)

### Changed

(none)

### Fixed

(none)

## [0.1.7] — 2026-05-07

### Added

- Phase 5 documentation set: `README.md` (`5.1`), `ARCHITECTURE.md`
  (`5.2`), `DEPLOYMENT.md` (`5.3`), `CONTRIBUTING.md` /
  `SECURITY.md` / `CHANGELOG.md` (`5.4`), `RELEASE_CHECKLIST.md` /
  `RELEASE_PLAYBOOK.md` (`5.5`).
- `xtask check-doc-fences`, `xtask check-api-doc`, lychee link
  checking in CI (`5.6`).
- First benchmark run + `bench/results/v0.1.md` (`5.7`).

### Changed

(none)

### Fixed

(none)

## [0.1.6] — 2026-05-06

### Added

- **Phase 4.10:** End-to-end gem smoke against the compose stack as
  a step in `release-ruby-gem.yml`. Closes
  `REQUIREMENTS.md § 8` item 3 ("a third party can integrate from
  the gem alone"). Local equivalent: `docker compose up && ruby
  examples/compose/gem_smoke.rb`. Phase 4.10 marked `[x]`.

## [0.1.5] — 2026-05-06

### Added

- **Phase 4.10 (partial):** Hand-written `Enumerable` wrapper layer
  in `knievel-ruby` — `Knievel::Resources::*` (one per paginated
  resource) and `Knievel::Client` (URL-parsing facade). 24 rspec
  examples cover cursor walks, `lazy.first(n)` short-circuit,
  filter forwarding, page-size validation.
- `.openapi-generator-ignore` extended (canonical version in
  `.github/ruby-client/`) to protect wrapper paths through
  regeneration.

## [0.1.4] — 2026-05-06

### Added

- **Phase 3.33:** Server-side cursor pagination on the eight
  demand+inventory list endpoints (`advertisers`, `campaigns`,
  `flights`, `ads`, `creatives`, `creative_templates`, `sites`,
  `zones`). `?limit=N&cursor=<opaque>` per `API.md` § "Pagination";
  default 50, max 500.
- `src/pagination.rs` core: `base64url(JSON{kind, last_id})`
  cursor with kind validation (cross-resource replay → `400
  invalid_cursor`); `?limit=0` and `?limit > 500` → `400
  invalid_limit`. 13 unit tests + 7 API-level tests.
- `400 BadRequest` variant added to each `List*Resp` ApiResponse
  enum on the affected resources.

### Changed

- Three taxonomy list endpoints (`listChannels`, `listPriorities`,
  `listAdTypes`) and two TEXT-id list endpoints
  (`listAdLibraryItems`, `listTokens`) remain non-paginated for
  v0; documented in API.md and deferred to Phase 6.5.
- The `x-knievel-paginated*` vendor extensions API.md aspirationally
  promised are deferred to Phase 6.6 — poem-openapi 5 has no
  operation-level extension API; we'll upstream it rather than
  carrying a `cargo xtask openapi` post-processor.

## [0.1.3] — 2026-05-06

### Added

- **Phase 4.10 (partial):** Default `servers:` block stamped into
  `openapi.yaml` (`http://localhost:8080`) so the generated Ruby
  gem doesn't default to `http://localhost`. Both the static spec
  (`lib.rs::openapi_spec_yaml()`) and the live spec
  (`server.rs::routes()`) read from a shared
  `DEFAULT_OPENAPI_SERVER_URL` constant.

## [0.1.2] — 2026-05-06

### Added

- **Phase 4.10 (partial):** Operation tagging via
  `src/api_tags.rs` and `#[OpenApi(tag = "ApiTags::…")]` on the 15
  resource modules. The Ruby gem now exposes 15 focused API classes
  (`Knievel::AdvertisersApi`, `Knievel::CampaignsApi`, …) instead
  of one 3970-line `DefaultApi`. Variant doc-comments flow through
  to tag descriptions in the spec.

## [0.1.1] — 2026-05-06

### Added

- **Phase 4.10 (partial):** Generator CI for the Ruby gem.
  `.github/workflows/release-ruby-gem.yml` triggers on `v*` tags,
  mints an installation token via the `knievel-pipelines` GitHub
  App, regenerates the Faraday-based gem from `openapi.yaml`,
  smoke-tests the build, and commits + tags
  `knievel-ads/knievel-ruby` with the matching version.
  `.github/workflows/publish-rubygems.yml` (in knievel-ruby) takes
  the new tag and `gem push`es to RubyGems via `RUBYGEMS_ORG_API_KEY`.

## [0.1.0] — squat tag

Squatted `knievel` on RubyGems. No public release; first real
release was `0.1.1`.

[Unreleased]: https://github.com/knievel-ads/knievel/compare/v0.1.37...HEAD
[0.1.37]: https://github.com/knievel-ads/knievel/compare/v0.1.36...v0.1.37
[0.1.36]: https://github.com/knievel-ads/knievel/compare/v0.1.35...v0.1.36
[0.1.35]: https://github.com/knievel-ads/knievel/compare/v0.1.34...v0.1.35
[0.1.34]: https://github.com/knievel-ads/knievel/compare/v0.1.33...v0.1.34
[0.1.33]: https://github.com/knievel-ads/knievel/compare/v0.1.32...v0.1.33
[0.1.32]: https://github.com/knievel-ads/knievel/compare/v0.1.31...v0.1.32
[0.1.31]: https://github.com/knievel-ads/knievel/compare/v0.1.30...v0.1.31
[0.1.30]: https://github.com/knievel-ads/knievel/compare/v0.1.29...v0.1.30
[0.1.29]: https://github.com/knievel-ads/knievel/compare/v0.1.28...v0.1.29
[0.1.28]: https://github.com/knievel-ads/knievel/compare/v0.1.27...v0.1.28
[0.1.27]: https://github.com/knievel-ads/knievel/compare/v0.1.26...v0.1.27
[0.1.26]: https://github.com/knievel-ads/knievel/compare/v0.1.25...v0.1.26
[0.1.25]: https://github.com/knievel-ads/knievel/compare/v0.1.24...v0.1.25
[0.1.24]: https://github.com/knievel-ads/knievel/compare/v0.1.23...v0.1.24
[0.1.23]: https://github.com/knievel-ads/knievel/compare/v0.1.22...v0.1.23
[0.1.22]: https://github.com/knievel-ads/knievel/compare/v0.1.21...v0.1.22
[0.1.21]: https://github.com/knievel-ads/knievel/compare/v0.1.20...v0.1.21
[0.1.20]: https://github.com/knievel-ads/knievel/compare/v0.1.19...v0.1.20
[0.1.19]: https://github.com/knievel-ads/knievel/compare/v0.1.18...v0.1.19
[0.1.18]: https://github.com/knievel-ads/knievel/compare/v0.1.17...v0.1.18
[0.1.17]: https://github.com/knievel-ads/knievel/compare/v0.1.16...v0.1.17
[0.1.16]: https://github.com/knievel-ads/knievel/compare/v0.1.15...v0.1.16
[0.1.15]: https://github.com/knievel-ads/knievel/compare/v0.1.14...v0.1.15
[0.1.14]: https://github.com/knievel-ads/knievel/compare/v0.1.13...v0.1.14
[0.1.13]: https://github.com/knievel-ads/knievel/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/knievel-ads/knievel/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/knievel-ads/knievel/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/knievel-ads/knievel/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/knievel-ads/knievel/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/knievel-ads/knievel/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/knievel-ads/knievel/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/knievel-ads/knievel/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/knievel-ads/knievel/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/knievel-ads/knievel/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/knievel-ads/knievel/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/knievel-ads/knievel/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/knievel-ads/knievel/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/knievel-ads/knievel/releases/tag/v0.1.0
