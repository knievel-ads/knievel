# Knievel HTTP API

The Rust `#[oai]` declarations generate [`openapi.yaml`](openapi.yaml), which is
the machine-readable contract. This document supplies the human operation map
and current implementation caveats. JSON properties and query parameters are
`snake_case`.

## Canonical generated operations

The table between the markers is the **only** canonical operation table.
`cargo xtask check-api-doc` compares normalized `(HTTP method, path)` pairs in
both directions against generated OpenAPI. Parameter spelling inside braces is
normalized; methods are not.

<!-- BEGIN CANONICAL OPENAPI OPERATIONS -->
| Verb | Path | Purpose |
|---|---|---|
| `GET` | `/healthz` | Process liveness. |
| `GET` | `/readyz` | Database reachability readiness. |
| `GET` | `/version` | Build metadata and effective auth policy. |
| `GET` | `/v1/whoami` | Validate a bearer and return its principal. |
| `POST` | `/v1/orgs/{org_id}/projects` | Create a project and default taxonomy. |
| `GET` | `/v1/orgs/{org_id}/projects` | List projects in the organization. |
| `GET` | `/v1/orgs/{org_id}/projects/{project_id}` | Read one project. |
| `GET` | `/v1/orgs/{org_id}` | Read organization metadata. |
| `POST` | `/v1/orgs/{org_id}/tokens` | Mint an opaque token. |
| `GET` | `/v1/orgs/{org_id}/tokens` | List token metadata. |
| `DELETE` | `/v1/orgs/{org_id}/tokens/{token_id}` | Revoke a live token. |
| `POST` | `/v1/orgs/{org_id}/ad-library/items` | Create an organization ad-library item. |
| `GET` | `/v1/orgs/{org_id}/ad-library/items` | List organization ad-library items. |
| `GET` | `/v1/orgs/{org_id}/ad-library/items/{item_id}` | Read one ad-library item. |
| `PATCH` | `/v1/orgs/{org_id}/ad-library/items/{item_id}` | Update one ad-library item. |
| `POST` | `/v1/projects/{project_id}/advertisers` | Create an advertiser. |
| `GET` | `/v1/projects/{project_id}/advertisers` | List advertisers. |
| `GET` | `/v1/projects/{project_id}/advertisers/{id}` | Read an advertiser. |
| `PATCH` | `/v1/projects/{project_id}/advertisers/{id}` | Update an advertiser. |
| `POST` | `/v1/projects/{project_id}/advertisers:batchUpsert` | Batch upsert advertisers by external ID. |
| `POST` | `/v1/projects/{project_id}/campaigns` | Create a campaign. |
| `GET` | `/v1/projects/{project_id}/campaigns` | List campaigns. |
| `GET` | `/v1/projects/{project_id}/campaigns/{id}` | Read a campaign. |
| `PATCH` | `/v1/projects/{project_id}/campaigns/{id}` | Update a campaign. |
| `POST` | `/v1/projects/{project_id}/campaigns:batchUpsert` | Batch upsert campaigns by external ID. |
| `POST` | `/v1/projects/{project_id}/flights` | Create a flight. |
| `GET` | `/v1/projects/{project_id}/flights` | List flights. |
| `GET` | `/v1/projects/{project_id}/flights/{id}` | Read a flight. |
| `PATCH` | `/v1/projects/{project_id}/flights/{id}` | Update a flight. |
| `POST` | `/v1/projects/{project_id}/flights:batchUpsert` | Batch upsert flights by external ID. |
| `POST` | `/v1/projects/{project_id}/creatives` | Create a creative. |
| `GET` | `/v1/projects/{project_id}/creatives` | List creatives. |
| `POST` | `/v1/projects/{project_id}/creatives/{id}/image` | Validate and upload creative image bytes. |
| `GET` | `/v1/projects/{project_id}/creatives/{id}` | Read a creative. |
| `POST` | `/v1/projects/{project_id}/creative-templates` | Create a creative template. |
| `GET` | `/v1/projects/{project_id}/creative-templates` | List creative templates. |
| `GET` | `/v1/projects/{project_id}/creative-templates/{id}` | Read a creative template. |
| `PATCH` | `/v1/projects/{project_id}/creative-templates/{id}` | Update and version a creative template. |
| `POST` | `/v1/projects/{project_id}/ads` | Create a flight-to-creative ad binding. |
| `GET` | `/v1/projects/{project_id}/ads` | List ads. |
| `GET` | `/v1/projects/{project_id}/ads/{id}` | Read an ad. |
| `PATCH` | `/v1/projects/{project_id}/ads/{id}` | Update an ad. |
| `POST` | `/v1/projects/{project_id}/ads:batchUpsert` | Batch upsert ads by external ID. |
| `POST` | `/v1/projects/{project_id}/sites` | Create a site. |
| `GET` | `/v1/projects/{project_id}/sites` | List sites. |
| `POST` | `/v1/projects/{project_id}/sites/upsert-by-url` | Find or create a site by canonical URL. |
| `GET` | `/v1/projects/{project_id}/sites/{id}` | Read a site. |
| `PATCH` | `/v1/projects/{project_id}/sites/{id}` | Update a site. |
| `POST` | `/v1/projects/{project_id}/sites:batchUpsert` | Batch upsert sites by external ID. |
| `POST` | `/v1/projects/{project_id}/zones` | Create a zone. |
| `GET` | `/v1/projects/{project_id}/zones` | List zones. |
| `GET` | `/v1/projects/{project_id}/zones/{id}` | Read a zone. |
| `PATCH` | `/v1/projects/{project_id}/zones/{id}` | Update a zone. |
| `POST` | `/v1/projects/{project_id}/zones:batchUpsert` | Batch upsert zones by external ID. |
| `GET` | `/v1/projects/{project_id}/channels` | List project channels. |
| `GET` | `/v1/projects/{project_id}/channels/{id}` | Read a channel. |
| `GET` | `/v1/projects/{project_id}/priorities` | List priorities in tier order. |
| `GET` | `/v1/projects/{project_id}/priorities/{id}` | Read a priority. |
| `GET` | `/v1/projects/{project_id}/ad-types` | List project ad types. |
| `GET` | `/v1/projects/{project_id}/ad-types/{id}` | Read an ad type. |
| `POST` | `/v1/projects/{project_id}/decisions` | Select ads and enqueue decision events. |
| `POST` | `/v1/projects/{project_id}/decisions:explain` | Explain selection without enqueueing events. |
<!-- END CANONICAL OPENAPI OPERATIONS -->

