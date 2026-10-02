# Knievel reporting contract

This document describes the tables created by
[`migrations/0010_events_raw.sql`](migrations/0010_events_raw.sql) and
[`migrations/0011_events_rollup.sql`](migrations/0011_events_rollup.sql), plus
the current writers in [`src/events.rs`](src/events.rs) and
[`src/rollup.rs`](src/rollup.rs). It does not describe a built-in reporting HTTP
API; none ships.

## Important operational truth

- Events are buffered in process memory.
- The flusher batches in memory but executes one GUC bind and one SQL INSERT per
  row inside a transaction. Legacy comments calling this COPY are inaccurate.
- Failed flushes are logged and the batch is dropped; there is no durable queue.
- Raw leaves are detached at retention, not dropped.
- The rollup table is named `knievel.events_rollup`, not
  `events_rollup_hourly`.
- Replay dedup is timestamp-sensitive because the actual unique constraint
  includes `ts`. Do not treat `is_duplicate` as a billing-grade guarantee.

## Event kinds

The `smallint` values are fixed by `src/events.rs` and the migration:

| Value | Kind | Producer |
|---|---|---|
| `0` | decision | A selected ad from the decision handler. |
| `1` | impression | A successfully verified `/e/i/{signed}` request. |
| `2` | click | A successfully verified `/e/c/{signed}` request. |

Older examples using `1/2/3` or string kinds do not match storage.

## `knievel.events_raw`

`events_raw` is range-partitioned on `ts`. Its parent schema is:

| Column | PostgreSQL type | Current producer semantics |
|---|---|---|
| `id` | `bigserial` | Part of the `(id, ts)` primary key. |
| `ts` | `timestamptz` | Event timestamp; partition key. |
| `org_id` | `text` | Owning organization. |
| `project_id` | `text` | Owning project. |
| `kind` | `smallint` | `0`, `1`, or `2` as above. |
| `placement_id` | `text` nullable | Present on decision rows; pings carry only its signed hash. |
| `site_id` | `bigint` nullable | Present on decisions, absent on current ping rows. |
| `zone_id` | `bigint` nullable | First requested zone on decision rows. |
| `ad_id` | `bigint` nullable | Selected/signed ad. |
| `creative_id` | `bigint` nullable | Attached/signed creative; zero is converted to null for pings. |
| `flight_id` | `bigint` nullable | Present on decision rows. |
| `campaign_id` | `bigint` nullable | Present on decision rows. |
| `advertiser_id` | `bigint` nullable | Present on decision rows. |
| `url` | `text` nullable | Decision `context.url`. |
| `referrer_host` | `text` nullable | Host extracted from decision referrer. |
| `user_agent_hash` | `bytea` nullable | SHA-256 of decision user agent. |
| `signature_nonce` | `bytea` nullable | Decision/signature nonce. |
| `dedup_key` | `bytea` nullable | HMAC-derived for impression/click; null for decisions. |
| `snapshot_version` | `bigint` nullable | Snapshot sequence value observed by producer. |
| `is_duplicate` | `boolean` | Defaults false; conflict path sets true. |

RLS binds this table by `org_id`, not project ID. Query the partitioned parent
with a time predicate to get partition pruning and the parent policy.

### Timestamp-sensitive dedup caveat

The intended logical key is `(project_id, kind, dedup_key)`, but PostgreSQL
requires a unique constraint on a partitioned table to include its partition
key. The shipped constraint is:

```sql
UNIQUE (project_id, kind, dedup_key, ts)
```

The flusher uses:

```sql
ON CONFLICT (project_id, kind, dedup_key, ts)
DO UPDATE SET is_duplicate = true
```

A replayed ping receives a new `ts_ms` in the event endpoint. Unless both hits
share the exact stored timestamp, they are distinct unique keys and both remain
`is_duplicate=false`. `dedup_key` is useful for downstream grouping, but
`WHERE NOT is_duplicate` alone does not currently remove replays across
timestamps.

For downstream canonicalization, choose a business window and rank by the
stable key, for example:

```sql
SELECT *
FROM (
  SELECT e.*,
         row_number() OVER (
           PARTITION BY project_id, kind, dedup_key
           ORDER BY ts, id
         ) AS replay_rank
  FROM knievel.events_raw AS e
  WHERE org_id = 'org_example'
    AND project_id = 'pj_example'
    AND kind IN (1, 2)
    AND ts >= now() - interval '1 day'
) AS ranked
WHERE replay_rank = 1;
```

That policy is downstream-owned until the storage constraint changes. Decision
rows have null `dedup_key` and are not replay-deduplicated.

## Partitions and retention

Migration `0010` creates a broad seed leaf for calendar year 2026. The runtime
partition manager then attempts to create four daily leaves starting at the
current UTC date. During 2026 those ranges overlap the attached year-wide leaf,
so PostgreSQL rejects the first daily CREATE and `run_once` returns before the
retention loop. There is no default partition; an event outside attached bounds
fails loudly.

When a maintenance pass reaches retention, leaves with names older than the
configured cutoff are detached from `events_raw`. `DETACH PARTITION` removes
them from parent queries but does not delete the table or its bytes. Operators
must repair the overlapping seed layout, then inventory, archive, and drop
detached tables themselves. `partitions.retention_days` defaults to 30 in Rust;
the current Helm chart does not render that block.

## `knievel.events_rollup`

The leader-elected rollup processes settled hours under
`SET LOCAL ROLE knievel_loader`. For rows where `is_duplicate=false`, it groups
by:

```text
hour, project_id, kind, site_id, zone_id,
flight_id, ad_id, creative_id
```

and writes `count bigint`. Every nullable dimension is converted to `0`, the
“unattributed” sentinel, because all dimensions participate in the primary key.
Real resource IDs start above zero.

