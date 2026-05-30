//! In-process configuration snapshot.
//!
//! Phase 3.17. Decision-path RAM cache of every project's
//! flights, ads, sites, zones, and per-project secrets/flags.
//! Atomically swappable so reads never block writes.
//!
//! Spec: `REQUIREMENTS.md` § 7.2 — refresh is a notify+poll
//! belt-and-suspenders:
//!
//! 1. `LISTEN config_changed` on a long-lived writer connection.
//!    On notify, diff against the snapshot's current
//!    `config_version` and pull anything newer.
//! 2. Poll `SELECT last_value FROM knievel.config_version`
//!    every 5 s as a backstop. NOTIFY can drop messages under
//!    load, and Aurora failovers drop the LISTEN session.
//!
//! Both triggers reach the same diff-pull path; worst-case
//! staleness is bounded by the poll interval regardless.
//!
//! The in-memory shape is keyed by `(project_id, resource)` so
//! one process can serve thousands of small projects without
//! the per-project overhead a per-project snapshot map would
//! incur.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Result;
use sqlx::{PgPool, Row};
use tokio::sync::Notify;

use crate::selection::{Ad, Flight};

/// Wire-level snapshot keyed by project_id. Cheaply cloneable
/// because every leaf is `Arc`-backed; the swap is one atomic
/// pointer write.
#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub config_version: i64,
    pub projects: HashMap<String, Arc<ProjectSnapshot>>,
}

/// Per-project slice of the snapshot.
#[derive(Debug, Default)]
pub struct ProjectSnapshot {
    pub project_id: String,
    /// Owning org. Carried in-snapshot so the events flusher can
    /// attach `org_id` to ping rows without a per-request DB
    /// round-trip (`REQUIREMENTS.md` § 7.3 RLS-by-org).
    pub org_id_for_event: String,
    pub flights: Vec<Flight>,
    pub ads: Vec<Ad>,
    pub sites: Vec<SnapshotSite>,
    pub zones: Vec<SnapshotZone>,
    /// `ad_id → click_through_url` lookup for `/e/c/...`
    /// redirect resolution (`API.md` § 4). Populated by the
    /// snapshot loader from the creative attached to each ad;
    /// keyed on ad_id so the click endpoint resolves with no
    /// additional creative-id round-trip beyond what the
    /// signed payload already carries. Ads whose creative has
    /// no `clickThroughUrl` simply don't appear here — the
    /// click handler falls through to a safe placeholder.
    pub click_through_urls: HashMap<i64, String>,
    /// `creative_id → creative` lookup for the decision response's
    /// typed `creative` block (`API.md` § 1 / § 3.5). Carries the
    /// per-kind fields plus, for `templated` creatives, the Liquid
    /// source pulled from the joined template so the decision
    /// handler can render `body` server-side with no DB round-trip
    /// (Phase 6.8). Keyed on creative_id; an ad whose `creative_id`
    /// isn't here simply gets a null `creative`.
    pub creatives: HashMap<i64, SnapshotCreative>,
    /// Current HMAC signing secret. The decision endpoint signs
    /// new URLs with this; the event endpoints accept either
    /// this OR `hmac_secret_previous` (during the 8 h overlap).
    pub hmac_secret: Vec<u8>,
    pub hmac_secret_previous: Option<Vec<u8>>,
    pub allow_force_decision: bool,
}

#[derive(Debug, Clone)]
pub struct SnapshotSite {
    pub id: i64,
    pub url: String,
    pub aliases: Vec<String>,
}

