# UI.md

Plan for the knievel admin audit UI. Companion to `API.md`,
`AUTH.md`, and `PHASES.md`. Scope is **the admin console** —
operator-facing tooling to audit and manage the state of the ad
server. Self-serve advertiser/publisher consoles are out of scope
for v0 and will be a separate deliverable.

## Goals

- An in-browser console for operators to inspect projects,
  advertisers, campaigns, flights, ads, creatives, sites, zones,
  taxonomy, and (later) reporting and event flow.
- Uses the same OpenAPI surface as every other client. No
  privileged side channels, no admin-only endpoints that bypass
  the public contract. If the UI needs it, the API exposes it.
- Drift-checked against `openapi.yaml` in the same CI run as the
  Rust server. The spec is the contract; the UI's typed client
  is regenerated from it.

## Non-goals (v0)

- A self-serve advertiser/publisher console.
- Mobile-responsive layouts. Desktop-first; tablet works,
  phones are not a target.
- Real-time dashboards beyond what `/events` and reporting
  endpoints expose. No WebSocket push from the API in v0.
- An embedded editor for `creative_template.schema` JSON Schema.
  Raw JSON textarea with validation is acceptable for v0.

## Current implementation status

The admin SPA is shipped in the Knievel image and mounted by the
Poem server at `/admin/`. Vite emits `/admin/` asset URLs and
TanStack Router uses `/admin` as its history base. With no
`admin_ui.static_dir`, the same binary remains a headless API and
does not serve the SPA.

The current bundle includes both supported bearer flows: OIDC
Authorization Code + PKCE when `/admin/config.json` supplies an
issuer and public client ID, and a paste-token form that validates
an opaque `kvl_*` token with `/v1/whoami`. `/admin/login` remains a
direct paste-token route even when `require_oidc` is true; that
flag controls presented UI choices, not route-level or API auth
enforcement. No client secret is used by or belongs in the SPA.

`web/admin/src/api/generated.ts` is checked-in output from
`openapi.yaml`; `cargo xtask ui-client` regenerates it and
`cargo xtask ui-client --check` enforces drift in CI. All API calls
use the generated `openapi-fetch` paths through the same-origin,
auth-aware client.

Shipped route groups cover the org/project browser, org ad
library, project demand lists, inventory, templates, taxonomy,
advertiser creation, creative image upload, and the decision
tester. The project dashboard, rollup report, and events-tail
screens still contain explicit placeholders. The report explainer
route redirects to the tester. Members, tokens, and other Settings
pages are not implemented route modules.

Nightly Playwright runs against the production Vite preview at the
real `/admin/` mount, with API requests stubbed for the deterministic
login smoke. Nightly Size Limit enforces the existing JavaScript and
CSS budgets.

## Information architecture

Knievel's resources don't fit a single drill-down tree. There
are two shapes:

- **Demand tree** — Advertiser → Campaign → Flight → Ad →
  Creative. Natural parent/child chain; the obvious flow when
  auditing "what is project X actually serving."
- **Lateral surfaces** — Sites and Zones (inventory), Creative
  Templates and Taxonomy (config), Ad Library (org-scoped,
  referenced from project Ads), Members and Tokens (org admin).
  Siblings at the project or org level, not children of
  advertisers. Forcing them under the demand tree makes "list
  every template" or "where is this site referenced" needlessly
  indirect.

The UI splits the difference. A left rail at the project
workspace organizes resources into four sections; an org-level
rail handles the cross-project surfaces. **Every resource is
reachable two ways**: a flat searchable list view (cursor
pagination + filter, the audit-first path) and via the demand
tree when that's the natural drill-down. Detail pages
reconstruct the parent chain from the API and surface it as
breadcrumbs.

### Target rail layout

| Section | Scope | Resources | Backing endpoints |
|---|---|---|---|
| Demand | Project | Advertisers, Campaigns, Flights, Ads, Creatives | `/v1/projects/{p}/{resource}` |
| Inventory | Project | Sites, Zones | `/v1/projects/{p}/{resource}` |
| Config | Project | Creative Templates, Taxonomy (Channels, Priorities, Ad Types) | `/v1/projects/{p}/{resource}` |
| Reports | Project | Rollups, Decision Tester, Decision Explainer, Events tail (post-7.8) | `/v1/projects/{p}/decisions`, `/v1/projects/{p}/decisions:explain`, `/events`, reporting endpoints |
| Library | Org | Ad Library items | `/v1/orgs/{o}/ad-library/items` |
| Settings | Org | Projects list, Members, Tokens | `/v1/orgs/{o}/{resource}` |

### Routes

The deployed SPA prefixes every route below with `/admin`. The
current file-route tree is:

```
/                                          → resolve /v1/whoami, then redirect to the org
/login                                     → direct opaque-token login
/oidc/login                                → start Authorization Code + PKCE
/oidc/callback
/oidc/logout

/orgs/{org_id}                             → org details and projects list
/orgs/{org_id}/library                     → org ad-library list + JSON drawer
/orgs/{org_id}/projects/{project_id}       → project metadata placeholder

  # Demand lists
  /advertisers
  /advertisers/new                         → create advertiser
  /campaigns
  /flights
  /ads
  /creatives                               → list + image upload in drawer

  # Inventory lists
  /sites
  /zones

  # Config
  /templates
  /taxonomy                                → channels / priorities / ad-types tabs

  # Reports
  /reports                                 → placeholder rollup controls
  /reports/test                            → live decision + explain requests
  /reports/explain                         → redirect to /reports/test
  /reports/events                          → placeholder events tail
```

Current list views inspect a selected record in a JSON drawer;
separate resource detail URLs are not implemented. There is no
`/orgs/{org_id}/projects` route because the org dashboard owns the
projects list, and there are no Members, Tokens, or other Settings
route modules yet.

The longer-term URL design remains flat per resource within a
project (`{resource}/{id}` rather than a full parent chain) when
standalone detail routes are added.

### Cmd+K spotlight

Mantine's `Spotlight` (or equivalent) gives operators a single
search box to jump by ID, `external_id`, or name across every
resource in the active project. Backed by the same flat list
endpoints (`?q=...`); no separate search index in v0.

### Read-then-edit phasing

Read-only auditor views (Demand + Inventory + Config + Library)
land first as the 7.5 / 7.6 slices. Editing (7.7) layers
PATCH/POST forms onto the same routes — same nav, same detail
shells, same breadcrumbs. Reports (7.8) and OIDC hardening
(7.9) extend the rail without reshaping it.

## Stack

| Concern | Choice | Why |
| --- | --- | --- |
| Framework | **React 18 + TypeScript** | Largest off-the-shelf component & form ecosystem for an audit-style console. |
| Build | **Vite** | Fast dev server, first-class TS, no Next.js server runtime to operate. |
| Routing | **TanStack Router** | Type-safe routes pair well with the typed API client. |
| Data layer | **TanStack Query** + **`openapi-fetch`** (typed via **`openapi-typescript`**) | Codegen consumes `openapi.yaml` directly; no hand-written types. Cache + invalidation handled by Query. |
| Auth | **`react-oidc-context`** (wraps **`oidc-client-ts`**) | Authorization Code + PKCE against Keycloak; silent refresh; no client secret in the SPA. Paste-a-token fallback for dev / no-Keycloak environments. |
| Forms | **react-hook-form** + **zod** | Schemas derived from generated TS types; minimal runtime overhead. |
| Components | **Mantine v7** (tables, modals, forms, dates) | Batteries-included audit UI primitives. shadcn/ui is the fallback if Mantine bloat becomes a problem. |
| Tables | **TanStack Table** under Mantine wrappers | Server-side pagination/sort/filter against our cursor envelope. |
| Lint/format | **eslint** (typescript-eslint, react-hooks) + **prettier** | Match Rust gates' "fail CI on warnings" stance. |
| Tests | **vitest** + **@testing-library/react**; **Playwright** for end-to-end | Vitest mirrors `cargo test` ergonomics. Nightly Playwright runs the production preview with deterministic API stubs. |
| Package manager | **pnpm** | Lockfile determinism; faster than npm in CI. |

Hard constraints:

- **No Next.js, no SSR, no RSC.** This is a single-page admin
  app talking to a Rust API. SSR adds an extra runtime to
  operate and buys us nothing here.
- **No cookies for app auth.** Bearer tokens in
  `Authorization` headers; storage discussed under "Auth"
  below. Keeps CORS simple (`allow_credentials: false`).
- **No third-party telemetry SDKs in v0.** Operator console;
  we already get server-side request IDs via
  `observability.rs`.

## Repo layout

The UI lives under `web/admin/` and moves with the OpenAPI
surface. The current layout is:

```
web/admin/
  package.json
  pnpm-lock.yaml
  playwright.config.ts
  vite.config.ts
  src/
    api/
      generated.ts        # checked-in openapi-typescript output
      client.ts           # auth-aware openapi-fetch client
    auth/                  # runtime config, OIDC, paste token, route guard
    components/            # tables, drawers, workspace shell, upload/mint UI
    routes/
      index.tsx
      login.tsx
      oidc.{login,callback,logout}.tsx
      orgs.$org_id.tsx
      orgs.$org_id.library.tsx
      orgs.$org_id.projects.$project_id.tsx
      orgs.$org_id.projects.$project_id.{resource}.tsx
      orgs.$org_id.projects.$project_id.reports*.tsx
    main.tsx
  tests/e2e/smoke.spec.ts
  README.md
```

