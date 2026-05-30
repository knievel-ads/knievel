//! Integration test: the hourly rollup aggregates `events_raw` across
//! tenants (Phase 6.8 / C2b).
//!
//! Before the fix, `rollup::run_once` read `events_raw` on the raw
//! pool with no tenant GUC. Under FORCE'd RLS that read sees zero rows
//! (events_raw is org-scoped) and the watermark UPDATE matches nothing
//! (the watermark table has no write policy), so the rollup silently
//! produced nothing. Running under `SET LOCAL ROLE knievel_loader`
//! (BYPASSRLS) is what makes a cross-tenant aggregate work — so a
//! populated `events_rollup` spanning two orgs is itself proof of the
//! fix.
//!
//! Skipped (with a warning) when `DATABASE_URL` is not set.

use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};

use knievel::cli::seed_demo::{run, SeedDemoArgs, SeedDemoOutput};

fn skip_if_no_db() -> bool {
    if std::env::var("DATABASE_URL").is_err() {
        eprintln!("DATABASE_URL not set; skipping.");
        return true;
    }
    false
}

async fn seed(db_url: &str, org: &str, project: &str) -> Result<SeedDemoOutput> {
    run(SeedDemoArgs {
        database_url: db_url.to_string(),
        org_external_id: org.to_string(),
        project_external_id: project.to_string(),
        token: None,
        write_token_to: None,
    })
    .await
}

/// Insert `n` identical decision events for a project at `ts_secs`.
/// All dims are non-null (decision events from the live path that
/// carry a site/zone/flight/ad/creative), so they aggregate into a
/// single `events_rollup` row with `count = n`. Bound on the org GUC
/// so the events_raw RLS `WITH CHECK` is satisfied.
async fn seed_events(
    pool: &sqlx::PgPool,
    out: &SeedDemoOutput,
    ts_secs: i64,
    n: i64,
) -> Result<()> {
    let mut tx = knievel::db::begin_bound(pool, &out.org_id, Some(&out.project_id)).await?;
    for _ in 0..n {
        sqlx::query(
            "INSERT INTO knievel.events_raw
                (ts, org_id, project_id, kind, site_id, zone_id,
                 ad_id, creative_id, flight_id, is_duplicate)
             VALUES (to_timestamp($1::double precision),
                     $2, $3, 0, $4, $5, $6, $7, $8, false)",
        )
        .bind(ts_secs)
        .bind(&out.org_id)
        .bind(&out.project_id)
        .bind(out.site_id)
        .bind(out.zone_id)
        .bind(out.ad_id)
        .bind(out.creative_id)
        .bind(out.flight_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Insert `n` impression events (kind=1) the way `event_endpoints::
/// emit_event` writes them on the live path: NULL site/zone/flight,
/// only `ad_id`/`creative_id` set. These are exactly the rows whose
/// NULL dims would violate the `events_rollup` NOT-NULL primary key
/// and permanently wedge the rollup if the aggregate didn't coalesce
/// them to the `0` sentinel.
async fn seed_impressions(
    pool: &sqlx::PgPool,
    out: &SeedDemoOutput,
    ts_secs: i64,
    n: i64,
) -> Result<()> {
    let mut tx = knievel::db::begin_bound(pool, &out.org_id, Some(&out.project_id)).await?;
    for _ in 0..n {
        sqlx::query(
            "INSERT INTO knievel.events_raw
                (ts, org_id, project_id, kind, ad_id, creative_id, is_duplicate)
             VALUES (to_timestamp($1::double precision), $2, $3, 1, $4, $5, false)",
        )
        .bind(ts_secs)
        .bind(&out.org_id)
        .bind(&out.project_id)
        .bind(out.ad_id)
        .bind(out.creative_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Read the rolled-up count for a project + event kind.
async fn rollup_count(pool: &sqlx::PgPool, out: &SeedDemoOutput, kind: i16) -> Result<i64> {
    let mut tx = knievel::db::begin_bound(pool, &out.org_id, Some(&out.project_id)).await?;
    let total: Option<i64> = sqlx::query_scalar(
        "SELECT sum(count)::bigint FROM knievel.events_rollup
         WHERE project_id = $1 AND kind = $2",
    )
    .bind(&out.project_id)
    .bind(kind)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(total.unwrap_or(0))
}

#[tokio::test]
async fn rollup_aggregates_events_across_tenants() -> Result<()> {
    if skip_if_no_db() {
        return Ok(());
    }
    let db = testlib::db::ephemeral().await?;

    let a = seed(&db.url, "rollup-org-a", "rollup-project-a").await?;
    let b = seed(&db.url, "rollup-org-b", "rollup-project-b").await?;

    // Latest fully-settled hour boundary (matches rollup::run_once).
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let target = (now_secs - now_secs.rem_euclid(3600)) - 3600;
    // Mid the hour *before* target → falls in [target-3600, target),
    // the last hour the catch-up loop aggregates.
    let event_ts = target - 1800;

    seed_events(&db.pool, &a, event_ts, 3).await?;
    seed_events(&db.pool, &b, event_ts, 2).await?;
    // A NULL-dim impression in the same hour. Before the coalesce
    // fix this single row would abort the hour's aggregate with a
    // NOT NULL PK violation and wedge the whole rollup forever.
    seed_impressions(&db.pool, &a, event_ts, 1).await?;

    // Fresh DB → watermark at epoch 0; the cold-start clamp jumps it
    // to the earliest event hour rather than grinding 1970→now.
    let wm = knievel::rollup::run_once(&db.pool).await?;
    assert!(wm >= target, "watermark advanced past the event hour");

    assert_eq!(
        rollup_count(&db.pool, &a, 0).await?,
        3,
        "A decisions rolled up"
    );
    assert_eq!(
        rollup_count(&db.pool, &b, 0).await?,
        2,
        "B decisions rolled up"
    );
    // The NULL-dim impression rolled up under the 0 sentinel instead
    // of wedging the pipeline.
    assert_eq!(
        rollup_count(&db.pool, &a, 1).await?,
        1,
        "A impression (NULL dims) rolled up, not wedged"
    );

    // Re-running is safe: the watermark is already past the event
    // hour, so the second call is a no-op and never double-counts.
    let wm2 = knievel::rollup::run_once(&db.pool).await?;
    assert!(wm2 >= wm);
    assert_eq!(
        rollup_count(&db.pool, &a, 0).await?,
        3,
        "A stable on re-run"
    );
    assert_eq!(
        rollup_count(&db.pool, &b, 0).await?,
        2,
        "B stable on re-run"
    );

    testlib::db::ephemeral_drop(db).await?;
    Ok(())
}