/// A creative as the decision path sees it. Mirrors the
/// `API.md` § 3.5 `oneOf` (the `kind` column is the
/// discriminator) and carries the joined template's `name` and
/// Liquid `source` so the decision handler can build the typed
/// `creative` block — and render `templated` bodies — entirely
/// from RAM.
#[derive(Debug, Clone)]
pub struct SnapshotCreative {
    pub id: i64,
    /// `image` | `html` | `native` | `templated`.
    pub kind: String,
    pub image_url: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub alt: Option<String>,
    /// Verbatim HTML for `html` creatives.
    pub body: Option<String>,
    /// The referenced template's `name` (`creative_templates.name`),
    /// echoed as `creative.template` in the decision response for
    /// `native`/`templated` creatives. `None` for image/html.
    pub template_name: Option<String>,
    /// The referenced template's Liquid `source`
    /// (`creative_templates.template`). Present only when the
    /// template carries a renderable body — i.e. for `templated`
    /// creatives. `native` creatives leave this `None` (the caller
    /// renders client-side).
    pub template_source: Option<String>,
    /// The creative's `values` map. Defaults to an empty object
    /// when the column is NULL so the renderer and the wire shape
    /// always see an object.
    pub values: serde_json::Value,
    pub click_through_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SnapshotZone {
    pub id: i64,
    pub site_id: i64,
}

/// The handle handlers actually carry. `read()` returns a cheap
/// `Arc<Snapshot>` that's a consistent view across the whole
/// request — no torn reads.
#[derive(Clone)]
pub struct SnapshotStore {
    inner: Arc<RwLock<Arc<Snapshot>>>,
    /// Bumped by the loader on every successful swap. Tests
    /// `await_at_least(version)` on this to coordinate with the
    /// background task without sleeps.
    pub bumped: Arc<Notify>,
}

impl SnapshotStore {
    pub fn new(initial: Snapshot) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Arc::new(initial))),
            bumped: Arc::new(Notify::new()),
        }
    }

    pub fn empty() -> Self {
        Self::new(Snapshot::default())
    }

    /// Atomic read. Returned `Arc` is a consistent point-in-time
    /// view; subsequent swaps don't affect it.
    pub fn read(&self) -> Arc<Snapshot> {
        self.inner.read().expect("snapshot lock poisoned").clone()
    }

    /// Atomic write. Replaces the entire snapshot in one pointer
    /// swap; readers either see the old or the new, never a
    /// half-built state.
    pub fn swap(&self, next: Snapshot) {
        let mut guard = self.inner.write().expect("snapshot lock poisoned");
        *guard = Arc::new(next);
        drop(guard);
        self.bumped.notify_waiters();
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::empty()
    }
}

/// Read the current `config_version` sequence value. Used both
/// at boot (to initialize the snapshot's version) and by the
/// 5 s poll backstop.
pub async fn read_config_version(pool: &PgPool) -> Result<i64> {
    let row = sqlx::query("SELECT last_value FROM knievel.config_version")
        .fetch_one(pool)
        .await?;
    let v: i64 = row.try_get(0)?;
    Ok(v)
}

/// Background task: notify+poll snapshot loader. Runs forever
/// until the parent task is dropped. The loader is intentionally
/// resilient — every error path logs and retries with backoff
/// rather than panicking, since a missed reload is recoverable
/// (worst case: we serve a stale snapshot for a few seconds)
/// and a panic would tear down the parent runtime.
///
/// `reload` is the user-supplied function that fetches the
/// fresh snapshot from the DB. Splitting it out keeps this
/// module testable without a real Postgres connection.
pub async fn run_loader<F, Fut>(pool: PgPool, store: SnapshotStore, mut reload: F)
where
    F: FnMut(PgPool) -> Fut + Send,
    Fut: std::future::Future<Output = Result<Snapshot>> + Send,
{
    use tokio::time::{interval, MissedTickBehavior};
    let mut tick = interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Cold load.
    match reload(pool.clone()).await {
        Ok(snap) => store.swap(snap),
        Err(e) => tracing::error!(error = %e, "snapshot cold load failed; will retry"),
    }

    // NOTIFY listener integration is deferred — sqlx 0.8's
    // PgListener works against a writer connection but the
    // surrounding "diff and merge" path needs the events_raw
    // tables (Phase 3.20+) to materialize the per-resource
    // selects. For Phase 3.17 we rely on the 5 s poll loop to
    // pick up version drift, which the spec documents as the
    // backstop and a sufficient guarantee on its own.
    //
    // See `REQUIREMENTS.md` § 7.2: "worst-case staleness is
    // bounded by the poll interval regardless of NOTIFY
    // behavior."
    loop {
        tick.tick().await;
        let cur_version = store.read().config_version;
        let db_version = match read_config_version(&pool).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "config_version poll failed");
                continue;
            }
        };
        if db_version <= cur_version {
            continue;
        }
        match reload(pool.clone()).await {
            Ok(snap) => store.swap(snap),
            Err(e) => tracing::error!(error = %e, "snapshot reload failed; will retry"),
        }
    }
}