TanStack Router generates the ignored `src/routeTree.gen.ts` from
those flat route modules. The generated API client is checked in;
the generated router tree is not.

When/if the UI graduates to its own deploy cadence (post-Phase
3, once the spec stabilizes and the deploy story diverges),
extract to a sibling repo. Until then, in-tree wins for the same
reasons we keep `testlib/` and `xtask/` in-tree: one PR, one CI
run, no version-skew window.

## OpenAPI codegen

The shipped xtask commands are:

```bash
cargo xtask ui-client                # regenerate web/admin/src/api/generated.ts
cargo xtask ui-client --check        # CI gate: fail if generated.ts is stale
```

`xtask/src/ui_client.rs` runs the pinned `openapi-typescript`
from `web/admin/` against `../../openapi.yaml`. Check mode writes
a fresh client under `target/` and compares it byte-for-byte with
the committed file.

`ui-client --check` is wired into `.github/workflows/ci.yml` as a
peer of `openapi --check`. Both gates use the repository's shared
Node setup action.

The generated file is **checked in** (not gitignored) for the
same reason `openapi.yaml` is: it's part of the contract surface
and reviewers should see when it changes.

## Auth

**Primary flow: OIDC Authorization Code + PKCE against Keycloak.**
Humans sign in to Keycloak; the SPA receives a short-lived
access token (a JWT carrying the `knievel` claim); every API
call presents it as `Authorization: Bearer <jwt>`. The API
already validates these JWTs unchanged via the existing JWKS
machinery — no admin-only endpoints, no custom session backend,
no cookies. See `AUTH.md` "Keycloak Setup — Human Admin UI
(PKCE)" for the canonical contract; this section covers the SPA
side.

### Library + flow

- **`react-oidc-context`** (wraps `oidc-client-ts`) drives the
  dance. `<AuthProvider>` at the route root; `useAuth()` hook
  exposes `{ user, signinRedirect, signoutRedirect, ... }`.
- **Public client + PKCE.** No client secret in the bundle; the
  `code_verifier` is generated per-login and held in
  `sessionStorage` only until the callback completes.
- **Token storage.** Access token + refresh token live in
  memory inside `UserManager`, with `sessionStorage`
  persistence so a tab reload doesn't force re-auth. Closing
  the tab clears them; explicit logout calls
  `signoutRedirect()` which hits Keycloak's
  `end_session_endpoint`.
- **Silent refresh.** `oidc-client-ts` auto-refreshes the
  access token via the refresh token before expiry; on
  refresh failure (revoked session, Keycloak unreachable) the
  fetch wrapper redirects to `/oidc/login`.
- **`RequireAuth` route guard** wraps every protected route;
  unauthenticated → `signinRedirect()`. The fetch wrapper
  retries once on `401` after a silent refresh attempt.
- **Role-claim-driven UI gating.** The `knievel.role` from the
  JWT is read in the SPA to hide admin-only surfaces from
  `editor` / `reader` users. **Not a security boundary** —
  knievel still enforces every authz check server-side; this
  is purely cosmetic.

### Routes

Under the deployed `/admin` base:

- `/oidc/login` — initiates `signinRedirect()` and returns null.
- `/oidc/callback` — completes `signinRedirectCallback()`,
  redirects to the deep link the user originally requested.
- `/oidc/logout` — calls `signoutRedirect()`.
- `/login` — validates and stores a pasted opaque token.

### Runtime config (one bundle, multiple envs)

The SPA can't bake the Keycloak issuer / client-id into the
build because we want a single artifact to deploy across
staging / prod / dev. On boot the SPA fetches
`GET /admin/config.json`, served by the API from the same
origin:

```json
{
  "oidc": {
    "issuer":    "https://keycloak.scientist.com/realms/scientist",
    "client_id": "knievel-admin-ui-prod",
    "scopes":    ["openid", "profile", "knievel"],
    "require_oidc": true
  }
}
```

The API constructs this public payload from the shipped
`admin_ui:` config section. It registers the endpoint before the
`StaticFilesEndpoint` mount so a bundle file cannot shadow it;
see `AUTH.md` "Knievel-side configuration." OIDC is enabled only
when both `oidc.issuer` and `oidc.client_id` are non-empty.
Otherwise protected routes fall through to the paste-token form.

### Paste-a-token fallback

Kept as a deliberate first-class fallback, not a dev-only
hack:

- **Dev / `docker compose up`** — the seed sidecar mints an Org
  Editor opaque token (`AUTH.md` "Local Development"), no
  Keycloak in the picture. Paste-token login uses it
  unchanged.
- **Bootstrap** — bring up a knievel cluster before Keycloak
  is provisioned.
