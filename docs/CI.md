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
| `pr-check.yml` | PR → `main` (`crates/**`, `Cargo.toml`, `Cargo.lock`, `.cargo/**`, `.github/**`, `docs/**`, `*.md`) | `fmt` / `clippy` / `test` / release build on three platforms, a release-profile clippy pass, `cargo check` for three cross targets, actionlint, and the labelled auto-merge |
| `build-binaries.yml` | called by `publish.yml` and `nightly.yml` via `workflow_call` | release binaries for six platforms: build, optional UPX, smoke test, package, `.sha256`, upload artifact |
| `publish.yml` | push of a `v*` tag, or a manual dispatch (with `tag`) | verify the tag matches the version in `Cargo.toml` → call `build-binaries` → create a draft release → upload → publish |
| `nightly.yml` | every day at 04:00 UTC, or manual dispatch | build `main`, replace the `nightly` pre-release wholesale |
| `warm-cache.yml` | Cargo file changes on `main`, Monday 05:00 UTC, manual dispatch | write the cargo registry and target caches ahead of time |
| `cleanup-old-caches.yml` | Monday 00:00 UTC, or manual dispatch (`dry_run`) | keep only the newest entry of each cache family |
| `bump.yml` | manual dispatch (`version`, optional `dry_run`) | bump the version, sync `Cargo.lock`, commit, tag, and start the release |

## Releasing

1. Run **Bump Version**: put `0.2.0` or `0.2.0-alpha.1` in `version`; a `dry_run` first is
   fine. It does four things: edit `[workspace.package] version` (every crate inherits it),
   `cargo update --workspace` to sync `Cargo.lock`, commit `chore(release): vX`, and create
   an annotated tag. The version has to be given explicitly — the repository has neither
   `npm` nor `cargo-release`, and what "`patch`" means for a `0.1.0-alpha` is a decision a
   person should make.
2. It then starts **Publish Release**. There is a trap here: **a tag pushed with
   `GITHUB_TOKEN` does not trigger other workflows** (that is GitHub's recursion guard), so
   `bump.yml` calls `gh workflow run publish.yml -f tag=vX` after pushing instead of hoping
   the tag wakes the release up. (To drive everything from the tag alone, give that step a
   PAT with `repo` scope.)
3. `verify` compares `v$version` with the tag and fails **within five seconds** if they
   disagree, instead of burning six runners first. Whether `Cargo.lock` matches the
   manifests is enforced by `--locked` at build time.
4. Six platforms build in parallel → each produces a flat archive (extracting it gives the
   binary, not the `target/<triple>/release/` directory structure) plus a matching
   `.sha256`. **UPX compression is optional and off by default**: tick `compress` when
   dispatching by hand if you want smaller downloads (it applies to Linux and Windows; a
   failed compression only prints a warning and does not block the release). It is off by
   default because it rewrites the executable — backtraces in crash reports become hard to
   read, and packed binaries trip antivirus heuristics more easily. A nightly, whose whole
   purpose is debugging, needs it least. macOS is never compressed: the Mach-O that UPX
   produces is not reliable.
5. The release is created **as a draft and published only after the upload finishes**, so a
   failure halfway cannot leave a half-populated release that "latest" points at.
6. A version with `-alpha`/`-beta`/any pre-release suffix automatically gets
   `--prerelease --latest=false` and does not take the latest slot.
7. Assets: `pacor-<Platform>.zip|tar.gz`, a matching `.sha256`, and a combined
   `SHA256SUMS` (`sha256sum -c SHA256SUMS` works directly, and it is the format
   winget/Scoop/Homebrew want anyway).

**Re-publishing the same tag**: dispatch with `tag` filled in; it rebuilds from that tag's
tree and overwrites the existing release's notes and assets (`gh release upload --clobber`).

## Nightly

- Builds `main` every day at 04:00 UTC. `nightly` is a **moving tag**: the old release and
  tag are deleted and recreated each time, so it only ever means "the most recent `main`
  that still compiles".
- When it skips: `nightly` already points at the same commit, or HEAD itself carries a `v*`
  release tag (that release already covers it). A skip writes a `GITHUB_STEP_SUMMARY` line
  saying why, so "nothing was built" cannot be misread as "everything was verified".
- Intermediate artifacts are kept for one day.

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

A few rules, each of which was written down after being learned the hard way:

- **The toolchain stays in `restore-keys`.** cargo's fingerprints embed the rustc version,
  so after a compiler change none of the old artifacts can be used — restoring them is a few
  hundred megabytes of nothing. One honest cold build after a `stable` update is better.
- **Registry and target are cached separately.** When the dependencies did not change, a
  target cache hit also means `cargo fetch` need not touch the network.
- **`CARGO_INCREMENTAL=0`.** Incremental data is not part of the cached paths; leaving it on
  only makes the restored directory bigger and prints a "could not be used" warning.
- **`warm-cache.yml` uses `actions/cache` (read + write), pull requests use
  `actions/cache/restore` (read only), and the keys are identical** — otherwise the warming
  writes a set of caches nobody reads.
- GitHub caps each repository at 10 GB of caches and evicts **silently** by LRU when that is
  exceeded, usually right before the pull request that needs the cache.
  `cleanup-old-caches.yml` groups keys by "everything before the last `-`" (exactly the
  `restore-keys` prefix) and keeps only the newest entry of each family.

## Drift protection

- **`main` takes changes only through a pull request.** Every change arrives on its own
  branch and merges once `pr-check` is green; nothing is pushed to `main` directly. The one
  exception is the release bump that `bump.yml` pushes, which only works while the bot is on
  `main`'s bypass list — otherwise it has to open a pull request too.
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
- **It does not guess a version number**: `bump.yml` requires an explicit `version`.
- **It does not test the toolchain for the Windows-arm64 cross build**:
  `aarch64-pc-windows-msvc` relies on the ARM64 MSVC toolchain that the `windows-latest` image
  ships, exactly as before this rework.
