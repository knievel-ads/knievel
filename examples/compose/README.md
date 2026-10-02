# Knievel reference Compose stack

This stack runs PostgreSQL 16, the release image (or a local override), and a
manually invoked demo seeder. The canonical end-to-end command sequence is also
in the root [README](../../README.md).

## Start and seed

Run from the repository root so `tmp/` is the same host path mounted into the
seeder:

```sh
mkdir -p tmp

docker compose -f examples/compose/compose.yaml up -d \
  knievel-postgres knievel

until curl -fsS http://localhost:8080/healthz; do sleep 2; done

seed_out="$(docker compose -f examples/compose/compose.yaml run --rm \
  --user "$(id -u):$(id -g)" knievel-seed)"
printf '%s\n' "$seed_out"
```

Waiting for `/healthz` before seeding ensures server-side migrations have
created the schema. Running as the host UID/GID ensures
`tmp/knievel-dev-token` is writable and remains host-owned.

The CLI prints every generated/reused ID. Capture at least `project_id`,
`site_id`, and `ad_type_id`; do not assume they are `1`:

```sh
PROJECT_ID="$(printf '%s\n' "$seed_out" |
  sed -n 's/^seed-demo: .* project_id=\([^ ]*\)$/\1/p')"
SITE_ID="$(printf '%s\n' "$seed_out" |
  sed -n 's/^  creative_id=.* site_id=\([^ ]*\) zone_id=.*/\1/p')"
AD_TYPE_ID="$(printf '%s\n' "$seed_out" |
  sed -n 's/^  priority_id=.* ad_type_id=\([^ ]*\)$/\1/p')"
TOKEN="$(cat tmp/knievel-dev-token)"
```

`seed-demo` writes directly to PostgreSQL. Current management/CLI writes do not
bump `config_version`, so the already-running server will not poll the new rows.
Restart it to force a cold snapshot load:

```sh
docker compose -f examples/compose/compose.yaml restart knievel
until curl -fsS http://localhost:8080/healthz; do sleep 2; done
sleep 1
```

Issue a decision using the printed values and require a non-empty placement:

```sh
DECISION="$(curl -fsS -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  --data "{\"placements\":[{\"id\":\"header\",\"site_id\":${SITE_ID},\"ad_types\":[${AD_TYPE_ID}]}]}" \
  "http://localhost:8080/v1/projects/${PROJECT_ID}/decisions")"

printf '%s\n' "$DECISION"
printf '%s' "$DECISION" | python3 -c \
  'import json, sys; assert json.load(sys.stdin)["decisions"]["header"]'
```

A `snapshot_cold` response means the asynchronous cold load lost the race; wait
a second and retry.

## Local image

The Dockerfile is runtime-only. Compose build syntax is therefore not the local
build path. Build binaries, the admin UI, and the image through xtask:

```sh
cargo xtask build-image --tag knievel:local
KNIEVEL_IMAGE=knievel:local \
  docker compose -f examples/compose/compose.yaml up -d \
  knievel-postgres knievel
```

The default image is the mutable `0` major tag. For a controlled run, override
`KNIEVEL_IMAGE` with `ghcr.io/knievel-ads/knievel@sha256:<digest>`.

## Files

```text
examples/compose/
├── compose.yaml   # PostgreSQL, server, and one-shot seeder definitions
├── config.yaml    # effective Rust config fields for local use
├── init.sql       # non-superuser app role and loader-role bootstrap
└── README.md
```

The dev bootstrap grants broad default table writes to `knievel_loader` so the
rollup works after auto-migration. Production should use the narrower grants in
[DEPLOYMENT.md](../../DEPLOYMENT.md).

## Inspect and stop

```sh
docker compose -f examples/compose/compose.yaml logs -f knievel
docker compose -f examples/compose/compose.yaml exec knievel-postgres \
  psql -U knievel_app -d knievel

docker compose -f examples/compose/compose.yaml down -v
```

Uploaded creative images are process-local memory and disappear on restart.
There is no `/metrics` endpoint, OTel exporter, or Sentry SDK in this stack.