## Direct routes outside OpenAPI

These routes are mounted directly in [`src/server.rs`](src/server.rs) and are
not part of the 62-operation generated table:

| Verb | Path | Current behavior |
|---|---|---|
| `GET` | `/openapi.json` | Live JSON OpenAPI document from the same service tuple. |
| `GET` | `/admin/config.json` | Public OIDC client metadata for the SPA; no secret. |
| `GET` | `/e/i/{signed}` | HMAC-authorized impression response and best-effort event enqueue. |
| `GET` | `/e/c/{signed}` | HMAC-authorized click event and redirect. |
| `GET` | `/admin/*` | Static SPA and history fallback when a static directory is configured. |

There is no `/metrics` route.

## Common wire behavior

### Authentication

Generated management and decision operations require
`Authorization: Bearer <credential>` except the three generated system routes
(`/healthz`, `/readyz`, `/version`). `/v1/whoami` is the smallest authenticated
handshake.

Opaque credentials are DB-backed and always available with a DB. JWT-shaped
credentials are accepted when at least one `auth.jwt.issuers` policy is
configured. Scope, role, actual activation, and browser storage are documented
in [AUTH.md](AUTH.md).

### IDs and names

- Organization and project IDs are text such as `org_...` and `pj_...`.
- Most project resource IDs are signed 64-bit integers in JSON.
- `external_id` is caller-defined where exposed; it is not universally accepted
  in path parameters.
- Wire properties and query parameters are `snake_case`; operation IDs remain
  OpenAPI-style camelCase identifiers.

### Error body

Typed handler errors use exactly `code` and `message` inside `error`:

```json
{
  "error": {
    "code": "invalid_limit",
    "message": "limit must be <= 500"
  }
}
```

