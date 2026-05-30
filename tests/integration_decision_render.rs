//! Integration test: decision-time templated rendering (Phase 6.8).
//!
//! Exercises the full path the unit tests can't: a `templated`
//! creative stored in Postgres → loaded into the in-RAM snapshot via
//! the BYPASSRLS loader → rendered server-side at decision time →
//! returned in `decisions[].creative.body` over real HTTP.
//!
//! Two properties:
//!   1. A `templated` creative renders against its `values` plus the
//!      injected decision context (the freshly-signed click/impression
//!      URLs, placement id, snapshot version).
//!   2. Cross-tenant render isolation — the per-project `creatives`
//!      map means project A's decision renders A's template, never B's,
//!      even though one snapshot holds both.
//!
//! Skipped (with a warning) when `DATABASE_URL` is not set.

use anyhow::Result;
use poem::test::TestClient;
use poem::EndpointExt;
use serde_json::json;

use knievel::cli::seed_demo::{run, SeedDemoArgs, SeedDemoOutput};
use knievel::snapshot::{load_snapshot, SnapshotStore};

fn skip_if_no_db() -> bool {
    if std::env::var("DATABASE_URL").is_err() {
        eprintln!("DATABASE_URL not set; skipping.");
        return true;
    }
    false
}

async fn seed(db_url: &str, org: &str, project: &str, token: &str) -> Result<SeedDemoOutput> {
    run(SeedDemoArgs {
        database_url: db_url.to_string(),
        org_external_id: org.to_string(),
        project_external_id: project.to_string(),
        token: Some(token.to_string()),
        write_token_to: None,
    })
    .await
}

