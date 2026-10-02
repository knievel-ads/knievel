# Knievel authentication and authorization

This document describes the current server implementation in
[`src/auth/`](src/auth/), [`src/config.rs`](src/config.rs), and
[`src/handlers.rs`](src/handlers.rs). The generated schemas and operations are
in [`openapi.yaml`](openapi.yaml) and [API.md](API.md).

## Activation model

Every generated management and decision operation uses the same bearer security
scheme. Two credential forms can coexist:

| Form | When active | Runtime path |
|---|---|---|
| Opaque `kvl_...` | Whenever a DB pool exists | Parse ID, one-row RLS lookup, Argon2 verify. |
| JWT | When `auth.jwt.issuers` contains at least one policy and the bearer looks like a JWT | Match issuer, fetch/cache JWKS, verify signature/claims, build principal. |

There is no effective `auth.modes` configuration field. Unknown YAML is ignored,
so `auth.modes: [jwt]` does **not** disable opaque tokens. An empty JWT issuer
list gives an opaque-only deployment; a non-empty list enables both paths.

A JWT-shaped bearer that fails JWT verification returns unauthorized and does
not fall back to the opaque lookup. Non-JWT bearer strings fall through to the
opaque parser.

`GET /version` reports effective modes as `opaque` plus `jwt` when issuer
policies are configured. It also returns public issuer summaries. This is
visibility, not startup validation: JWKS discovery/fetch occurs on demand.

## Principal

Both credential paths produce the same internal fields:

```text
token_type: opaque | jwt
scope:      org | project
org_id:     text
project_id: text or null
role:       reader | editor | admin | org-admin | org-owner
actor_id:   opaque audit identifier
```

IDs for organizations and projects are text. Resource IDs below the project
boundary are generally `bigint`.

Roles are ordered from least to most privilege:

```text
reader < editor < admin < org-admin < org-owner
```

An org-scoped principal can address projects in its org after the server proves
the path project's membership. A project-scoped principal must match the path
project exactly. Project-scoped principals are rejected from org-scoped token
operations. Role names alone do not bypass scope or tenant checks.

## Opaque tokens

### Format and storage

The parser accepts:

```text
kvl_<env>_<scope>_<id_short>_<secret>
```

where `scope` is `org` or `project`. The row key is
`tok_<id_short>`. The plaintext secret is returned once at mint time and only an
Argon2 PHC hash is stored.

The `env` segment is currently structural only; authentication does not compare
it with a deployment environment. API-minted tokens hard-code `prod` in that
segment, while `seed-demo` generates or accepts `dev` tokens. Do not rely on the
prefix to prevent a token copied between environments; separate databases and
secret handling provide that boundary.

### Authentication transaction

Before a token is verified, its tenant is unknown. The opaque path therefore:

1. parses `id_short` without trusting the secret;
2. begins a transaction and sets transaction-local
   `knievel.auth_lookup_id=tok_<id_short>`;
3. queries that exact live token row under the RLS bootstrap policy;
4. rejects revoked and expired rows in SQL;
5. verifies the supplied secret with Argon2; and
6. creates a `Principal`.

The transaction rolls back on drop. The bootstrap policy does not grant a write
path.

### Mint/list/revoke behavior

`POST /v1/orgs/{org_id}/tokens`, list, and revoke require `org-admin` or higher.
Mint validates scope/project/role shape, stores the secret hash, and writes an
audit row in the same transaction. Revoke sets `revoked_at`; the next opaque
lookup rejects the row.

Current field caveats:

- `expires_at` is present in the generated request schema but the mint INSERT
  does not persist it. Do not assume newly minted API tokens expire.
- `ip_allowlist` is stored but no request-IP enforcement exists.
- For `scope=project`, mint validates that `project_id` is present but does not
  query that the project belongs to the path org. The FK targets a global
  project primary key and the token policy's write check binds only `org_id`.
  A trusted org-admin must supply an in-org project ID; do not expose minting to
  callers that can inject arbitrary known project IDs.
