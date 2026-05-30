//! Server-side Liquid creative rendering (Phase 6.8).
//!
//! A `templated` creative carries a Liquid source (on `creative_templates`)
//! and a `values` map (on the creative). At decision time the source is
//! rendered against `values` plus a small injected context — the ad's
//! signed click/impression URLs, the placement id, and the snapshot
//! version — and the resulting HTML is returned in the decision
//! response's `creative.body`. Mirrors Kevel's Ad Template semantics so
//! RX migrations can lift their template source verbatim (`API.md` § 3.6).
//!
//! Security: Liquid is sandboxed. `with_stdlib()` registers only the
//! standard filters/tags, and we never register partials, so there is no
//! `include`/filesystem/network reach. `values` is authored by
//! authenticated project editors (and validated against the template's
//! JSON Schema at write time), so it is trusted at the same level as a
//! `kind=html` creative's verbatim `body`; output is intentionally NOT
//! auto-escaped, matching the spec's example templates which inject
//! `values.*` into HTML attributes raw. A size cap bounds runaway output.

use anyhow::{Context, Result};

/// Cap on rendered output bytes — a coarse guard against a runaway
/// template. Per-template render-time caps are a follow-up.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// The injected decision context available to creative templates.
#[derive(Clone, Copy)]
pub struct RenderCtx<'a> {
    pub ad_id: i64,
    pub click_url: &'a str,
    pub impression_url: &'a str,
    pub placement_id: &'a str,
    pub snapshot_version: i64,
}

/// A Liquid parser with only the standard library — no partials, so no
/// `include`/filesystem/network tags are reachable. Mirrors the
/// parse-on-write parser in `creative_templates.rs`.
pub fn parser() -> liquid::Parser {
    liquid::ParserBuilder::with_stdlib()
        .build()
        .expect("liquid stdlib parser construction")
}

/// Render a `templated` creative's Liquid `source` against `values` plus
/// the injected decision context. Returns the rendered HTML, or an error
/// on parse/render failure or output exceeding [`MAX_OUTPUT_BYTES`].
pub fn render_templated(source: &str, values: &serde_json::Value, ctx: RenderCtx<'_>) -> Result<String> {
    let globals = liquid::to_object(&serde_json::json!({
        "values": values,
        "ad": {
            "id": ctx.ad_id,
            "click_url": ctx.click_url,
            "impression_url": ctx.impression_url,
        },
        "placement": { "id": ctx.placement_id },
        "decision": { "snapshot_version": ctx.snapshot_version },
    }))
    .context("building liquid globals")?;

    let template = parser().parse(source).context("parsing liquid template")?;
    let out = template.render(&globals).context("rendering liquid template")?;

    if out.len() > MAX_OUTPUT_BYTES {
        anyhow::bail!(
            "rendered creative exceeds {} bytes ({} rendered)",
            MAX_OUTPUT_BYTES,
            out.len()
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RenderCtx<'static> {
        RenderCtx {
            ad_id: 7,
            click_url: "/e/c/signed",
            impression_url: "/e/i/signed",
            placement_id: "main",
            snapshot_version: 3,
        }
    }

    #[test]
    fn renders_values_and_injected_context() {
        let src = r#"<a href="{{ ad.click_url }}" data-imp="{{ ad.impression_url }}">{{ values.title }}</a>"#;
        let values = serde_json::json!({ "title": "Buy now" });
        let out = render_templated(src, &values, ctx()).unwrap();
        assert_eq!(
            out,
            r#"<a href="/e/c/signed" data-imp="/e/i/signed">Buy now</a>"#
        );
    }

    #[test]
    fn undefined_value_surfaces_as_error() {
        // Liquid 0.26 is strict on undefined access. A creative's `values`
        // is synced paired with its template (schema-validated), so this
        // only fires on a genuine template/value mismatch — which the
        // decision handler treats as no-fill for that ad rather than
        // failing the request.
        assert!(render_templated("[{{ values.missing }}]", &serde_json::json!({}), ctx()).is_err());
    }

    #[test]
    fn parse_error_is_surfaced() {
        assert!(render_templated("{% if %}", &serde_json::json!({}), ctx()).is_err());
    }

    #[test]
    fn output_over_cap_errors() {
        // A loop that blows past the byte cap.
        let src = "{% for i in (1..200000) %}xxxx{% endfor %}";
        let err = render_templated(src, &serde_json::json!({}), ctx());
        assert!(err.is_err(), "expected output-cap error");
    }
}