- **Keycloak outage / DR** — operators with a break-glass
  opaque token in their secret store stay unblocked.
- **CI smoke tests** — deterministic credential, no IdP
  dependency.

The form accepts a `kvl_*` token, validates it against
`GET /v1/whoami`, and stores it in `sessionStorage` exactly as
the JWT path does. From the API's perspective the request is
identical to any other Bearer call; the UI just constructs the
header from a different source.

With complete OIDC metadata, the protected-route guard prefers
the OIDC redirect. The direct `/admin/login` paste-token route is
still reachable, including when `admin_ui.oidc.require_oidc` is
true. That flag changes the login choices presented by the SPA;
it is not route-level enforcement and must not be treated as the
security boundary. Knievel's API authentication and authorization
remain authoritative.

### What's intentionally not here

- **No BFF / no admin-session endpoint.** The original sketch
  had `POST /v1/admin/sessions` backed by argon2id user
  credentials; replaced by Keycloak-as-the-IdP, which deletes
  custom auth code we'd have to maintain and gets MFA / SSO /
  password rotation / session policy free.
- **No cookies for app auth.** Keeps CORS at
  `allow_credentials: false` and removes the cookie-CSRF
  surface. Refresh tokens go through `oidc-client-ts` directly
  to Keycloak's token endpoint, never via knievel.
- **No tokens baked into the bundle.** Build artifacts contain
  no secrets. The runtime config only exposes public OIDC
  metadata (issuer URL + public client ID).

### Token-mint "show-once" UX

Mint endpoints (`POST /v1/orgs/{org_id}/tokens` and HMAC
secret rotation via `PATCH .../projects/{project_id}`) return
the plaintext credential **exactly once**; knievel stores the
argon2id hash and the value is unrecoverable thereafter
(`AUTH.md` "Opaque Tokens"). Getting this UX wrong silently
locks operators out, so the workflow is pinned:

1. Mint form posts and the modal shows the plaintext value
   in a monospace block, with a one-click copy-to-clipboard
   button and a prominent "**Save this now — it will not be
   shown again**" callout.
2. Dismissal is gated behind an explicit checkbox: "I've
   stored this value somewhere safe." No "X" close button,
   no Esc-to-dismiss, no clickaway. The Done button is
   disabled until the box is ticked.
3. After dismissal the value is wiped from React state and
   from the TanStack Query cache; revisiting the token in
   the list view shows only the metadata (id, name, scope,
   role, last-used) — never the secret.
4. The list view's row never shows the plaintext, only the
   `kvl_<env>_<scope>_` prefix as a visual marker so
   operators can disambiguate without revealing anything.

Same pattern applies to:

- **HMAC secret rotation.** `PATCH` returns the new secret
  exactly once; we never round-trip it again. Rotation
  overlap (`hmac.rotation_overlap_hours`) keeps already-
  signed URLs valid; the UI surfaces the overlap window so
  the operator knows when old secrets stop being honored.
- **Future mint endpoints** (project-scoped tokens,
  member-invite codes, etc.). Whenever a server-only secret
  is returned in a response body, the same modal pattern is
  reused.

A dedicated `<MintRevealModal>` component owns the pattern;
all mint flows route through it so the policy can't drift.

## Error handling

Every API call goes through one fetch wrapper, and every
non-2xx response goes through one error pipeline. The shape:

### Request ID surfacing

Knievel stamps every response with `X-Request-Id`
(`src/observability.rs`). The fetch wrapper reads it on
every response (success or failure) and attaches it to the
TanStack Query result. Error toasts and error panels render
the ID prominently with a copy button:

> Failed to update advertiser. Please try again.
> Request ID: `01J9X...` _(copy)_

Operators can paste that ID into a support channel and
on-call can correlate to server logs in one query. **This
is the single most important debugging UX in the console**;
without it, support tickets devolve into "what time was
this, and which org" guessing.

### State machine per failure mode

| Status | UX |
|---|---|
| `400 validation` | Field-level errors mapped from the API's `errors[]` envelope into react-hook-form via the `setError` API. Toast for non-field errors. |
| `401 invalid_token` | Fetch wrapper attempts one silent refresh via `oidc-client-ts`; on failure, redirect to `/oidc/login` preserving the original deep link via `?return_to=`. |
| `403 forbidden` | Inline panel: "You don't have access to this resource. Your role is `editor`; this requires `org-admin`." Reads the `knievel.role` claim from the active session for the role hint. |
| `404 not_found` | Inline empty-state on detail pages; toast on list-page filters that resolve to nothing. |
| `409 conflict` | Field-level if attributable (`external_id_conflict` → highlight the externalId field); toast otherwise. |
| `422 unprocessable` | Same shape as 400; field-level mapping. |
| `429 rate_limited` | Toast with the `Retry-After` value, exponential backoff on the next attempt. |
| `5xx` | Generic toast: "Knievel returned an error. Request ID: ...". Detailed error drawer from a "Show details" link revealing status, error code, message, and request ID. |
| Network error | Toast: "Couldn't reach knievel — check your connection." Retry button. No Request ID since no response was received. |

