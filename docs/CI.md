<p align='center'>
<span>English</span> | <a href="CI.zh-CN.md">中文</a>
</p>

# CI and the release process

`.github/workflows` holds seven workflows. They share one goal: **a check that can run
locally must run on every pull request; something that cannot run locally (the binaries
for six platforms) runs only when it is really needed; and any state that "looks green
because nothing happened" has to say so out loud.**

## Overview

| File | Trigger | What it does |
| ---- | ---- | ---- |
| `pr-check.yml` | PR → `main` (`crates/**`, `Cargo.toml`, `Cargo.lock`, `.cargo/**`, `.github/**`, `docs/**`, `*.md`) | `fmt` / `clippy` / `test` / release build on three platforms, a release-profile clippy pass, `cargo check` for four cross targets, actionlint, and the labelled auto-merge with a token whose merges the branch cleanup can see |
| `build-binaries.yml` | called by `publish.yml` and `nightly.yml` via `workflow_call` | release binaries for six platforms: build, optional UPX, smoke test, package, `.sha256`, upload artifact |
| `publish.yml` | manual dispatch: a version bump and channel, or an existing `tag` to re-publish | bump the version on a temporary `release` branch → check it → call `build-binaries` → create a draft release at the release branch head, upload, publish → fast-forward `main` → delete the branch |
| `nightly.yml` | every day at 04:00 UTC, or manual dispatch | build `main`, replace the `nightly` pre-release wholesale |
| `warm-cache.yml` | Cargo file changes on `main`, Monday 05:00 UTC, manual dispatch | write the cargo registry and target caches ahead of time |
| `cleanup-old-caches.yml` | Monday 00:00 UTC, or manual dispatch (`dry_run`) | keep only the newest entry of each cache family |
| `delete-merged-branch.yml` | a merged pull request closes | delete the head branch of the merged pull request unless it is long-lived, from a fork, or still used by an open pull request |

## Releasing

1. Dispatch **Publish Release** and choose how the version moves (`patch`, `minor`, `major`,
   or `prerelease` for one more alpha/beta) and the channel (`stable`, `alpha`, `beta`).
   Filling in `tag` re-publishes an existing tag instead, without touching the version.
2. `prepare` works the version out with npm's rules — a patch on `0.1.0-alpha` is `0.1.0`,
   because that pre-release *was* the patch release — edits `[workspace.package] version`,
   runs `cargo update --workspace`, commits `chore(release): vX` and pushes it to a
   temporary `release` branch. A `release`-only branch that has to be cleaned up sounds
   worse than pushing to `main`, and it is not: nothing outside this workflow reads it, and
   `main` stays where it was until the release is public.
3. `check` runs `fmt`, `clippy` and the test suite **on that branch**, because no pull
   request reviewed it. Then six platforms build from the same ref.
4. Each platform produces a flat archive (extracting it gives the binary, not the
   `target/<triple>/release/` directory structure) plus a matching `.sha256`. **UPX
   compression is optional and off by default**: tick `compress` if you want smaller
   downloads (Linux and Windows only; a failed compression prints a warning and does not
   block the release). It is off because it rewrites the executable — backtraces become hard
   to read and packed binaries trip antivirus heuristics. macOS is never compressed: the
   Mach-O that UPX produces is not reliable.
5. The release is created **as a draft**, the assets are uploaded, and only then is it
   published. The downloads list in its notes is rendered **from the assets that actually
   exist**, with their measured sizes, so it cannot drift from the upload.
6. A pre-release channel additionally deletes the previous pre-releases: the updater only
   ever sees the newest one, and older ones on the page are invitations to install the wrong
   thing. Their tags stay, so the history is still reproducible.
7. `merge-back` fast-forwards `main` onto the bump commit — **only** after the release is
   public, and only if `main` has not moved in the meantime (a fast-forward that fails
   means someone merged while the release was building). It does not create the tag:
   the draft release did, at the head of the release branch, and `merge-back` refuses to
   push unless `vX` and the commit `main` would land on are the same one. Tagging twice
   is exactly how the first real release failed — fast-forward done, `main` unpushed,
   and a published tag pointing at the tree *before* the bump.
8. `cleanup` deletes the `release` branch on success; a failed run leaves it behind for
   whoever debugs it, and the next run force-pushes over it. A stable release also retires
   the `nightly` release and tag: people on the nightly are one command away from stable.

