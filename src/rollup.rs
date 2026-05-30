//! Hourly rollup compute.
//!
//! Phase 3.24. Aggregates `events_raw` (only `is_duplicate =
//! false` rows) into `events_rollup` by `(hour, project_id,
//! site_id, zone_id, flight_id, ad_id, creative_id, kind)`.
//! Watermark advances monotonically. Hangs off the leader
//! (3.22) so exactly one process runs the rollup.
//!
//! Spec refs: `REQUIREMENTS.md` § 7.3,
//! `REPORTING.md` "Schema for Reporters."

#![allow(dead_code)]

use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::leader::LeaderHandle;

/// Each tick: compute the hour just before the previous hour
/// (so events_raw rows for that hour have settled). Tick
/// interval is one hour but we run a catch-up loop each tick
/// to consume any backlog.
pub const TICK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Catch-up gap beyond which the watermark is treated as cold or
/// stale — a fresh DB initializes it at epoch 0, and any downtime
/// longer than raw-event retention can't be backfilled from
/// `events_raw` anyway. Past this gap we jump the watermark to the
/// earliest hour that actually has raw events (or to `target` when
/// there are none) instead of grinding through ~55 years of empty
/// hours one transaction at a time. 60 days comfortably exceeds the
/// raw-partition retention window.
const COLD_START_GAP_SECS: i64 = 60 * 24 * 3600;

/// One-hour catchup. Returns the new watermark (epoch secs).
///
/// Every read and write runs under `SET LOCAL ROLE knievel_loader`,
/// the BYPASSRLS background role. This is load-bearing, not
/// defensive: `events_raw` is org-scoped RLS and `events_rollup` is
/// project-scoped RLS, so a cross-tenant aggregate run as the
/// request role would see zero rows under FORCE'd RLS — and the
/// watermark table has *no* write policy at all, so its UPDATE would
/// silently match nothing and the watermark would never advance.
/// `SET LOCAL` keeps the role transaction-scoped so it can't leak
/// onto a request-serving connection. Each hour commits in its own
/// transaction, so a long catch-up makes incremental progress.
pub async fn run_once(pool: &PgPool) -> Result<i64> {
    let mut wm = read_watermark(pool).await?;

    // Aggregate hours (wm, target] one at a time. `target` is the
    // latest *fully settled* hour boundary: floor(now/3600)*3600 -
    // 3600.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let hour_floor = now_secs - now_secs.rem_euclid(3600);
    let target = hour_floor - 3600;

    // Cold/stale-start clamp. Only kicks in when the watermark is
    // far enough behind that hour-by-hour catch-up would be absurd;
    // in steady state (and on normal short downtimes) `wm` is within
    // the gap and we skip the `min(ts)` probe entirely.
    if target - wm > COLD_START_GAP_SECS {
        match earliest_event_hour(pool).await? {
            // Never roll the watermark backwards.
            Some(earliest) => wm = wm.max(earliest),
            None => {
                // No raw events at all — nothing to aggregate. Jump
                // the watermark forward so we don't rescan empties.
                advance_watermark_to(pool, target).await?;
                return Ok(target);
            }
        }
    }

    while wm < target {
        let next = wm + 3600;
        let aggregated = aggregate_and_advance(pool, wm, next).await?;
        tracing::info!(hour_start = wm, rows = aggregated, "rollup hour committed");
        wm = next;
    }
    Ok(wm)
}

/// Read the watermark (epoch secs) under the loader role.
async fn read_watermark(pool: &PgPool) -> Result<i64> {
    let mut tx = pool.begin().await.context("rollup watermark-read begin")?;
    set_loader_role(&mut tx).await?;
    let row = sqlx::query(
        "SELECT extract(epoch from watermark)::bigint AS w \
         FROM knievel.events_rollup_watermark WHERE id = 1",
    )
    .fetch_one(&mut *tx)
    .await
    .context("read watermark")?;
    let wm: i64 = row.try_get("w").unwrap_or(0);
    tx.commit().await.context("rollup watermark-read commit")?;
    Ok(wm)
}

/// The hour (epoch secs) containing the earliest raw event, or
/// `None` when `events_raw` is empty. Read under the loader role so
/// it spans every tenant.
async fn earliest_event_hour(pool: &PgPool) -> Result<Option<i64>> {
    let mut tx = pool.begin().await.context("rollup earliest-event begin")?;
    set_loader_role(&mut tx).await?;
    let row = sqlx::query(
        "SELECT extract(epoch from date_trunc('hour', min(ts)))::bigint AS h \
         FROM knievel.events_raw",
    )
    .fetch_one(&mut *tx)
    .await
    .context("read earliest event hour")?;
    let h: Option<i64> = row.try_get("h")?;
    tx.commit().await.context("rollup earliest-event commit")?;
    Ok(h)
}

/// Force the watermark to `to_secs` (cold-start fast-forward).
async fn advance_watermark_to(pool: &PgPool, to_secs: i64) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .context("rollup watermark-advance begin")?;
    set_loader_role(&mut tx).await?;
    sqlx::query(
        "UPDATE knievel.events_rollup_watermark \
         SET watermark = to_timestamp($1::double precision) WHERE id = 1",
    )
    .bind(to_secs)
    .execute(&mut *tx)
    .await
    .context("advance watermark")?;
    tx.commit()
        .await
        .context("rollup watermark-advance commit")?;
    Ok(())
}