/// Concrete snapshot reload: read every project's config from the DB
/// into a fresh [`Snapshot`]. This is the function `run_loader` calls.
///
/// Cross-tenant by necessity — one process serves all projects — so it
/// reads under `SET LOCAL ROLE knievel_loader`, a `BYPASSRLS`
/// background-only role provisioned by the operator (see
/// `MIGRATION_RX.md`). The per-request path keeps strict per-tenant
/// RLS; only this trusted in-process loader is exempt. `SET LOCAL`
/// scopes the role to this one transaction, so it can never leak back
/// onto the pool's request-serving connections.
pub async fn load_snapshot(pool: PgPool) -> Result<Snapshot> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL ROLE knievel_loader")
        .execute(&mut *tx)
        .await?;

    let config_version: i64 = sqlx::query("SELECT last_value FROM knievel.config_version")
        .fetch_one(&mut *tx)
        .await?
        .try_get(0)?;

    // Seed one ProjectSnapshot per active project.
    let mut projects: HashMap<String, ProjectSnapshot> = HashMap::new();
    for r in sqlx::query(
        "SELECT id, org_id, hmac_secret, allow_force_decision \
         FROM knievel.projects WHERE is_active",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let id: String = r.try_get("id")?;
        projects.insert(
            id.clone(),
            ProjectSnapshot {
                project_id: id,
                org_id_for_event: r.try_get("org_id")?,
                hmac_secret: r.try_get("hmac_secret")?,
                // Rotation overlap (hmac_secret_previous) is not modeled
                // on the projects row yet; None = no previous secret.
                hmac_secret_previous: None,
                allow_force_decision: r.try_get("allow_force_decision")?,
                ..Default::default()
            },
        );
    }

    // Flights — join priorities for the numeric tier; dates → epoch ms.
    for r in sqlx::query(
        "SELECT f.project_id, f.id, f.campaign_id, c.advertiser_id, p.tier, \
                (extract(epoch from f.start_date) * 1000)::bigint AS start_ms, \
                (extract(epoch from f.end_date)   * 1000)::bigint AS end_ms, \
                f.site_ids, f.zone_ids, f.ad_types, f.is_active \
         FROM knievel.flights f \
         JOIN knievel.priorities p ON p.id = f.priority_id \
         JOIN knievel.campaigns  c ON c.id = f.campaign_id",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            proj.flights.push(crate::selection::Flight {
                id: r.try_get("id")?,
                campaign_id: r.try_get("campaign_id")?,
                advertiser_id: r.try_get("advertiser_id")?,
                priority_tier: r.try_get("tier")?,
                start_ms: r.try_get("start_ms")?,
                end_ms: r.try_get("end_ms")?,
                site_ids: r.try_get("site_ids")?,
                zone_ids: r.try_get("zone_ids")?,
                ad_types: r.try_get("ad_types")?,
                is_active: r.try_get("is_active")?,
            });
        }
    }

    // Ads.
    for r in sqlx::query(
        "SELECT project_id, id, flight_id, creative_id, weight, is_active FROM knievel.ads",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            proj.ads.push(crate::selection::Ad {
                id: r.try_get("id")?,
                flight_id: r.try_get("flight_id")?,
                creative_id: r.try_get("creative_id")?,
                weight: r.try_get("weight")?,
                is_active: r.try_get("is_active")?,
            });
        }
    }

    // Sites.
    for r in sqlx::query("SELECT project_id, id, url, aliases FROM knievel.sites")
        .fetch_all(&mut *tx)
        .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            proj.sites.push(SnapshotSite {
                id: r.try_get("id")?,
                url: r.try_get("url")?,
                aliases: r.try_get("aliases")?,
            });
        }
    }

    // Zones.
    for r in sqlx::query("SELECT project_id, id, site_id FROM knievel.zones")
        .fetch_all(&mut *tx)
        .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            proj.zones.push(SnapshotZone {
                id: r.try_get("id")?,
                site_id: r.try_get("site_id")?,
            });
        }
    }

    // click_through_urls: ad_id → its creative's click_through_url.
    for r in sqlx::query(
        "SELECT a.project_id, a.id AS ad_id, c.click_through_url \
         FROM knievel.ads a \
         JOIN knievel.creatives c ON c.id = a.creative_id \
         WHERE c.click_through_url IS NOT NULL",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            let ad_id: i64 = r.try_get("ad_id")?;
            let url: String = r.try_get("click_through_url")?;
            proj.click_through_urls.insert(ad_id, url);
        }
    }

    // Creatives — keyed by creative_id, with the template name +
    // Liquid source joined in for `native`/`templated` rendering.
    //
    // The `ct.project_id = c.project_id` predicate is load-bearing:
    // this runs under the BYPASSRLS loader role, and the creatives FK
    // on `template_id` references the *global* template PK, so without
    // it a creative that somehow carried another project's template_id
    // would pull that tenant's template name/source into this
    // project's map. The same-project guard makes the loader
    // self-defending regardless of how the row was written.
    for r in sqlx::query(
        "SELECT c.project_id, c.id, c.kind, c.image_url, c.width, c.height, \
                c.alt, c.body, c.values, c.click_through_url, \
                ct.name AS template_name, ct.template AS template_source \
         FROM knievel.creatives c \
         LEFT JOIN knievel.creative_templates ct \
                ON ct.id = c.template_id AND ct.project_id = c.project_id",
    )
    .fetch_all(&mut *tx)
    .await?
    {
        let pid: String = r.try_get("project_id")?;
        if let Some(proj) = projects.get_mut(&pid) {
            let id: i64 = r.try_get("id")?;
            let values: Option<serde_json::Value> = r.try_get("values")?;
            proj.creatives.insert(
                id,
                SnapshotCreative {
                    id,
                    kind: r.try_get("kind")?,
                    image_url: r.try_get("image_url")?,
                    width: r.try_get("width")?,
                    height: r.try_get("height")?,
                    alt: r.try_get("alt")?,
                    body: r.try_get("body")?,
                    template_name: r.try_get("template_name")?,
                    template_source: r.try_get("template_source")?,
                    values: values.unwrap_or_else(|| serde_json::json!({})),
                    click_through_url: r.try_get("click_through_url")?,
                },
            );
        }
    }

    tx.commit().await?;

    Ok(Snapshot {
        config_version,
        projects: projects
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snap(v: i64) -> Snapshot {
        Snapshot {
            config_version: v,
            projects: HashMap::new(),
        }
    }

    #[test]
    fn store_atomic_swap_visible_to_readers() {
        let s = SnapshotStore::new(make_snap(1));
        assert_eq!(s.read().config_version, 1);
        s.swap(make_snap(2));
        assert_eq!(s.read().config_version, 2);
    }

    #[test]
    fn store_read_holds_consistent_view_across_swaps() {
        // A reader holding an Arc<Snapshot> from before a swap
        // continues to see the old version (no torn reads).
        let s = SnapshotStore::new(make_snap(1));
        let pre = s.read();
        s.swap(make_snap(2));
        assert_eq!(pre.config_version, 1);
        assert_eq!(s.read().config_version, 2);
    }

    #[tokio::test]
    async fn store_signals_bumped_on_swap() {
        let s = SnapshotStore::new(make_snap(1));
        let s2 = s.clone();
        let waiter = tokio::spawn(async move {
            s2.bumped.notified().await;
            s2.read().config_version
        });
        // Yield once so the waiter gets to register.
        tokio::task::yield_now().await;
        s.swap(make_snap(7));
        let v = waiter.await.unwrap();
        assert_eq!(v, 7);
    }

    #[test]
    fn project_snapshot_default_is_empty() {
        let p = ProjectSnapshot::default();
        assert!(p.flights.is_empty());
        assert!(p.ads.is_empty());
        assert!(p.sites.is_empty());
        assert!(p.zones.is_empty());
        assert!(p.hmac_secret.is_empty());
        assert!(p.hmac_secret_previous.is_none());
        assert!(p.org_id_for_event.is_empty());
        assert!(p.click_through_urls.is_empty());
        assert!(p.creatives.is_empty());
        assert!(!p.allow_force_decision);
    }
}
