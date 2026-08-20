# Release Playbook

Operational recovery for `.github/workflows/release.yml`. Read
`RELEASE_CHECKLIST.md` before creating a tag.

## Safety model

A `v*` push starts one serialized workflow with several irreversible external
effects: GHCR image aliases, image/chart signatures and attestations, a GitHub
Release, and an atomic `knievel-ruby/main` + tag push that can trigger RubyGems.
The workflow is intentionally **not generally idempotent**. A rerun may meet an
existing Release or tag and fail; it must never overwrite a published tag.

The read-only preflight precedes all builds and publishers. It reduces mistakes,
but protected `main` and restricted `v*` tag rules are the real trust boundary.
If those owner controls are absent, stop rather than release.

## 1. Triage before retrying

1. Freeze additional releases; the global `release` concurrency group prevents
   workflow overlap but does not replace communication.
2. Inspect each job and record which external effects occurred:

   ```sh
   gh run view <run-id> --log
   gh run view <run-id> --json jobs,url,headSha,headBranch
   ```

3. Check immutable/public state directly: GHCR digests and aliases, GitHub
   Releases/assets, Helm OCI artifact, `knievel-ruby/main` and `vX.Y.Z`, and
   RubyGems.
4. Do not assume a red job made no side effect; a network response can be lost
   after a server accepted a write.

## 2. Failure classes

### Preflight failed

No workflow publisher was eligible to start. Fix metadata through a new
`release/vX.Y.Z` PR. If the rejected tag was already pushed, do not move it;
choose the next version after the corrected commit is on protected `main`.

A missing or malformed top-level MIT `LICENSE`, off-main SHA, non-monotonic
version, or Cargo/OpenAPI/changelog mismatch is a hard failure, not a waiver.

### Build failed before upload/push

Fix through a PR and cut the next version. A tag identifies one immutable source
commit; do not rebuild different source under the same tag.

### Per-architecture image digest exists, manifest job failed

The untagged digest may remain in GHCR. If no public alias or later side effect
exists, `gh run rerun <run-id> --failed` can retry the failed DAG. Confirm the
rerun still uses the same tag SHA and review logs for alias/signature conflicts.

### GitHub Release or Helm failed after image publication

The image is already consumable. Prefer a patch release rather than pretending
the first run was atomic. If retrying a transient failure, use only:

```sh
gh run rerun <run-id> --failed
```

There is no `workflow_dispatch` trigger. Before retrying, determine whether the
failed job partially created its target; GitHub Release and downstream tag
creation can reject duplicates.

### Downstream Ruby job failed

Generation, bundle install, gem build, and load check occur before any App token
is minted. The final push advances `knievel-ruby/main` and `vX.Y.Z` atomically,
so Git cannot leave only one of those refs updated.

- If neither ref moved, a failed-job rerun may be safe after fixing only an
  external/transient cause.
- If downstream `vX.Y.Z` exists, the workflow intentionally refuses reuse. Do
  not delete or move it to make a rerun pass. Verify RubyGems and cut a patch if
  correction is needed.
- If RubyGems publication failed after the downstream tag triggered it, recover
  in the downstream repository according to its trusted publish procedure and
  record the manual action on the release.

## 3. Bad release: roll forward

Never force-update `vX.Y.Z`, repoint its canonical image alias to different
bytes, or replace a RubyGems version. Cut `vX.Y.(Z+1)` with the fix and mark the
bad GitHub Release prominently.

RubyGems can be yanked (existing installs remain):

```sh
gem yank knievel -v X.Y.Z
```

Treat a yank as an incident action and document it in the next changelog.

## 4. Operator rollback

Prefer a previously recorded immutable image digest. Tags below are shown only
for readability.

```sh
# Compose
KNIEVEL_IMAGE=ghcr.io/knievel-ads/knievel:v<previous-good> \
  docker compose -f examples/compose/compose.yaml up -d

# Helm chart and application image
helm upgrade knievel oci://ghcr.io/knievel-ads/charts/knievel \
  --version <previous-good> \
  --set image.repository=ghcr.io/knievel-ads/knievel \
  --set image.tag=v<previous-good>
```

Validate migration compatibility before rollback; additive schema policy does
not prove an old binary understands every newer migration. Rotate credentials
if the incident may have exposed HMAC secrets, bearer tokens, App credentials,
or signing material.

## 5. Incident record

For any consumer-visible failure:

1. annotate the bad GitHub Release without deleting history;
2. link the failed run and exact tag SHA;
3. list every observed image/chart/gem digest or version;
4. ship a patch and name the bad version in `CHANGELOG.md`; and
5. record any manual publish, yank, or rollback with reviewer approval.

Forbidden recovery shortcuts: direct commits to `main`, force-pushed release
tags, deleting a downstream tag to permit reuse, and in-place image repair.