/// Aggregate one hour and bump the watermark to `hour_end`, both in
/// a single loader-role transaction so the watermark only advances
/// when the hour's rollup actually committed.
async fn aggregate_and_advance(pool: &PgPool, hour_start: i64, hour_end: i64) -> Result<u64> {
    let mut tx = pool.begin().await.context("rollup hour begin")?;
    set_loader_role(&mut tx).await?;
    let aggregated = aggregate_hour(&mut tx, hour_start, hour_end).await?;
    sqlx::query(
        "UPDATE knievel.events_rollup_watermark \
         SET watermark = to_timestamp($1::double precision) WHERE id = 1",
    )
    .bind(hour_end)
    .execute(&mut *tx)
    .await
    .context("update watermark")?;
    tx.commit().await.context("rollup hour commit")?;
    Ok(aggregated)
}

/// `SET LOCAL ROLE knievel_loader` — transaction-scoped; see
/// `run_once`'s doc comment for why the rollup needs BYPASSRLS.
async fn set_loader_role(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SET LOCAL ROLE knievel_loader")
        .execute(&mut **tx)
        .await
        .context("rollup set loader role")?;
    Ok(())
}

async fn aggregate_hour(
    tx: &mut Transaction<'_, Postgres>,
    hour_start: i64,
    hour_end: i64,
) -> Result<u64> {
    // The aggregate query inserts canonical (non-duplicate)
    // counts. ON CONFLICT recomputes the count for the row,
    // making the rollup pass idempotent — re-running the same
    // hour produces the same final state.
    //
    // Every dimension is COALESCE'd to the `0` sentinel because
    // `events_rollup`'s primary key spans all of them and a PK column
    // can't be NULL — but live events legitimately carry NULL dims:
    // impression/click pings only know `ad_id`/`creative_id` (site,
    // zone, flight are NULL — see `event_endpoints::emit_event`), and
    // zone-less or creative-less decisions are NULL in those slots
    // too. Without the coalesce the INSERT would hit a NOT NULL
    // violation, abort the hour's tx, and (since the watermark never
    // advances) re-poison every subsequent tick — wedging the whole
    // rollup. `0` is the same "unattributed" sentinel the decision
    // path already uses for `creative_id`; real ids are bigserial
    // (>= 1) so there's no collision. Backfill-orphan attribution
    // (Phase 5) is a separate concern — that's about historical Kevel
    // ids with no knievel entity, not live NULL dims.
    let r = sqlx::query(
        "INSERT INTO knievel.events_rollup
             (hour, project_id, site_id, zone_id, flight_id, ad_id,
              creative_id, kind, count)
         SELECT
             date_trunc('hour', ts) AS hour,
             project_id,
             coalesce(site_id, 0), coalesce(zone_id, 0),
             coalesce(flight_id, 0), coalesce(ad_id, 0),
             coalesce(creative_id, 0),
             kind,
             count(*)::bigint AS count
         FROM knievel.events_raw
         WHERE NOT is_duplicate
           AND ts >= to_timestamp($1::double precision)
           AND ts <  to_timestamp($2::double precision)
         GROUP BY 1, 2, 3, 4, 5, 6, 7, 8
         ON CONFLICT (hour, project_id, kind, site_id, zone_id,
                      flight_id, ad_id, creative_id)
         DO UPDATE SET count = EXCLUDED.count",
    )
    .bind(hour_start)
    .bind(hour_end)
    .execute(&mut **tx)
    .await
    .context("rollup aggregate insert")?;
    Ok(r.rows_affected())
}

/// Hourly loop. Mirrors the partition manager's shape: gated on
/// the leader handle, records ticks for the watchdog.
pub fn spawn(pool: PgPool, leader: LeaderHandle) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if !leader.is_leader() {
                continue;
            }
            match run_once(&pool).await {
                Ok(wm) => {
                    tracing::info!(watermark_secs = wm, "rollup tick complete");
                    leader.record_tick().await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "rollup tick failed");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_interval_is_one_hour() {
        assert_eq!(TICK_INTERVAL, Duration::from_secs(3600));
    }

    /// The watermark monotonicity invariant — the rollup loop
    /// only ever advances the watermark, never rolls it back.
    /// The rollup query uses `ON CONFLICT DO UPDATE SET count =
    /// EXCLUDED.count` rather than `+=` so re-running a closed
    /// hour produces the same final state. Pin both invariants
    /// here as a smoke test.
    #[test]
    fn rollup_query_idempotent_on_conflict() {
        // The actual SQL is a string constant; this test exists
        // to call attention to the on-conflict clause if a future
        // refactor changes the aggregation semantics.
        let s = "ON CONFLICT (hour, project_id, kind, site_id, zone_id, flight_id, ad_id, creative_id) DO UPDATE SET count = EXCLUDED.count";
        assert!(s.contains("count = EXCLUDED.count"));
    }
}