`field`, `request_id`, `details`, RFC 9457 fields, and a
`problem+json` content type are not a universal shipped error contract. The
`x-request-id` response header is added by request-logging middleware, but it is
not inserted into `ErrorBody`.

Framework-level extraction/auth failures can have a framework response shape;
consult the generated response schemas and test the operation you consume.

### Pagination

Advertisers, campaigns, flights, creatives, creative templates, ads, sites, and
zones use bigserial cursor pagination:

```text
?limit=50&cursor=<base64url-json>
```

- default limit: 50;
- maximum: 500;
- order: `id DESC`;
- cursor payload: resource kind plus last ID; and
- a cursor from another resource returns `invalid_cursor`.

Taxonomy lists are bounded and return their full set. Project, token, and
ad-library list envelopes currently return `next_cursor: null` and have their
own safety caps. Keep filters stable between pages because the cursor does not
bind filter state.

### Mutations and idempotency

Do not assume every POST/PATCH implements a global write contract. Behavior is
per handler and represented in its generated responses:

- project creation accepts `Idempotency-Key` and has a replay cache path;
- batch-upsert handlers use one outer transaction and roll it back on a row
  failure, but the current resource loops generally stop at the first SQL error;
  they do not guarantee exhaustive per-row diagnostics (the shared savepoint
  helper is not wired into these handlers);
- resource create handlers generally return conflicts for duplicate external
  IDs rather than treating POST as a universal upsert; and
- PATCH support exists only where listed in the canonical table.

Use `:batchUpsert` only for resources with a generated operation. There is no
single batch endpoint spanning advertiser → campaign → flight → creative → ad.

### ETags

Responses expose `etag` on mutable resources. PATCH handlers that accept
`If-Match` call the shared checker where wired; generated parameters are the
operation-level authority. Do not infer PATCH support from an entity having an
etag.

## Decision API

### Request

A minimal request uses one real site ID and one ad-type ID:

```json
{
  "placements": [
    {
      "id": "header",
      "site_id": 12,
      "ad_types": [16]
    }
  ]
}
```

The generated `DecisionsRequest` fields are:

- optional `context` (`url`, `referrer`, `user_agent`);
- `placements` (1–32);
- optional `block` (`creative_ids`, `advertiser_ids`, `campaign_ids`); and
- optional `force_reason`.

A placement carries `id`, one of the currently useful site locators,
`zone_ids`, required non-empty `ad_types`, optional `count`, and optional
`force`. Current resolution supports numeric `site_id` and exact
`site_url`/alias. `site_external_id` is accepted by the schema but not present
in the snapshot and therefore yields no fill. `count` defaults to one and is
clamped to the range 1–10 rather than rejected above the maximum.

### Selection and response

The handler authenticates at reader level, proves project tenancy, then uses one
snapshot version for all placements. It filters active flights/ads, targeting,
and blocklists; keeps the lowest numeric priority tier; and performs weighted
selection without replacement.

A successful response has:

```json
{
  "snapshot_version": 42,
  "decisions": {
    "header": [
      {
        "ad_id": 1,
        "creative_id": 1,
        "flight_id": 1,
        "campaign_id": 1,
        "advertiser_id": 1,
        "priority_id": 2,
        "site_id": 12,
        "click_url": "/e/c/<signed>",
        "impression_url": "/e/i/<signed>",
        "creative": null
      }
    ]
  }
}
```

`decisions[placement_id]` is always an array; no eligible ad is an empty array.
The creative is a generated discriminated union for `image`, `html`, `native`,
or `templated`, and can be null. Templated creatives render Liquid from the
snapshot at decision time; a render failure skips that ad.

Tracking URLs are currently relative. `api.public_base_url` is parsed but not
used by `decide_pure` or its handler.

### Force overrides

Any placement carrying `force` activates a four-part gate:

1. global `decisions.force_overrides_enabled`;
2. project `allow_force_decision` in the loaded snapshot;
3. `admin` role or higher; and
4. an audit insert/commit before selection.

A failed control returns `force_disabled`. The implemented selector currently
applies `force.ad_id`; the other force fields are present in the request shape
but are not used to choose a candidate.

### Snapshot freshness

