# Knievel marketing-site source

Static source for a possible `knievel-ads/knievel-ads.github.io` organization
site. It lives here so public claims can be reviewed with the server. There is
no deployment/synchronization workflow in this repository; editing these files
does not prove the external Pages site was updated.

## Files

```text
web/marketing/
├── index.html   # static landing page
├── api.html     # Redoc shell loading generated OpenAPI from main
├── style.css    # local styles
└── README.md
```

There is no build step. Preview locally:

```sh
cd web/marketing
python3 -m http.server 4000
```

Then open `http://localhost:4000`. The Redoc page still needs network access to
fetch the spec and CDN script.

## Contract and freshness rules

- `index.html` must agree with the current
  [`README.md`](../../README.md), [`ARCHITECTURE.md`](../../ARCHITECTURE.md),
  [`DEPLOYMENT.md`](../../DEPLOYMENT.md), and source limitations.
- Do not advertise persistent image storage, OTel/Sentry export, `/metrics`,
  notification-triggered snapshots, COPY ingestion, or an operation absent from
  the canonical [API table](../../API.md).
- Do not hard-code a release number. Link to Releases or describe the exact tag
  and commit selected by the consumer.
- `api.html` intentionally shows generated `openapi.yaml` from `main`, which may
  be unreleased. A consumer requiring stability should use the spec from its
  deployed release tag or commit.
- When current public/operator docs change the elevator pitch, limits,
  quickstart, or security boundary, review `index.html` in the same PR.

## Publishing manually

If the organization Pages repository is configured to deploy its root `main`
branch, copy the three served files there and review the resulting diff:

```sh
cp /path/to/knievel/web/marketing/{index.html,api.html,style.css} .
git add index.html api.html style.css
git commit -m "docs: refresh Knievel site"
git push origin main
```

Repository policy and authorization for that separate push are owned by the
Pages repository; this document does not grant them.

## Third-party runtime dependency

`index.html` is fully static. `api.html` loads Redoc from its CDN and the OpenAPI
spec from GitHub raw at runtime. A CDN or GitHub outage can break that page while
the landing page remains available. Pinning or vendoring Redoc would be a
separate supply-chain decision.