### What gets logged client-side

Errors get console-logged with the request ID, the URL, the
HTTP status, and the API's error envelope. Nothing more.
Sentry-on-the-client is in Open Questions below — error
tracking is arguably different from telemetry, and a real
operator complaint should motivate it before we add another
SDK.

## CORS

Knievel supports CORS for an intentional split-origin deployment
(e.g. `admin.example.com` → `api.example.com`). The normal bundled
production path is same-origin, and the normal Vite workflow also
keeps browser requests same-origin through its development proxy.
CORS is therefore available but is not required for either default
path.

### API-side configuration

`src/config.rs::ApiConfig` includes:

```rust,ignore
/// Origins permitted to make cross-origin requests. Empty
/// (default) disables the CORS layer entirely — same-origin
/// only. Each entry is a literal origin like
/// `https://admin.example.com`; wildcards are not supported.
#[serde(default)]
pub allowed_origins: Vec<String>,
```

In `src/server.rs`, when `cfg.api.allowed_origins` is non-empty,
wrap the route with `poem::middleware::Cors`:

- `allow_origins`: from config.
- `allow_methods`: `GET, POST, PATCH, DELETE, OPTIONS`.
- `allow_headers`: `Authorization`, `Content-Type`,
  `Idempotency-Key`, `If-Match`, `X-Request-Id`.
- `expose_headers`: `ETag`, `Location`, `X-Request-Id`,
  `X-Idempotency-Replayed`. (Anything custom that the UI needs
  to read off responses.)
- `allow_credentials`: **false**. We use bearer tokens, not
  cookies; staying credentials-less keeps the wildcard escape
  hatch open and removes a cookie-CSRF surface.
- `max_age`: 600 seconds. Cuts preflight chatter for the SPA's
  navigation patterns.

### Dev defaults

Vite proxies `/v1` and exactly `/admin/config.json` to
`KNIEVEL_ADMIN_API_ORIGIN`, which defaults to
`http://localhost:8080`. It deliberately does not proxy `/admin`
or any other asset path. The variable is loaded only by the Vite
server (there is no `VITE_` prefix), so it is not exposed to the
browser or built bundle.

Set a different local API without changing application code:

```bash
cd web/admin
KNIEVEL_ADMIN_API_ORIGIN=http://127.0.0.1:9080 pnpm dev
```

Because the browser calls the Vite origin, the API can leave
`allowed_origins` empty in this default workflow. Configure
`allowed_origins` only when deliberately testing direct
cross-origin requests.

### Prod guidance

- Same-origin deploy (UI served behind the same hostname as
  the API via reverse proxy or a poem static-files route):
  leave `allowed_origins` empty. Simplest, no CORS layer
  installed, no preflight overhead.
- Split-origin deploy: list each admin origin explicitly. Do
  not use `*`; bearer tokens shouldn't fly to arbitrary
  origins even with `allow_credentials: false`.

### Test coverage

`tests/api_cors.rs` asserts:

