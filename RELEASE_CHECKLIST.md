# Release Checklist

Use this checklist in the `release/vX.Y.Z` preparation PR. It is operator
review material; no workflow parses PR checkboxes today.

> **Current repository prerequisite:** as of 2026-08-20 the repository has no
> branch protection or ruleset. Do not create a release tag until owners add a
> ruleset that protects `main`, blocks force pushes, requires the permanently
> named `CI result` check, and restricts creation/update/deletion of `v*` tags.
> In-workflow ancestry checks cannot make an attacker-supplied tag workflow
> trustworthy.

## Release `vX.Y.Z`

**Releaser:** _@handle_

**Previous tag:** `vA.B.C`

**Preparation branch:** `release/vX.Y.Z`

### Repository controls and source

- [ ] From a clean, current local `main`,
      `cargo xtask release-tag X.Y.Z --commit` created `release/vX.Y.Z`,
      updated/regenerated the release files,
      and created no tag or remote push.
- [ ] Protected `main` and restricted `v*` tag rules are active and verified by
      an owner.
- [ ] The preparation PR targets `main`, has the required reviews, and `CI
      result` is successful at the exact merge candidate.
- [ ] The release commit was merged through the PR; it was not pushed directly.
- [ ] The intended tag SHA is reachable from current `origin/main`.
- [ ] No canonical release tag at or above `vX.Y.Z` already exists.

### Machine-verifiable preflight

- [ ] The tag is exactly `vMAJOR.MINOR.PATCH` with no leading zeros or suffix.
- [ ] `Cargo.toml` workspace version, every local source-free package in
      `Cargo.lock`, and `openapi.yaml` `info.version` equal `X.Y.Z`.
- [ ] `CHANGELOG.md` has a dated `[X.Y.Z]` heading, a canonical release link,
      and `[Unreleased]` compares from `vX.Y.Z`.
- [ ] Workspace and package license metadata is `MIT` and a complete top-level
      `LICENSE` exists. This is mandatory: CLI packaging does not tolerate a
      missing README or LICENSE.
- [ ] From a complete clone of merged `main`, create the local annotated tag and
      run `cargo xtask release-preflight vX.Y.Z` before pushing it.

### Tests and review

- [ ] `CI result` covers formatting/clippy, unit, Postgres 16 integration/API,
      xtask linters, OpenAPI, docs, UI, Helm, gem/image smoke, four acceptance
      shards, Rust 1.94.1, and the non-publishing Apple Silicon smoke.
- [ ] The observed acceptance inventory is understood: seven active tests and
      23 ignored scenarios. Green CI does not imply those ignored bodies ran.
- [ ] The nine `chaos_*` files remain deferred skeletons; nightly claims no
      chaos coverage.
- [ ] Auth, tenant/RLS, migration, logging/PII, and compatibility changes since
      `vA.B.C` received explicit human review.
- [ ] Changelog entries accurately describe user-facing changes and any
      deprecation/sunset.

### Expected artifacts

- [ ] Multi-architecture image (`linux/amd64`, `linux/arm64`) is available under
      the canonical raw alias `ghcr.io/knievel-ads/knievel:vX.Y.Z` and the
      compatibility aliases `:X.Y.Z`, `:X.Y`, and `:X` (plus commit alias).
- [ ] The merged image digest has a Cosign signature and GitHub artifact
      attestation.
- [ ] Exactly three native CLI archives are attached:
  - `x86_64-unknown-linux-musl`
  - `aarch64-unknown-linux-musl`
  - `aarch64-apple-darwin` (minimum macOS 11.0)
- [ ] Each CLI archive executes with exact output `knievel-cli X.Y.Z`, contains
      only `knievel-cli`, `README.md`, and `LICENSE`, and has SHA-256 and Cosign
      sidecars.
- [ ] Helm chart `X.Y.Z` is present, signed, and pulls successfully.
- [ ] `knievel-ruby/main` and downstream `vX.Y.Z` advanced atomically; the gem
      was generated, built, and load-checked before the scoped App token was
      minted.
- [ ] Downstream RubyGems publication succeeded and a clean
      `gem install knievel -v X.Y.Z` loads.

### Final warning

- [ ] I understand a release rerun is **not idempotent**. Image/GitHub/Helm
      effects may already exist, and downstream tag reuse fails closed. Never
      move or force-update either release tag; follow `RELEASE_PLAYBOOK.md`.

**Sign-off:** _@handle, date_

**Second reviewer for auth/RLS changes (or N/A):** _@handle, date_
