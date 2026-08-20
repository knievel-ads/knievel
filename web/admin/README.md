# Knievel Admin UI

React 18 operator console for the Knievel ad server. The UI is built with
Vite, TanStack Router and Query, Mantine, and the generated OpenAPI client.
See [`../../UI.md`](../../UI.md) for the design and shipped-status notes.

## Local development

Run the API and UI in separate terminals from the repository root:

```bash
cargo run
```

```bash
cd web/admin
pnpm install --frozen-lockfile
pnpm dev
```

Open <http://localhost:5173/admin/>. The Vite development server proxies
same-origin browser requests for `/v1` and exactly `/admin/config.json` to
`http://localhost:8080`. To use a different API origin, set the server-only
variable when starting Vite:

```bash
KNIEVEL_ADMIN_API_ORIGIN=http://127.0.0.1:9080 pnpm dev
```

`KNIEVEL_ADMIN_API_ORIGIN` deliberately has no `VITE_` prefix, so it is not
exposed through `import.meta.env` or included in the browser bundle. It can
also be set in an ignored `.env.local`; do not put bearer tokens, OIDC client
secrets, or other credentials in frontend env files. The proxy is development
only and does not intercept the `/admin/` asset tree. Production API requests
remain same-origin.

## Authentication

The bundle fetches `/admin/config.json` before mounting React:

- When both the OIDC issuer and public client ID are present,
  `react-oidc-context` and `oidc-client-ts` use Authorization Code + PKCE.
- Without complete OIDC metadata, protected routes use the paste-token login.
  The form validates a `kvl_*` opaque token with `/v1/whoami` and keeps it in
  the tab's `sessionStorage`.
- `/admin/login` is a direct paste-token route in either configuration.
  `require_oidc` changes which login choices the UI presents; it does not deny
  direct access to that route. API-side bearer validation and authorization
  remain the security boundary.

No OIDC client secret belongs in this SPA. The runtime config contains only
public issuer, client ID, scope, and UI-policy metadata.

## Shipped routes

All client routes are below the `/admin` router base:

| Group          | Routes and current behavior                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry and auth | `/`, `/login`, and `/oidc/{login,callback,logout}`. `/` resolves the authenticated operator's org with `/v1/whoami`.                                                                                |
| Organization   | `/orgs/{org_id}` lists projects; `/orgs/{org_id}/library` lists org-scoped ad-library items.                                                                                                        |
| Project        | `/orgs/{org_id}/projects/{project_id}` shows project metadata in a placeholder dashboard.                                                                                                           |
| Demand         | Project-scoped lists for `advertisers`, `campaigns`, `flights`, `ads`, and `creatives`; advertiser creation is available at `advertisers/new`, and the creative drawer supports image upload.       |
| Inventory      | Project-scoped `sites` and `zones` lists.                                                                                                                                                           |
| Config         | Project-scoped `templates` plus `taxonomy` tabs for channels, priorities, and ad types.                                                                                                             |
| Reports        | `reports/test` runs real decision and explain requests; `reports/explain` redirects there. Rollups at `reports` and the `reports/events` tail are placeholders until public backing endpoints ship. |

List rows open JSON inspection drawers rather than separate detail routes.
Members, tokens, and other Settings pages are not implemented routes today.

## Generated clients and routes

`src/api/generated.ts` is checked in and generated from the repository's
`openapi.yaml`:

```bash
# Run from the repository root
cargo xtask ui-client
cargo xtask ui-client --check
```

Application calls go through `src/api/client.ts`, which wraps
`openapi-fetch`, attaches the active bearer, captures `X-Request-Id`, and
performs the safe-method OIDC refresh retry. Its base URL stays empty so
production calls are same-origin.

TanStack Router generates `src/routeTree.gen.ts` during `pnpm dev`, build,
typecheck, lint, and test setup. That generated route tree is intentionally
ignored rather than checked in.

## Validation

From the repository root:

```bash
pnpm --dir web/admin typecheck
pnpm --dir web/admin lint
pnpm --dir web/admin format:check
pnpm --dir web/admin test --run
pnpm --dir web/admin build
pnpm --dir web/admin size

pnpm --dir web/admin exec playwright install chromium # once per machine
pnpm --dir web/admin test:e2e
cargo xtask ui-client --check
```

The Playwright suite starts Vite preview on `127.0.0.1:4173`, waits for the
actual `/admin/` mount, and checks both the login smoke path and production
asset URLs. Size Limit and Playwright run nightly rather than on every PR.

## Deployment

`pnpm build` emits `dist/` with `/admin/` asset URLs. The release image copies
that bundle to `/var/lib/knievel/admin`, and Knievel's Poem server mounts it at
`/admin/` when `admin_ui.static_dir` is configured. SPA history fallback keeps
deep links under `/admin/` working, while `/admin/config.json` remains the API's
public runtime-config endpoint. With no static directory, Knievel runs in
headless API mode and does not serve the console.