1. Empty `allowed_origins` → no `Access-Control-Allow-Origin`
   header on responses; OPTIONS returns 404 (poem's default).
2. Configured origin echoes back on `Access-Control-Allow-Origin`
   for matching `Origin:` requests; non-matching origins get no
   header.
3. Preflight (`OPTIONS` with `Access-Control-Request-Method`)
   returns 204 with the expected `Allow-Methods`,
   `Allow-Headers`, `Max-Age`.
4. `Authorization` is in the allowed-headers reflection.

This integration slice runs with the repository's API tests.

## Dev workflow

Install once, then run the API and Vite in separate terminals:

```bash
# Repository root
pnpm --dir web/admin install --frozen-lockfile
cargo run                                      # API on :8080
```

```bash
# Second terminal, repository root
pnpm --dir web/admin dev                       # UI at /admin/ on :5173
```

Open <http://localhost:5173/admin/>. Same-origin requests to
`/v1` and `/admin/config.json` reach the API through Vite's
development-only proxy. To target another API:

```bash
KNIEVEL_ADMIN_API_ORIGIN=http://127.0.0.1:9080 pnpm --dir web/admin dev
```

After changing `openapi.yaml`, regenerate and check the committed
client from the repository root:

```bash
cargo xtask ui-client
cargo xtask ui-client --check
```

Run the frontend PR gates with:

```bash
pnpm --dir web/admin typecheck
pnpm --dir web/admin lint
pnpm --dir web/admin format:check
pnpm --dir web/admin test --run
pnpm --dir web/admin build
```

## CI integration

The current `.github/workflows/ci.yml` jobs run on every PR:

- `ui-typecheck` — `pnpm typecheck`
- `ui-lint` — `pnpm lint` plus `pnpm format:check`
- `ui-test` — `pnpm test --run`
- `ui-client-drift` — `cargo xtask ui-client --check`
- `ui-build` — `pnpm build`

`nightly.yml` separately runs `pnpm size` and the Playwright
smoke against the production preview. The smoke stubs runtime
config and `/v1/whoami`; it does not require Postgres or an OIDC
provider. The shared `.github/actions/node-setup/` action pins
Node and pnpm and installs with the frozen lockfile.

## Test coverage

Layered, with each layer tightening one concern. The
foundational helpers (auth/session, runtime config, fetch
wrapper, error-notification helper) get **vitest** unit
coverage; security-critical surfaces (mint flows) and
representative views get **axe** sweeps; the integrated
boot path gets **Playwright** smoke tests in nightly CI.

| Surface | Layer | Why |
|---|---|---|
| `auth/session` (paste-token store/clear, bearer accessor) | vitest | Wire-shape contract for the fetch wrapper. Lock the precedence order. |
| `auth/runtimeConfig` (OIDC metadata fetch, defaults, cache) | vitest | Boot can't pivot on this getting wrong; the safe-defaults branches must work. |
| `api/client` fetch wrapper (Bearer attach, X-Request-Id capture, 401 silent-refresh + retry on safe methods) | vitest | The most-touched code path; the silent-refresh retry is the kind of logic that breaks invisibly. |
| `api/errors.notifyApiError` (per-status title/body/color) | vitest | Pinned strings; future error-UX changes go through here. |
| `auth/RequireAuth` (paste-token path) | vitest | Guard logic isn't a security boundary, but a regression breaks every protected route. OIDC variant deferred — needs full `react-oidc-context` mocking. |
| `MintRevealModal` (Done-disabled-until-ticked, Esc no-op) | vitest | Security-critical mint UX — silent regressions here lock operators out. |
| `MintRevealModal` a11y | vitest + axe-core | Security-critical surface; WCAG 2 A/AA baseline. |
| `PasteTokenLogin` a11y | vitest + axe-core | Auth entry point. |
| Per-resource list views, detail drawer, edit forms, decision tester | typecheck + Playwright | Ten near-identical list views; per-view component tests would be repetitive. The typed `apiClient.GET(...)` calls catch wire-shape regressions; Playwright covers the integration. |
| Boot → paste-token login → invalid-token error | Playwright (nightly) | The walking-skeleton smoke. Stubs `/admin/config.json` + `/v1/whoami` so no backend needed. |
| Bundle-size budget (180 KB main / 35 KB Mantine CSS, gzip) | size-limit (nightly) | Advisory; guards against accidental dep bloat. |

What's intentionally NOT covered:

- **OIDC redirect flow.** Requires a running Keycloak.
  Manual verification per `AUTH.md` § 7 is the v0 path;
  CI-side OIDC-fixture e2e is a follow-up if/when a
  Keycloak-in-CI rig becomes worth standing up.
- **Per-resource list views as separate tests.** The 10
  list routes share `<DataTable>` + `<JsonDrawer>` + the
  same `apiClient.GET(...)` pattern; testing each
  individually is duplicate work. Add per-resource
  Playwright specs only when a real bug surfaces in a
  specific view.
- **Color-contrast axe rule.** Skipped in vitest because
  happy-dom doesn't compute paint; relies on Mantine's
  theme defaults plus manual verification.
- **WorkspaceShell role-gating in unit tests.** Covered by
  the typecheck + Playwright (which sees the rail with
  whatever role the stubbed `/v1/whoami` returns).

## Deployment

The admin UI ships in the **same `ghcr.io/<owner>/knievel`
image as the API**. No separate static host, no second deploy
pipeline, no version-skew window between the two halves. UI
version === API version === git SHA, all the way through to
`GET /version`.

### Build split: bundle in CI, image just copies the artifact

The Node build runs in **GitHub Actions**, not as a Docker
stage. CI's pnpm cache (via `actions/setup-node` with
`cache: pnpm`) is dramatically faster and simpler than
shoehorning a Node build into a Docker layer; we keep the
Dockerfile focused on packaging.

The runtime `Dockerfile` copies the built bundle and configures
the shipped mount:

```dockerfile
COPY web/admin/dist /var/lib/knievel/admin
ENV KNIEVEL_ADMIN_UI__STATIC_DIR=/var/lib/knievel/admin
```

The Rust multi-stage build is unchanged. The Dockerfile
expects `web/admin/dist/` to be present in the build context;
CI guarantees it, locally a wrapper handles it (next
section).

### CI workflow shape