/// Insert a Liquid creative-template + a `templated` creative that
/// references it, then repoint the seeded ad at the templated creative
/// so selection picks it. Returns the templated creative_id.
async fn install_templated_creative(
    pool: &sqlx::PgPool,
    out: &SeedDemoOutput,
    template_src: &str,
    values: serde_json::Value,
    click_through: &str,
) -> Result<i64> {
    let mut tx = knievel::db::begin_bound(pool, &out.org_id, Some(&out.project_id)).await?;

    let template_id: i64 = sqlx::query_scalar(
        "INSERT INTO knievel.creative_templates
            (org_id, project_id, external_id, name, schema, template, template_engine)
         VALUES ($1, $2, 'tpl-card', 'card',
                 '{\"type\":\"object\"}'::jsonb, $3, 'liquid')
         RETURNING id",
    )
    .bind(&out.org_id)
    .bind(&out.project_id)
    .bind(template_src)
    .fetch_one(&mut *tx)
    .await?;

    let creative_id: i64 = sqlx::query_scalar(
        "INSERT INTO knievel.creatives
            (org_id, project_id, advertiser_id, external_id, name, kind,
             template_id, values, click_through_url)
         VALUES ($1, $2, $3, 'tpl-creative', 'Templated Creative', 'templated',
                 $4, $5, $6)
         RETURNING id",
    )
    .bind(&out.org_id)
    .bind(&out.project_id)
    .bind(out.advertiser_id)
    .bind(template_id)
    .bind(&values)
    .bind(click_through)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("UPDATE knievel.ads SET creative_id = $1 WHERE id = $2")
        .bind(creative_id)
        .bind(out.ad_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(creative_id)
}

fn build_app(pool: sqlx::PgPool, snapshot: SnapshotStore) -> impl poem::Endpoint {
    let state = knievel::state::AppState::new()
        .with_db(pool)
        .with_snapshot(snapshot);
    knievel::server::routes().data(state)
}

async fn post_decision(
    cli: &TestClient<impl poem::Endpoint>,
    project_id: &str,
    token: &str,
    site_id: i64,
    ad_type_id: i64,
) -> serde_json::Value {
    let resp = cli
        .post(format!("/v1/projects/{project_id}/decisions"))
        .header("Authorization", format!("Bearer {token}"))
        .body_json(&json!({
            "placements": [{
                "id": "main",
                "site_id": site_id,
                "ad_types": [ad_type_id],
                "count": 1
            }]
        }))
        .send()
        .await;
    resp.assert_status_is_ok();
    resp.json().await.value().deserialize()
}

/// A `templated` creative renders its Liquid body server-side and the
/// rendered HTML carries both the creative's `values` and the
/// freshly-signed click URL.
#[tokio::test]
async fn templated_creative_renders_in_decision() -> Result<()> {
    if skip_if_no_db() {
        return Ok(());
    }
    let db = testlib::db::ephemeral().await?;
    let out = seed(
        &db.url,
        "render-org",
        "render-project",
        "kvl_dev_org_render_secret",
    )
    .await?;

    // Template touches every injected context channel + a value.
    let src = r#"<a href="{{ ad.click_url }}" data-imp="{{ ad.impression_url }}">{{ values.title }} @ {{ placement.id }} v{{ decision.snapshot_version }}</a>"#;
    install_templated_creative(
        &db.pool,
        &out,
        src,
        json!({ "title": "Buy Widgets" }),
        "https://demo.example.com/widgets",
    )
    .await?;

    let snapshot = SnapshotStore::new(load_snapshot(db.pool.clone()).await?);
    let cli = TestClient::new(build_app(db.pool.clone(), snapshot));

    let body = post_decision(
        &cli,
        &out.project_id,
        &out.token,
        out.site_id,
        out.ad_type_id,
    )
    .await;

    let ads = body["decisions"]["main"].as_array().expect("main array");
    assert_eq!(ads.len(), 1, "exactly one ad selected");
    let ad = &ads[0];

    assert_eq!(ad["creative"]["type"], json!("templated"));
    assert_eq!(ad["creative"]["template"], json!("card"));
    assert_eq!(ad["creative"]["values"]["title"], json!("Buy Widgets"));
    assert_eq!(
        ad["creative"]["click_through_url"],
        json!("https://demo.example.com/widgets")
    );

    // The real creative_id (not the old hardcoded 0) is surfaced.
    assert!(ad["creative_id"].as_i64().unwrap() > 0, "real creative_id");

    let rendered = ad["creative"]["body"].as_str().expect("rendered body");
    let click_url = ad["click_url"].as_str().expect("click url");
    assert!(
        rendered.contains("Buy Widgets"),
        "value rendered: {rendered}"
    );
    assert!(
        rendered.contains("@ main"),
        "placement id injected: {rendered}"
    );
    assert!(
        rendered.contains(click_url),
        "signed click url injected into body: body={rendered} click_url={click_url}"
    );

    testlib::db::ephemeral_drop(db).await?;
    Ok(())
}

/// Two projects in two orgs, each with its own templated creative, are
/// served from one snapshot. Each project's decision renders its own
/// template — the other tenant's `values` never leak in.
#[tokio::test]
async fn templated_render_is_tenant_isolated() -> Result<()> {
    if skip_if_no_db() {
        return Ok(());
    }
    let db = testlib::db::ephemeral().await?;

    let a = seed(
        &db.url,
        "iso-org-a",
        "iso-project-a",
        "kvl_dev_org_isoa_secret",
    )
    .await?;
    let b = seed(
        &db.url,
        "iso-org-b",
        "iso-project-b",
        "kvl_dev_org_isob_secret",
    )
    .await?;

    let src = r#"<span>{{ values.brand }}</span>"#;
    install_templated_creative(
        &db.pool,
        &a,
        src,
        json!({ "brand": "AlphaBrand" }),
        "https://a.example/x",
    )
    .await?;
    install_templated_creative(
        &db.pool,
        &b,
        src,
        json!({ "brand": "BetaBrand" }),
        "https://b.example/x",
    )
    .await?;

    // One snapshot holds both tenants (loader reads across orgs).
    let snapshot = SnapshotStore::new(load_snapshot(db.pool.clone()).await?);
    let cli = TestClient::new(build_app(db.pool.clone(), snapshot));

    let body_a = post_decision(&cli, &a.project_id, &a.token, a.site_id, a.ad_type_id).await;
    let body_b = post_decision(&cli, &b.project_id, &b.token, b.site_id, b.ad_type_id).await;

    let rendered_a = body_a["decisions"]["main"][0]["creative"]["body"]
        .as_str()
        .expect("a body");
    let rendered_b = body_b["decisions"]["main"][0]["creative"]["body"]
        .as_str()
        .expect("b body");

    assert!(
        rendered_a.contains("AlphaBrand"),
        "A renders A: {rendered_a}"
    );
    assert!(
        !rendered_a.contains("BetaBrand"),
        "A must not leak B: {rendered_a}"
    );
    assert!(
        rendered_b.contains("BetaBrand"),
        "B renders B: {rendered_b}"
    );
    assert!(
        !rendered_b.contains("AlphaBrand"),
        "B must not leak A: {rendered_b}"
    );

    testlib::db::ephemeral_drop(db).await?;
    Ok(())
}
