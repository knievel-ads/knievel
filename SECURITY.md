# Security policy

## Report a vulnerability privately

Use this repository's
[GitHub private vulnerability reporting form](https://github.com/knievel-ads/knievel/security/advisories/new).
Private reporting is enabled and routes through GitHub's repository security
advisory workflow. Do not open a public issue or discussion for a suspected
vulnerability.

Include, when possible:

- the affected tag or commit;
- reproduction steps or a minimal proof of concept;
- expected and observed behavior;
- impact, prerequisites, and tenant boundary involved; and
- whether exploitation is known in the wild.

The maintainers will coordinate investigation, remediation, credit, and a
publication date in the private advisory. This project does not advertise a paid
bug-bounty program.

## Supported revisions

Security fixes target `main` and the latest tagged release. Backports to older
pre-1.0 tags are assessed case by case; no fixed support window is promised.
The GitHub Releases page and security advisories are the source for published
fixes.

## Shipped trust boundaries

### Server API

Management and decision operations require an `Authorization: Bearer` header.
The caller is responsible for authenticating its own users and protecting the
bearer. Knievel accepts:

- opaque `kvl_...` tokens looked up under a one-row RLS bootstrap and verified
  with Argon2; and
- JWT-shaped tokens when at least one `auth.jwt.issuers` policy is configured.

Opaque mode is not disabled by an `auth.modes` field. The opaque token's `env`
segment is parsed but not checked against deployment configuration. Stored IP
allowlists are not enforced by the request path. Project-token mint validates
that a project ID is present but does not prove it belongs to the path org, so
minting must remain a trusted org-admin operation. JWT runtime verification
currently builds decoding keys only for RSA JWKs; configure RS256 rather than
relying on the parsed ES256 default. See [AUTH.md](AUTH.md).

Public direct routes are deliberately different:

- `/healthz`, `/readyz`, `/version`, `/openapi.json`, and
  `/admin/config.json` are unauthenticated;
- `/e/i/{signed}` and `/e/c/{signed}` use the HMAC-bearing path as
  authorization; and
- `/admin/*` serves browser application files when enabled.

Put system metadata behind a reverse proxy if it should not be internet-visible.
Signed event URLs are credentials until expiry: avoid leaking them through
analytics, referrers, support logs, or chat transcripts.

### Browser admin application

The first-party admin SPA is a browser-facing authenticated surface, not merely
a server-to-server API viewer. OIDC Authorization Code + PKCE and the
paste-token fallback both leave bearer material in `window.sessionStorage`.
OIDC state also uses `sessionStorage`; tokens survive refresh in the tab and are
removed with the tab/session, but they are not HttpOnly.

Consequences:

- any same-origin XSS can read and exfiltrate the bearer;
- browser extensions and local device compromise are outside the server's
  boundary;
- TLS, CSP/reverse-proxy headers, origin isolation, and admin network exposure
  are operator responsibilities; and
- `admin_ui.oidc.require_oidc=true` hides paste-token login but does not change
  browser storage.

The UI reads `/v1/whoami` and hides controls by role, but that is usability only.
Every authorization decision remains server-side. Do not grant the browser an
org-owner token when a narrower project role is sufficient.

### Tenant isolation

Tenant tables use PostgreSQL `ENABLE ROW LEVEL SECURITY` and `FORCE ROW LEVEL
SECURITY`. A project request follows the security-sensitive path
`auth/security.rs` → `db.rs` → `handlers.rs`:

1. authenticate to a `Principal`;
2. bind only the principal's `org_id`;
3. verify that the path project belongs to that org; and
4. bind `project_id` for the remaining transaction.

The application DB role must be `NOSUPERUSER NOBYPASSRLS`; PostgreSQL
superusers bypass forced RLS. CI's migration and manifest gates add useful
coverage, but `tests/cross_tenant_manifest.toml` is an operation registry and
does not itself execute its diagnostic `test` names.

### Background loader role

The process also needs a `NOLOGIN BYPASSRLS` role named `knievel_loader` for
cross-tenant snapshot reads and rollup work. It is assumed only with
transaction-scoped `SET LOCAL ROLE`; request handlers must never use it.
Production grants should limit it to SELECT on snapshot/event inputs and
INSERT/UPDATE on the two rollup outputs. A login-capable or broadly writable
loader role expands the blast radius substantially.

### Event and image data

Event buffering and uploaded image bytes are process memory:

- event batches can be lost on DB flush failure or process shutdown;
- upload bytes disappear on restart and are not shared across replicas; and
- the raw-event uniqueness key includes `ts`, so replay dedup is not a
  billing-grade guarantee across different timestamps.

Do not use the in-memory upload operation as durable content storage. Review
[REPORTING.md](REPORTING.md) before treating `is_duplicate` or rollups as a
security/fraud control.

## Operator-owned controls

Knievel does not terminate TLS or ship a WAF, DDoS control, Kubernetes
NetworkPolicy, backup encryption, secret manager, durable object store, or
Prometheus endpoint. Operators own:

- TLS and trusted proxy configuration;
- network segmentation and admin-route exposure;
- DB role creation, credential rotation, backups, and encryption;
- OIDC client/issuer policy and short browser token lifetimes;
- protection of opaque tokens and HMAC event URLs;
- archive/drop policy for detached event partitions; and
- image hosting when durable creative assets are required.

The distroless image runs as non-root, and the chart defaults to a read-only root
filesystem and dropped Linux capabilities. Those defaults do not replace the
controls above.

## Disclosure history

Published reports appear as GitHub Security Advisories for this repository.