The shared Node setup action installs the package with the frozen
lockfile, then `ci.yml` and `release.yml` run
`pnpm --dir web/admin build`. Release stages `dist/` beside the
native Knievel binaries in a small `docker-context/` before
calling `docker/build-push-action`.

Wins from this split:

- **pnpm store cached automatically** by `setup-node`. No
  Docker buildx cache mount, no registry round-trips for
  cache layers — just a tarball restore that runs in
  seconds.
- **Bundle artifact is reusable**: same `dist/` flows into
  the image AND into bundle-size budget checks AND
  (eventually) into Sentry sourcemap upload, all from one
  build.
- **Dockerfile stays small**: one extra COPY, no second
  language toolchain in the build image.
- **No Node in the runtime image**, no Node in the
  build-image cache. Less to invalidate.

### Local dev wrapper

Building the image locally needs the bundle present first. The
shipped wrapper is:

```bash
cargo xtask build-image                 # pnpm build + docker build
cargo xtask build-image --skip-ui       # headless API only
```

`xtask/src/build_image.rs` shells out to `pnpm --dir
web/admin install --frozen-lockfile && pnpm --dir web/admin
build`, then `docker build -t knievel:dev .`. The
`--skip-ui` flag substitutes an empty `web/admin/dist/`
(just enough to make COPY succeed) for the headless case.

### Static-file serving

Poem has first-class static-file support — no extra crate
needed beyond enabling the `static-files` feature:

```toml
poem = { workspace = true, features = ["static-files"] }
```

The mount in `src/server.rs`:

```rust,ignore
use poem::endpoint::StaticFilesEndpoint;

if let Some(dir) = &cfg.admin_ui.static_dir {
    route = route.nest(
        "/admin",
        StaticFilesEndpoint::new(dir)
            .index_file("index.html")
            .fallback_to_index(),       // SPA history-routing
    );
}
// /admin/config.json is registered BEFORE this nest so
// the static fallback doesn't shadow it.
```

When `cfg.admin_ui.static_dir` is unset, no mount is
installed and `/admin/*` returns 404 — same image runs as a
headless API deployment. Same-origin means the CORS layer
stays disabled in bundled mode.

`GET /admin/config.json` is registered **before** the static
nest so the SPA's runtime config (`AUTH.md` "Knievel-side
configuration") doesn't get shadowed by a `config.json`
file inside the bundle.

### ghcr.io publishing

The existing `release.yml` workflow publishes
`ghcr.io/<owner>/knievel` via `docker/build-push-action` on
`v*` tags. Its pnpm build runs before the Docker step in the
same lane. UI version === API version === git SHA, all the way
through to `GET /version`.

## Phasing (historical plan)

The sequence below records the original implementation decomposition.
The current-status section and checked-in route tree above are
authoritative for what is shipped:

- **Phase 7.1** — Repo skeleton: `web/admin/` scaffold, vite +
  TS + Mantine, ESLint/Prettier, vitest harness, README. No
  routes yet beyond a placeholder.
- **Phase 7.2** — CORS rail: `ApiConfig.allowed_origins`,
  poem `Cors` middleware, `tests/api_cors.rs`, config example
  update.
- **Phase 7.3** — Codegen rail: `xtask ui-client [--check]`,
  `node-setup` composite action, CI wiring, generated
  `src/api/generated.ts` checked in.
- **Phase 7.4** — Auth (OIDC PKCE primary, paste-token
  fallback): `react-oidc-context` wired against Keycloak,
  `/oidc/login` + `/oidc/callback` + `/oidc/logout` routes,
  `RequireAuth`, 401-aware fetch wrapper with silent refresh,
  paste-a-token form behind a runtime-config flag.
  Adds the `admin_ui:` config block on the API side and
  `GET /admin/config.json` to surface OIDC metadata to the
  bundle. Smallest "who am I" endpoint added on the API side
  if it doesn't exist yet (validates both JWT and opaque
  Bearer paths identically). Refs: `AUTH.md` "Keycloak Setup
  — Human Admin UI (PKCE)."
- **Phase 7.5** — Org/project browser: list + detail for
  `/v1/orgs` and `/v1/projects/:project_id`. First end-to-end
  slice; proves the typed client + Query + Router stack.
- **Phase 7.6** — Resource audit views: read-only tables for
  advertisers, campaigns, flights, ads, creatives, sites,
  zones, taxonomy. Cursor pagination + filter UI per the API
  envelope.
- **Phase 7.7** — Editing: PATCH/POST forms with
  react-hook-form + zod, idempotency-key handling, optimistic
  invalidation. Feature-flag rollout per resource. Includes
  the **`<MintRevealModal>`** for token mint + HMAC rotation
  (per "Token-mint show-once UX") and the **creative image
  upload** (`POST /creatives/{id}/image`) drag-and-drop with
  `images.upload.max_bytes` + `allowed_mime_types`
  validation.