| Column | PostgreSQL type |
|---|---|
| `hour` | `timestamptz` |
| `project_id` | `text` |
| `site_id` | `bigint` (`0` when absent) |
| `zone_id` | `bigint` (`0` when absent) |
| `flight_id` | `bigint` (`0` when absent) |
| `ad_id` | `bigint` (`0` when absent) |
| `creative_id` | `bigint` (`0` when absent) |
| `kind` | `smallint` |
| `count` | `bigint` |

`ON CONFLICT ... DO UPDATE SET count = EXCLUDED.count` makes recomputing one
hour replace, rather than add to, its prior aggregate. The table has no
automatic retention.

Because the rollup trusts `is_duplicate`, it inherits the timestamp caveat
above. It is the aggregate of the server's current flag, not an independently
re-deduplicated billing fact.

### Watermark

`knievel.events_rollup_watermark` is a one-row table whose timestamp advances in
the same loader-role transaction as a successful hour. On a fresh DB it starts
at the Unix epoch; the rollup fast-forwards a very stale empty watermark to the
earliest raw-event hour or the current target.

Use strictly older rollup hours and an at-or-after raw tail to avoid overlap:

```sql
WITH watermark AS (
  SELECT watermark
  FROM knievel.events_rollup_watermark
  WHERE id = 1
), hourly AS (
  SELECT hour AS ts, count AS n
  FROM knievel.events_rollup, watermark
  WHERE project_id = 'pj_example'
    AND kind = 1
    AND hour < watermark.watermark
    AND hour >= now() - interval '7 days'

  UNION ALL

  SELECT date_trunc('hour', ts) AS ts, count(*)::bigint AS n
  FROM knievel.events_raw, watermark
  WHERE org_id = 'org_example'
    AND project_id = 'pj_example'
    AND kind = 1
    AND ts >= watermark.watermark
    AND ts >= now() - interval '7 days'
    AND NOT is_duplicate
  GROUP BY 1
)
SELECT ts, sum(n) AS impressions
FROM hourly
GROUP BY ts
ORDER BY ts;
```

This reflects the server's duplicate flag. Apply the downstream window/ranking
rule from the prior section if replay-resistant counts are required.

## RLS-capable reporting access

A plain `GRANT SELECT` is insufficient because tables force RLS. Provision a
non-login, non-bypass reader role after the app role exists:

```sql
CREATE ROLE knievel_reader NOLOGIN NOSUPERUSER NOBYPASSRLS;
GRANT USAGE ON SCHEMA knievel TO knievel_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA knievel TO knievel_reader;
ALTER DEFAULT PRIVILEGES FOR ROLE knievel_app IN SCHEMA knievel
  GRANT SELECT ON TABLES TO knievel_reader;

GRANT knievel_reader TO analytics_service;
```

The trusted analytics login must use an explicit read-only transaction and bind
the tenant GUCs before selecting:

```sql
BEGIN TRANSACTION READ ONLY;
SET LOCAL ROLE knievel_reader;
SELECT set_config('knievel.org_id', 'org_example', true);
SELECT set_config('knievel.project_id', 'pj_example', true);

SELECT hour, kind, sum(count)
FROM knievel.events_rollup
WHERE project_id = 'pj_example'
  AND hour >= now() - interval '7 days'
GROUP BY hour, kind
ORDER BY hour, kind;

COMMIT;
```

Use the same transaction shape for `events_raw` and the watermark. The GUCs are
session inputs, not cryptographic authorization: a direct SQL analytics login
that can set arbitrary values is a trusted operator identity with potential
cross-tenant visibility. Do not expose it to untrusted end users. Keep the role
`NOBYPASSRLS` so missing bindings fail closed.

A read replica is suitable for reporting if its lag is acceptable. The running
Knievel server itself must use the primary because it writes configuration,
events, rollups, and partitions; current code does not use LISTEN despite older
writer-endpoint explanations.

## Capacity arithmetic

The following is arithmetic, not a measured throughput claim. At a sustained
20,000 event rows per second:

```text
20,000 × 86,400 = 1,728,000,000 rows/day
1,728,000,000 × 30 = 51,840,000,000 rows/30 days
51,840,000,000 × 200 bytes = 10,368,000,000,000 bytes
```

That is **1.728 billion rows/day**, **51.84 billion rows over 30 days**, and
approximately **10.37 TB at 200 bytes/row before PostgreSQL page, tuple, index,
WAL, backup, and replica overhead**. The actual schema has wide nullable columns
and two parent constraints, so 200 bytes is only a simplifying assumption, not
a sizing guarantee.

The current per-row INSERT implementation has not demonstrated that sustained
rate. Benchmark the exact hardware, connection pool, WAL/storage, partition
shape, and query load before selecting retention. Detached leaves still count
against physical storage.

## Recommended downstream model

1. Read a bounded raw window from the partitioned parent under a tenant-bound
   transaction.
2. Preserve `(id, ts)` as the physical row identity.
3. Map `0/1/2` to names in downstream SQL, not at ingestion.
4. For pings, derive the desired replay policy from
   `(project_id, kind, dedup_key)` and event time.
5. Snapshot mutable dimensions if historical names/targeting matter.
6. Use `events_rollup` only when its dimensions and current duplicate semantics
   are sufficient.
7. Monitor watermark lag and detached-table bytes independently.

## Current non-features

- No reporting HTTP endpoints or reporting UI.
- No CDC/Kafka sink.
- No shipped dbt project or `examples/dbt/` directory.
- No raw-partition index configuration field.
- No automated archive/drop of detached leaves.
- No proven COPY ingest path.

Build these downstream only after accounting for RLS, watermark, and dedup
semantics above.
