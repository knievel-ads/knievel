# Knievel Helm chart

This chart deploys the Knievel server/admin image and renders a YAML ConfigMap.
Read the root [deployment guide](../../DEPLOYMENT.md) before production use;
several legacy chart values are intentionally disclosed as ignored or stubbed.

## Minimal install

```sh
helm upgrade --install knievel \
  oci://ghcr.io/knievel-ads/charts/knievel \
  --set database.host=db.example \
  --set database.name=knievel \
  --set database.existingSecret=knievel-db \
  --set api.publicBaseUrl=https://ads.example
```

Requirements:

- a Kubernetes cluster supporting `apps/v1` and `networking.k8s.io/v1`;
- reachable PostgreSQL (the reference and CI environment use PostgreSQL 16);
- a non-superuser application role and separately provisioned loader role;
- a Secret containing `username` and `password`; and
- an explicit production `api.publicBaseUrl`.

The chart default for `api.publicBaseUrl` is a localhost value only so strict
lint/render works. Override it in every non-local install. The server currently
parses this field but still emits relative `/e/...` tracking paths.

## Image tags and digests

A release Git tag has a leading `v`, but the current image workflow publishes
semver tags without it: `X.Y.Z`, `X.Y`, and `X`, plus `sha-<commit>`.

A tag value renders with `:`:

```yaml
image:
  repository: ghcr.io/knievel-ads/knievel
  tag: "X.Y.Z"
```

A real digest value renders with `@`:

```yaml
image:
  repository: ghcr.io/knievel-ads/knievel
  tag: "sha256:<manifest-digest>"
```

`sha-<commit>` is an image tag, not an OCI digest. Prefer the manifest digest
for reproducible rollout. When `image.tag` is empty, the chart uses
`.Chart.AppVersion`; the release workflow overrides chart version/appVersion at
package time.

The release manifest is keyless-signed. Verify a pinned digest with:

```sh
cosign verify ghcr.io/knievel-ads/knievel@sha256:<manifest-digest> \
  --certificate-identity-regexp \
    'https://github.com/knievel-ads/knievel/.github/workflows/release.yml.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

## Database Secret and roles

The default key names are `username` and `password`:

```yaml
database:
  host: db.example
  port: 5432
  name: knievel
  sslMode: require
  existingSecret: knievel-db
  userKey: username
  passwordKey: password
  maxConnections: 8
  autoMigrate: true
```

The ConfigMap references Secret-projected environment variables rather than
containing credentials. If `existingSecret` is empty, those variables are not
created and config interpolation fails at startup.

The Secret's login must be `NOSUPERUSER NOBYPASSRLS`. Provision
`knievel_loader NOLOGIN BYPASSRLS`, membership, and least-privilege grants before
starting the workload. Exact SQL is in [DEPLOYMENT.md](../../DEPLOYMENT.md).

## Effective runtime values

| Value | Current effect |
|---|---|
| `image.*`, `replicaCount` | Workload image and replicas. |
| `database.host`, `port`, `name`, `sslMode`, Secret keys | PostgreSQL URL. |
| `database.maxConnections`, `autoMigrate` | Rust DB pool and boot migration behavior. |
| `events.channelCapacity` | In-memory event queue capacity. |
| `decisions.forceOverridesEnabled` | Global force gate. |
| `logging.level`, `format`, `requestLog*` | Working structured/request logging. |
| `api.bindAddr`, `publicBaseUrl` | Typed API config; base URL is parsed but not applied to tracking paths. |
| `adminUi.oidc.*` | Public SPA OIDC runtime metadata. |
| `auth.jwt.issuers` | Enables JWT verification alongside opaque tokens. |
| ingress/service/probes/scheduling/security contexts | Kubernetes workload behavior. |

Use RS256 JWT policies; current runtime decoding-key construction supports RSA
JWKs only.

## Ignored and unsupported values

The existing workload templates still reference these values, so they remain in
`values.yaml`, but operators must not rely on them:

| Value | Why unsupported |
|---|---|
| `events.retentionDays` | Renders under `events.retention_days`; Rust expects `partitions.retention_days`. Runtime stays at its default. |
| `events.flushIntervalMs`, `events.flushBatchSize` | Rust uses constants and ignores these keys. |
| `logging.decisionsSampleRate` | No consumer in `LoggingConfig`. |
| `sentry.*` | Template renders an unrecognized top-level block; even the recognized Rust block is SDK-stubbed. |
| `otel.*` | Template renders an unrecognized top-level block; even the recognized Rust block has no exporter. |
| `serviceMonitor.enabled` | Renders a scrape for `/metrics`, but Knievel has no `/metrics` route. Leave false. |

The chart also cannot configure persistent image storage. Creative uploads use
process-local memory and disappear on restart.

## Admin UI

The release image sets the static admin directory by default, so `/admin/` is
served even though the chart does not render `admin_ui.static_dir`. Configure
OIDC public metadata through `adminUi.oidc`. With `requireOidc=false`, users can
paste an opaque token.

Both OIDC and paste-token flows use browser `sessionStorage`. Use TLS, restrict
admin reachability, and review the root [security policy](../../SECURITY.md).

## Probes and metrics

- Liveness `/healthz` checks process HTTP serving.
- Readiness `/readyz` checks DB `SELECT 1` only; it does not check snapshot or
  event-flusher health.
- There is no metrics endpoint. Do not enable ServiceMonitor.

## Topology example

Scheduling values are passed through directly. A hard zone spread can be set as
follows, after adapting label selectors to the rendered release labels:

```yaml
topologySpreadConstraints:
  - maxSkew: 1
    topologyKey: topology.kubernetes.io/zone
    whenUnsatisfiable: DoNotSchedule
    labelSelector:
      matchLabels:
        app.kubernetes.io/name: knievel
```

## Validate a values change

```sh
helm lint --strict charts/knievel
helm template knievel charts/knievel \
  --set database.host=db.example \
  --set database.name=knievel \
  --set database.existingSecret=knievel-db \
  --set api.publicBaseUrl=https://ads.example >/tmp/knievel-rendered.yaml
```

Rendering proves Kubernetes/YAML shape only. Compare the rendered ConfigMap with
[`src/config.rs`](../../src/config.rs) before describing a chart value as
working.
