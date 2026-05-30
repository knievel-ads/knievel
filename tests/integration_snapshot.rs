//! Integration test: the snapshot loader reads every project's decision
//! config across tenants — via the BYPASSRLS `knievel_loader` role — into
//! the in-RAM snapshot. A plain pool read would see zero rows here (every
//! table is FORCE'd RLS with no tenant GUC bound), so a populated snapshot
//! is itself proof the cross-tenant loader path works.
//!
//! Skipped (with a warning) when `DATABASE_URL` is not set.

use anyhow::Result;
use knievel::cli::seed_demo::{run, SeedDemoArgs};
use knievel::snapshot::load_snapshot;

#[tokio::test]
async fn load_snapshot_reads_seeded_project_across_tenants() -> Result<()> {
    if std::env::var("DATABASE_URL").is_err() {
        eprintln!("DATABASE_URL not set; skipping.");
        return Ok(());
    }

    let db = testlib::db::ephemeral().await?;

    let seeded = run(SeedDemoArgs {
        database_url: db.url.clone(),
        org_external_id: "demo-org".into(),
        project_external_id: "demo-project".into(),
        token: None,
        write_token_to: None,
    })
    .await?;

    // No tenant GUC is bound here — the loader reads across all tenants
    // through the BYPASSRLS role.
    let snap = load_snapshot(db.pool.clone()).await?;

    let proj = snap
        .projects
        .get(&seeded.project_id)
        .expect("snapshot carries the seeded project");
    assert_eq!(proj.project_id, seeded.project_id);
    assert!(!proj.org_id_for_event.is_empty(), "org carried for events");
    assert!(
        !proj.hmac_secret.is_empty(),
        "per-project hmac secret loaded"
    );

    assert!(
        proj.flights.iter().any(|f| f.id == seeded.flight_id),
        "seeded flight present in snapshot"
    );
    assert!(
        proj.ads
            .iter()
            .any(|a| a.id == seeded.ad_id && a.creative_id == Some(seeded.creative_id)),
        "seeded ad present with its creative_id"
    );
    assert!(
        proj.sites.iter().any(|s| s.id == seeded.site_id),
        "seeded site present"
    );
    assert!(
        proj.zones.iter().any(|z| z.id == seeded.zone_id),
        "seeded zone present"
    );

    testlib::db::ephemeral_drop(db).await?;
    Ok(())
}