Every pod cold-loads, then checks the `config_version` sequence on a fixed
five-second interval. Management writes do not bump the sequence and no
LISTEN/NOTIFY path is active. A write is not guaranteed to appear in decisions
until an external bump or process restart. `snapshot_cold` is a 503 while the
project is absent from the loaded map.

### Event enqueue

Each selected ad composes one kind-0 decision event. The bounded sender is
non-blocking; saturation or a dead flusher returns 503. The flusher performs
per-row INSERTs, not COPY. See [REPORTING.md](REPORTING.md).

### Explainer

`decisions:explain` accepts the same request shape and returns selections plus
candidate/rule explanations. It does not enqueue an event or write the force
audit row. Its tracking values are placeholders rather than usable signed URLs.

## Creative image upload

The upload operation validates a maximum 40 MiB body and magic bytes for JPEG,
PNG, GIF, WebP, or AVIF; SVG is not accepted. The server always installs
`InMemoryStore`, so the returned `image_url` uses `memory://...`, data disappears
on restart, and replicas do not share it. This is suitable for tests and local
experimentation, not durable asset hosting.

## Event tracking direct routes

### Impression

`GET /e/i/{signed}` returns 204 whether verification succeeds or fails. With
`?fmt=gif`, it returns a 43-byte transparent GIF with 200. A verified hit
attempts to enqueue kind 1; channel failure is logged/dropped and does not change
the HTTP result.

### Click

`GET /e/c/{signed}` returns 400 for an invalid signature. A verified hit attempts
to enqueue kind 2 and redirects with 302 to the snapshot's click-through URL,
or `/` when none is found. The `u` query parameter is parsed but ignored because
no override target is present in the signed payload.

### Signature and dedup limitations

The compact payload contains project, ad, creative, placement hash, issue time,
and nonce. The current signer uses the same blob for click and impression; the
route path selects the recorded kind. The verifier's `kind` argument does not
bind the signature to one route.

Ping events derive a stable dedup key, but the database uniqueness constraint is
`(project_id, kind, dedup_key, ts)`. Replays at different timestamps do not
conflict. Read [REPORTING.md](REPORTING.md) before using `is_duplicate` for
billing or fraud controls.

The schema has previous-secret rotation columns, but the snapshot loader does
not load them and sets the previous secret to null. An eight-hour overlap is not
active behavior.

## System and admin routes

- `/healthz`: always `200` with `ok` while the process serves.
- `/readyz`: DB `SELECT 1`; explicit DB-less mode also returns 200. It does not
  check snapshot/event/leader health.
- `/version`: package/schema/build fields and effective opaque/JWT summary.
- `/openapi.json`: live generated spec.
- `/admin/config.json`: public OIDC issuer/client/scopes/require flag.
- `/admin/*`: optional static SPA. Both OIDC and paste-token flows use browser
  `sessionStorage`; see [SECURITY.md](SECURITY.md).

## Design-only operations not shipped

These nine operations appeared in older requirements or phase-era API tables.
They are intentionally outside the canonical table and have no generated
handler:

| Verb | Path | Classification |
|---|---|---|
| `POST` | `/v1/orgs/{org_id}/projects:batchUpsert` | Design only. |
| `PATCH` | `/v1/orgs/{org_id}/projects/{project_id}` | Design only. |
| `GET` | `/v1/orgs/{org_id}/members` | Design only. |
| `POST` | `/v1/orgs/{org_id}/members` | Design only. |
| `PATCH` | `/v1/orgs/{org_id}/members/{user_id}` | Design only. |
| `DELETE` | `/v1/orgs/{org_id}/members/{user_id}` | Design only. |
| `POST` | `/v1/orgs/{org_id}/ad-library/items:batchUpsert` | Design only. |
| `GET` | `/v1/orgs/{org_id}/ad-library/items/{item_id}/references` | Design only. |
| `PATCH` | `/v1/projects/{project_id}/creatives/{id}` | Design only. |

Other unshipped design areas include project users, reporting endpoints,
webhooks, custom event kinds, taxonomy writes, and site groups. Their mention in
[REQUIREMENTS.md](REQUIREMENTS.md), [E2E.md](E2E.md), or
[PHASES.md](PHASES.md) is not a compatibility commitment.