- list is capped at 500 and returns `next_cursor: null`.

Use short-lived JWTs where enforced expiry is required today, or rotate/revoke
opaque tokens operationally.

## JWTs

### Effective configuration fields

`JwtIssuerConfig` consumes exactly these fields:

| Field | Required | Effect |
|---|---|---|
| `issuer` | yes | Exact `iss` match and OIDC discovery base. |
| `audience` | yes | Required `aud` membership. |
| `algorithms` | no | Per-issuer allowlist; config default is `[RS256, ES256]`. |
| `jwks_url` | no | Explicit JWKS URL; empty triggers OIDC discovery. |
| `claim` | no | Authz claim name; default `knievel`. |
| `claim_mapping.rules` | no | Ordered fallback rules when the named claim is absent. |

There are no effective issuer fields for `claim_format`, cache TTL, clock skew,
refresh intervals, client secrets, or mode selection. Those names in older
examples are ignored.

A minimal, currently usable RSA policy is:

```yaml
auth:
  jwt:
    issuers:
      - issuer: https://identity.example.com/realms/ads
        audience: knievel
        algorithms: [RS256]
        jwks_url: ""
        claim: knievel
        claim_mapping:
          rules: []
```

Environment overrides use double underscores, but lists are easier and less
error-prone in the YAML mounted through `KNIEVEL_CONFIG`.

### Verification behavior

For a JWT-shaped bearer, the runtime:

1. decodes the header and rejects HMAC algorithms or a missing `kid`;
2. decodes untrusted payload only to select a policy by exact `iss`;
3. requires the header algorithm in that policy's allowlist;
4. uses explicit `jwks_url`, or fetches
   `{issuer}/.well-known/openid-configuration` and reads `jwks_uri`;
5. finds the JWK by `kid`, refreshing once when a cached key set lacks it;
6. verifies signature, issuer, audience, and time claims with
   `jsonwebtoken`; and
7. extracts the configured authz claim or applies the first matching mapping
   rule.

The HTTP client has a fixed five-second timeout. JWKS entries have a fixed
one-hour in-process cache TTL, and validation uses fixed 30-second leeway. These
are constants, not config fields. Cache state is per process.

Although ES256 appears in the config default, the current decoding-key builder
implements RSA JWKs (`kty=RSA`, `n`, `e`) only. An EC key reaches signature
failure. Configure RS256 until EC key construction is implemented and tested.
Unsigned and HMAC JWTs are not accepted.

### Authz claim

The default named claim is an object:

```json
{
  "knievel": {
    "scope": "project",
    "org_id": "org_example",
    "project_id": "pj_example",
    "role": "editor"
  }
}
```

For org scope, omit or set `project_id` to null. The server does not accept a
flat family of `knievel_scope`, `knievel_org_id`, and similar claims.

`actor_id` for a JWT is currently `jwt:<sub>`; issuer and authorized-party are
not included. Treat audit actor strings as implementation identifiers, not a
stable external identity schema.

### Claim mapping

When the named claim is absent, ordered mapping rules can derive a principal
from exact top-level string claims. Every entry in `match` must match; first
rule wins:

```yaml
auth:
  jwt:
    issuers:
      - issuer: https://kubernetes.default.svc.cluster.local
        audience: knievel
        algorithms: [RS256]
        claim: knievel
        claim_mapping:
          rules:
            - match:
                sub: system:serviceaccount:ads:publisher
              principal:
                scope: project
                org_id: org_example
                project_id: pj_example
                role: editor
```

A present named claim takes precedence even when it is malformed; mapping is a
missing-claim fallback, not a rescue path. Keep mappings narrow and use projected
service-account tokens with the configured audience. Knievel does not call the
Kubernetes TokenReview API; it trusts issuer signature, claims, audience, and
the static mapping.

