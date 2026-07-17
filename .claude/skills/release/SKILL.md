---
name: release
description: Cut a txcript release — preflight the workspace, bump versions, tag, watch the publish-crates workflow, and verify on crates.io. Use when asked to release, publish, or ship a new version of txcript.
argument-hint: [version]
---

# Release txcript

Publishes the `txcript` library to crates.io via the tag-triggered
`publish-crates` workflow. Three releases (v0.1.0–v0.3.0) established this
procedure; follow it in order. A crates.io version is **permanent** — it can be
yanked but never deleted or reused — so every gate runs before the tag exists.

## Inputs

`$ARGUMENTS`: optional target version. If absent, propose one from the diff
since the last tag: breaking API change on 0.x → minor bump, otherwise patch.
Confirm the version with the user before tagging — this is the one
irreversible decision.

## Preflight (before any version edit)

1. Working tree clean, on `main`, up to date with origin. Uncommitted files
   fail `cargo publish --locked`.
2. `cargo test --workspace` — bare `cargo test` skips the CLI member.
3. `cargo clippy --workspace --all-targets` — pedantic baseline,
   `unwrap/expect/panic` denied in `src/`.
4. `cargo test --no-default-features` — the featureless build is a supported
   surface and has broken independently of the default build before.
5. `cargo publish --dry-run --locked -p txcript` — catches packaging errors
   (missing metadata, dirty files) that the workflow would only surface after
   the tag is pushed.

## Bump and tag

1. Set the new version in **both** `Cargo.toml` (root `txcript`) and
   `cli/Cargo.toml` (`txcript-cli`) — the CLI is `publish = false` but its
   version tracks the library. `cargo check` once so `Cargo.lock` picks up the
   bump; commit the lockfile with the manifests.
2. Commit the bump, push, and confirm CI is green on that commit before
   tagging.
3. Annotated tag matching the manifest exactly:
   `git tag -a v<X.Y.Z> -m "v<X.Y.Z>" && git push origin v<X.Y.Z>`.
   The workflow's first step compares `${GITHUB_REF_NAME#v}` against the
   manifest and hard-fails on mismatch.

## Watch and verify

1. Watch the `publish-crates` run for the tag (`gh run watch` in the
   background). It publishes with `cargo publish --locked -p txcript` — only
   the library ships.
2. Verify the version is live: `cargo search txcript` or fetch
   `https://crates.io/api/v1/crates/txcript` and check `max_version`.
3. **Known failure**: the `publish-npm` workflow also fires on every `v*` tag
   and the npm path is unresolved (`package.json` is stale — it was still
   0.2.0 at v0.3.0). Expect that run to fail or publish nothing; report its
   status to the user and ask whether to resolve, retrigger, or keep ignoring
   it. Do not silently swallow it.

## Report

State the published version, the workflow run URL, the crates.io
verification result, and the npm workflow outcome.
