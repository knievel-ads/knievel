# Chaos / degraded-mode skeletons

The repository currently contains **nine deferred skeletons**:

- `chaos_db_writer_unreachable.rs`
- `chaos_listen_drops.rs`
- `chaos_notify_overflow.rs`
- `chaos_aurora_failover.rs`
- `chaos_event_channel_saturation.rs`
- `chaos_jwks_unreachable.rs`
- `chaos_pool_exhaustion.rs`
- `chaos_leader_watchdog_miss.rs`
- `chaos_minio_midflight.rs`

Each file has one `#[tokio::test]`, and every scenario is `#[ignore]` with the
missing injection mechanism in its reason. There is no compose fault-injection
harness under `tests/chaos/` today. Consequently neither per-change CI nor
`.github/workflows/nightly.yml` executes or claims chaos coverage.

`cargo xtask test-shape` enforces only this boundary: `chaos_*` is an accepted,
explicitly deferred target class, every test in that class remains ignored, and
no CI selector silently starts running it. That gate does **not** prove degraded
behavior.

## Activation contract

Activating a scenario requires one focused change that:

1. adds a reproducible harness (for example an isolated compose project);
2. implements the fault injection and recovery assertion;
3. removes `#[ignore]` from that scenario;
4. adds an explicit workflow selector whose failures are not masked; and
5. updates `TESTING.md` with the coverage actually observed.

Likely mechanisms include `iptables`/`tc`, `pg_terminate_backend`, process pause
or kill, and a controllable JWKS/MinIO stand-in. Those are design notes, not
present capabilities.

See `REQUIREMENTS.md` § 10.9 and `TESTING.md` § 9 for the intended behavior.