## Authorization and RLS

### Project operations

Generated project handlers call `open_project_tx` with a minimum role. The
security-critical sequence is:

1. reject an insufficient role;
2. for project-scoped tokens, reject a different path project;
3. begin a transaction with only `knievel.org_id` set;
4. query the path project under the org-only RLS policy; and
5. after ownership succeeds, set `knievel.project_id` locally.

The two-stage bind prevents an attacker-supplied project ID from satisfying the
project policy before org membership is proved. Resource SQL then runs inside
that bound transaction.

A PostgreSQL superuser bypasses forced RLS. The application role must be
`NOSUPERUSER NOBYPASSRLS`. `knievel_loader` is a separate background-only
`NOLOGIN BYPASSRLS` role and is never an HTTP principal.

### Minimum roles in current handlers

| Operation family | Minimum role |
|---|---|
| Project/org reads, taxonomy, decisions, explainer | `reader` |
| Project resource creates/updates/batch upserts, image upload | `editor` |
| Org ad-library read | `reader` |
| Org ad-library create/update | `editor` |
| Project creation | `org-admin` |
| Token mint/list/revoke | `org-admin` |
| Honored `force.*` decision override | `admin`, plus project flag and global flag |

There are no shipped member-management, project-update, or project-batch-upsert
operations; role tables in historical designs that include them are not the
current API.

## Browser admin authentication

The admin SPA reads public OIDC settings from `/admin/config.json`.

- When issuer and client ID are present, `oidc-client-ts` uses Authorization
  Code + PKCE, automatic silent renewal, and `sessionStorage`.
- When OIDC is absent, or when fallback is allowed, an operator can paste an
  opaque token. The SPA validates it with `/v1/whoami` and stores it in the same
  tab's `sessionStorage`.
- `admin_ui.oidc.require_oidc=true` hides the paste-token fallback.

Both flows expose bearer material to JavaScript running on the admin origin.
Use TLS, a tightly controlled origin/CSP, short upstream access-token TTLs, and
least-privilege roles. The server does not issue an HttpOnly admin session
cookie. [SECURITY.md](SECURITY.md) defines the full browser boundary.

## Direct event authorization

`/e/i/{signed}` and `/e/c/{signed}` do not use bearer auth. The signed path is
the credential and is verified against project signing material in the current
snapshot. Invalid impressions are deliberately silent; invalid clicks return a
bad-signature response. Keep these URLs out of logs and referrers where
possible.

Signing-secret rotation columns exist in the schema, but the snapshot loader
currently sets `hmac_secret_previous` to `None`; do not claim an active overlap
window until loader and rotation behavior are implemented together.

## Local provisioning

The reference seeder is DB-direct because a fresh install has no bearer from
which to mint the first token. Run the sequence in [README.md](README.md): start
DB/server, wait for migrations, run `knievel-seed` as the host UID/GID, restart
the server for a cold snapshot load, then use the token file.

For production-shaped bootstrap without demo demand rows, the shipped CLI is:

```sh
DATABASE_URL='postgres://knievel_app:secret@db/knievel' \
  knievel-cli admin create-org \
  --external-id acme \
  --name 'Acme' \
  --write-token-to /secure/path/acme-token
```

The parent directory must already exist. The CLI-generated bootstrap token uses
a `dev` prefix even in this path; as noted above, that prefix is not an enforced
environment boundary.

## Operational checklist

- Keep the app role non-superuser and the loader role `NOLOGIN`.
- Prefer RS256 and verify issuer/audience values exactly.
- Confirm `/version` shows the intended issuer count and claim source.
- Exercise `/v1/whoami` with each credential class before rollout.
- Treat browser/sessionStorage and signed event URLs as bearer boundaries.
- Rotate or revoke opaque tokens because API-mint expiry is not persisted.
- Do not use ignored legacy fields such as `auth.modes` as controls.
