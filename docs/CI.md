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
| `pr-check.yml` | PR → `main` (`crates/**`, `Cargo.toml`, `Cargo.lock`, `.cargo/**`, `.github/**`, `docs/**`, `*.md`) | `fmt` / `clippy` / `test` / release build on three platforms, a release-profile clippy pass, `cargo check` for four cross targets, actionlint, and the labelled auto-merge |
| `build-binaries.yml` | called by `publish.yml` and `nightly.yml` via `workflow_call` | release binaries for six platforms: build, optional UPX, smoke test, package, `.sha256`, upload artifact |
| `publish.yml` | manual dispatch: a version bump and channel, or an existing `tag` to re-publish | bump the version on a temporary `release` branch → check it → call `build-binaries` → create a draft release at the release branch head, upload, publish → fast-forward `main` → delete the branch |
| `nightly.yml` | every day at 04:00 UTC, or manual dispatch | build `main`, replace the `nightly` pre-release wholesale |
| `warm-cache.yml` | Cargo file changes on `main`, Monday 05:00 UTC, manual dispatch | write the cargo registry and target caches ahead of time |
| `cleanup-old-caches.yml` | Monday 00:00 UTC, or manual dispatch (`dry_run`) | keep only the newest entry of each cache family |
| `delete-merged-branch.yml` | a pull request is closed after being merged | delete the head branch, unless it is long-lived, from a fork, or still used by an open pull request |

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

`delete-merged-branch.yml` runs when a pull request is closed **and merged** — the
`automerge` label and a person pressing "Merge" are the same event — and deletes the head
branch. It keeps a branch when:

- it is a long-lived name (`main`, `master`, `develop`, `release/*`, `release-*`): a pull
  request that happened to use one of those as its head is not an argument for deleting it;
- the pull request came from a fork: the branch lives in another repository, which this
  token cannot reach;
- another open pull request still uses it as its head — a stacked pair looks exactly like
  that, and deleting the ref would close the child pull request without a decision.

A ref that is already gone counts as success: with the repository's own "Automatically
delete head branches" setting enabled, this workflow is a second opinion rather than the
mechanism. Every run writes what it did to the job summary, so a branch that survived has
its reason printed next to it.

## Caches

Shape of the cache keys (`<cachekey>` is the rustc version hash reported by
dtolnay/rust-toolchain):

```
<os>-cargo-registry-<cachekey>-<hash(Cargo.lock)>
<os>-rust-debug-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<target>-<cachekey>-<hash(Cargo.lock)>
<os>-rust-cross-<target>-<cachekey>-<hash(Cargo.lock)>
```

The first three are host-triple and are written by `warm-cache.yml`; that is what keeps
pull requests fast. The last two are per-target and are written by the jobs that read
them — `build-binaries` (every nightly, and the release) and `cross-check`. They are
worth it because nightly builds all six targets every day: the first run after a
`Cargo.lock` change pays for the upload, every one after it restores.

A few rules, each of which was written down after being learned the hard way:

- **The toolchain stays in `restore-keys`.** cargo's fingerprints embed the rustc version,
  so after a compiler change none of the old artifacts can be used — restoring them is a few
  hundred megabytes of nothing. One honest cold build after a `stable` update is better.
- **Registry and target are cached separately.** When the dependencies did not change, a
  target cache hit also means `cargo fetch` need not touch the network.
- **`CARGO_INCREMENTAL=0`.** Incremental data is not part of the cached paths; leaving it on
  only makes the restored directory bigger and prints a "could not be used" warning.
- **Every cache step either saves or shares.** `warm-cache.yml` uses `actions/cache`
  (read + write) for the host-triple families, and `build-binaries` and `cross-check`
  use it for their own per-target keys. Nothing uses `actions/cache/restore` alone: a
  restore-only step is a cache that can never hit, and two of them sat in these
  workflows described as if they worked.
- GitHub caps each repository at 10 GB of caches and evicts **silently** by LRU when that is
  exceeded, usually right before the pull request that needs the cache.
  `cleanup-old-caches.yml` groups keys by "everything before the last `-`" (exactly the
  `restore-keys` prefix) and keeps only the newest entry of each family.

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
- **It does not warm the release builds from `warm-cache.yml`**, because those caches
  are per-target (`target/<triple>/release/`) and the builds that use them write them.
  `nightly` builds all six targets daily, so nightly *is* the warmer; a release, which
  bumps `Cargo.lock` and therefore misses the exact key, restores through
  `restore-keys` and recompiles only the workspace crates. The cross-check keeps its
  own debug caches for the same reason.