- **Phase 7.8** — Reporting + event-flow inspector: charts on
  rollups, a tail view over `/events` (poll-based for v0).
- **Phase 7.9** — OIDC hardening: end-session integration with
  Keycloak's `end_session_endpoint`, session-timeout UX
  (idle-warning modal + grace refresh), role-claim-driven UI
  gating (hide admin-only surfaces from `editor` / `reader`
  claim values; not a security boundary — knievel still
  enforces server-side), and a documented Keycloak admin-UI
  client setup verified end-to-end against a real realm.
  Refs: `AUTH.md` "Keycloak Setup — Human Admin UI (PKCE)."
- **Phase 7.10** — Playwright e2e in `nightly.yml`, bundle-size
  budgets, accessibility sweep.
- **Phase 7.11** — Single-image ghcr publish: Node build runs
  in GitHub Actions (pnpm cache via `actions/setup-node`),
  Dockerfile gains one `COPY web/admin/dist
  /var/lib/knievel/admin` line, `admin_ui:` config block +
  poem `StaticFilesEndpoint` mount at `/admin/` (with
  `static-files` feature enabled), `GET /admin/config.json`
  for runtime config, `cargo xtask build-image [--skip-ui]`
  wrapper for local devs. Same `ghcr.io/<owner>/knievel`
  image, no new release lane.
- **Phase 7.13** — Decision tester / debugging surface: a
  form that builds a real `POST /v1/projects/{p}/decisions`
  request (zone, channel, ad-type, targeting JSON, force.*
  overrides), fires it, and renders the served ad alongside
  the `:explain` response showing per-flight-and-ad reasons.
  The single most valuable surface for "why isn't my
  campaign serving?" debugging. Lives at `/reports/test`;
  reuses 7.7's form components and the auth fetch wrapper.
  (7.12 was vacated when fly.io was dropped; numbering
  jumps to 13.)

Numbering is provisional — the actual phase number lands when
this gets merged into `PHASES.md`. The point is the dependency
order: skeleton → CORS → codegen → auth → views → editing →
reporting → OIDC hardening → polish → single-image publish
→ decision tester.

## Open questions

- **`creative_template.schema` editor.** Raw textarea + JSON
  parse for v0. Monaco-based editor with JSON-Schema-aware
  IntelliSense is appealing but adds ~2 MB to the bundle —
  defer until a real operator complaint motivates it.
- **i18n.** Not in v0. English-only. Plan: when needed, lift
  strings into a `messages/` directory and pick a library
  then; don't pre-build a translation pipeline.
- **Dark mode.** Mantine ships it free; enable from day one.
  No additional spec needed.
- **Audit-log viewer.** `audit_log` records privileged ops
  (force.* decisions, member changes, token mints). No
  list endpoint exists today; once one lands, surface it
  under Settings as a paginated read-only feed. Auditors
  will care about this; operators will not. Track when the
  endpoint is added.
- **CSV / JSON export from list views.** Cursor pagination
  makes paginate-and-stream straightforward. Defer until
  someone asks; the current "open dev tools and grab the
  JSON" path covers ad-hoc cases.
- **Empty-state / first-run UX.** Fresh projects with no
  advertisers, no flights, no creatives need landing copy
  that points at the next step. Mantine's `<Empty>` patterns
  cover the visual shell; the copy is product work that
  belongs alongside 7.5/7.6 once views are real.
- **Large-table virtualization.** Ad Library and Sites can
  reach thousands of rows. TanStack Table integrates with
  `@tanstack/react-virtual`; defer until a real list
  exceeds ~500 rows in practice.
- **Client-side error tracking (Sentry).** UI.md "Stack"
  excludes telemetry SDKs; error tracking is arguably
  different. Defer until a real operator-side bug motivates
  it. Server-side Sentry already covers the API path.
- **Saved filter views.** Power users want to bookmark
  filter combos. URL query params already support deep
  links per route; saved views as a separate concept can
  wait.
- **Version footer.** A small footer or About modal showing
  `GET /version` (knievel SHA, build timestamp, auth
  policy). Cheap to add; do it when the first support
  ticket starts with "what version are you on?"

## Cross-references

- `API.md` — endpoint shapes the UI consumes.
- `AUTH.md` — bearer token semantics; the UI honors them
  unchanged.
- `REQUIREMENTS.md` § 6 — OpenAPI 3.0 reality (gotcha #4 in
  `CLAUDE.md`); `openapi-typescript` handles 3.0 fine.
- `TESTING.md` § 12.4 — CI DAG; new UI jobs slot in as
  conditional peers.
- `PHASES.md` — phase numbering and the canonical task list.