**Re-publishing a tag** (`tag` filled in) rebuilds from that tag's tree, replaces the assets
with `--clobber` and refreshes the notes — no bump, no merge-back, no cleanup. That is the
way out of a broken asset.

**Why dispatch and not a tag push**: the tag is now *produced* by this workflow, so a
`push: tags: v*` trigger would re-enter itself at the merge-back step.
## Nightly

- Builds `main` every day at 04:00 UTC. `nightly` is a **moving tag**: the old release and
  tag are deleted and recreated each time, so it only ever means "the most recent `main`
  that still compiles".
- When it skips: `nightly` already points at the same commit, or HEAD itself carries a `v*`
  release tag (that release already covers it). A skip writes a `GITHUB_STEP_SUMMARY` line
  saying why, so "nothing was built" cannot be misread as "everything was verified".
- Intermediate artifacts are kept for one day, and a stable release deletes the
  `nightly` release and tag (`publish.yml`'s last job): someone on the nightly is one
  command away from stable, and leaving both published keeps them on yesterday's build.

## Branch cleanup

`delete-merged-branch.yml` deletes the head branch of a merged pull request. It keeps a
branch when:

- it is a long-lived name (`main`, `master`, `develop`, `dev`, `backup`, `release/*`,
  `release-*`): a pull request that happened to use one of those as its head is not an
  argument for deleting it;
- the pull request came from a fork: the branch lives in another repository, which this
  token cannot reach;
- another open pull request still uses it as its head — a stacked pair looks exactly like
  that, and deleting the ref would close the child pull request without a decision.

The event is the only trigger. There is no schedule and no manual entry point: deleting a
branch by hand is `git push origin --delete <branch>`, and a workflow input for the branch
name would only be a slower way to run that command.

That the event exists at all is the point. GitHub does not start a workflow for the events
`GITHUB_TOKEN` causes, so a merge made with the default token emits no `pull_request.closed`
and no cleanup can run — #17 and #18 were merged that way and kept their branches. `automerge`
therefore merges with a token of its own, and the repository needs an `AUTOMERGE_TOKEN`
secret:

- a fine-grained personal access token for this repository with **Contents**, **Pull
  requests** and **Workflows** set to *read and write* (the last one is what lets the merge
  include a change to `.github/workflows/`);
- stored with `gh secret set AUTOMERGE_TOKEN --body <token>`.

A fine-grained token expires after at most a year, so it needs rotating; the merge is
attributed to its owner. Without the secret, a pull request labelled `automerge` fails its
`Auto Merge` job with a message naming the missing secret rather than merging quietly.

A ref that is already gone counts as success: with the repository's own "Automatically
delete head branches" setting enabled, this workflow is a second opinion rather than the
mechanism. Every run writes what it did to the job summary, so a branch that survived has
its reason printed next to it.

## Caches

Shape of the cache keys (`<cachekey>` is the rustc version hash reported by
dtolnay/rust-toolchain):

```
<os>-cargo-registry-<cachekey>-<hash(Cargo.lock)>
<os>-rust-debug-v<N>-<target>-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-v<N>-<target>-<cachekey>-<hash(Cargo.lock)>
```

Three families, and the target triple is in the key of every one of them — the host jobs
included, because `pr-check` passes `--target` explicitly even where it names the machine it
is running on.

`<N>` is the revision of the cached *paths*, not a run counter, and it is currently `4`.
A cache entry cannot be overwritten, so widening or narrowing the path list without a new
key leaves the old entry in place and the change silently does nothing: the restore keeps
finding the previous generation and the new files are never saved. Bump it in every
workflow that touches the family in the same commit.

Each entry carries two directories, not one. With `--target`, cargo writes the target
artifacts under `target/<triple>/…` but the build scripts and proc macros — compiled for the
host, which is why most of a cross entry is host artifacts — under the untripled
`target/debug` and `target/release`. Caching only the tripled half left about
thirty units to be rebuilt in every job of every run: a cache hit that still compiled
`libc`, `syn`, `cc` and `ring` and took the same two minutes it would have taken cold.
Both halves are in every entry, including the cross ones, even though their untripled copy
is the same on all four Linux-hosted targets: the duplication is a few tens of megabytes and
the alternative is a second restore step per job.

That `--target` is what makes one writer possible, and there has to be exactly one: a cache
written by a pull request is scoped to that pull request's merge ref, so the next pull
request reports `Cache not found` for a key that was saved minutes earlier by a run with the
same `Cargo.lock` and the same toolchain. Every job in `pr-check` is therefore a reader
(`actions/cache/restore`), and the writing happens on `main`: `warm-cache.yml` builds the
host debug and release entries for all three platforms plus the four debug entries that
`cross-check` reads, and `build-binaries` writes `rust-release-v<N>-<target>` on every nightly and
every release — also from `main`.

A few rules, each of which was written down after being learned the hard way:

- **The toolchain stays in `restore-keys`.** cargo's fingerprints embed the rustc version,
  so after a compiler change none of the old artifacts can be used — restoring them is a few
  hundred megabytes of nothing. One honest cold build after a `stable` update is better.
- **Registry and target are cached separately.** When the dependencies did not change, a
  target cache hit also means `cargo fetch` need not touch the network.
- **`CARGO_INCREMENTAL=0`.** Incremental data is not part of the cached paths; leaving it on
  only makes the restored directory bigger and prints a "could not be used" warning.
- **No `#` comments inside a `path: |` block.** Every line of that block is a path, comments
  included, and the path list is part of the cache *version*: an entry saved by a workflow
  whose list contains a comment is a different version from the one a workflow without it
  looks up, and the lookup reports `Cache not found` for a key that exists — which is how
  the host half of the cache above was silently ignored for a day. Comments go above the
  `path:` key.
- **A pull request must never be the writer.** Its cache lives under `refs/pull/<n>/merge`:
  no other pull request can restore it, and it is useless the moment the branch is deleted.
  `warm-cache.yml` is a `push` to `main` for exactly this reason, and its keys are
  byte-for-byte the ones the pull-request jobs ask for.

## Drift protection

- **`main` takes changes only through a pull request.** Every change arrives on its own
  branch and merges once `pr-check` is green; nothing is pushed to `main` directly. Branch
  protection is not available on this repository's plan, so that is held by review rather
  than by GitHub. The one exception is the release bump that `bump.yml` pushes, which
  changes nothing but the version and the lock file.
- **Every action is referenced by version** (`@v7`, `@v5.1.0`), not by commit SHA: a reviewer
  can see which version runs. A floating branch (`@main`) is not a version and is never used.
- **Every job has a `timeout-minutes`**: a hung job does not hold a runner for six hours.
- **concurrency**: pull requests cancel their predecessor (a new push invalidates the old
  result); release, cleanup and bump have a concurrency group that does *not* cancel — an
  upload or delete loop cut in half is worse than being slow.
- **workflow-lint runs actionlint** (with the runner's shellcheck, when present, checking the
  `run:` blocks; only `-S warning` and worse fail). The problems it catches: an `if:` naming
  the wrong job (the gate silently never fires and looks exactly like a pass), a `needs`
  pointing at a job that does not exist, a mistyped expression. It is installed from the
  official release and verified against the published checksums, not from an image tag.
- **Auto-merge** only happens when all of these hold: the pull request comes from a branch of
  this repository (a fork gets a read-only token), it carries the `automerge` label, and no
  check failed or was cancelled (a skipped check is acceptable).

## The same checks locally

This is what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings   # code that only exists in the release profile

RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-unknown-linux-gnu
```

A change to a workflow can be checked with actionlint locally:

```bash
docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:latest -color
```

## What this deliberately does not do

- **It does not run real packaging in CI.** `pacor build` needs the Flutter SDK, Android also
  needs the SDK + JDK, and Windows needs the NSIS/WiX toolchain — putting those in CI would
  turn every pull request into tens of minutes. So
  `cargo test -p pacor-packaging-windows -- --ignored` (the NSIS end-to-end test that needs a
  prepared `makensis`) still runs only locally, on demand; see `docs/WINDOWS.md`.
- **It does not sign or notarize pacor itself.** What is published is a CLI binary;
  `codesign`/`notarytool` are what pacor does to a user's artifacts (`--target macos`), which
  has nothing to do with building pacor.
- **It does not accept a hand-written version**: the bump is computed from
  `bump_type` × `release_channel`, so the three places a version lives (the manifest,
  the lock file, the tag) can never disagree.
- **It does not test the toolchain for the Windows-arm64 cross build**:
  `aarch64-pc-windows-msvc` relies on the ARM64 MSVC toolchain that the `windows-latest` image
  ships, exactly as before this rework.
- **It does not warm the cross-target release builds.** `warm-cache.yml` warms the host
  release entries and the four cross-target *debug* ones; the release binaries for all six
  targets come from `nightly`, which fills `rust-release-<target>` every day from `main`.
  Warming them a second time would be the same six builds twice.
