# AGENTS.md

This file is for anyone changing code in this repository, human or automated. It records
only the rules **this repository actually enforces**: every rule can be traced to a CI
workflow, a script, or an existing commit — it is not a generic style guide.

## What this is

`pacor` is a cross-platform packaging tool for Flutter applications (a CLI). It takes a
Flutter project plus a `pacor.toml` and produces releasable artifacts for Linux
(AppImage/deb), macOS (app/dmg), Windows (NSIS/MSI and a portable ZIP) and Android
(APK/AAB).

The workspace holds seven crates (edition 2024, `rust-version = "1.99"`, `resolver = 3`):

| Crate | Responsibility |
| --- | --- |
| `pacor-cli` | argument parsing, command dispatch, user-facing output |
| `pacor-core` | configuration schema, domain model, error types, toolchain downloads, shared utilities |
| `pacor-packaging` | the registry that maps `--target` to a `BundleBuilder`, plus `--format` handling |
| `pacor-packaging-{linux,macos,windows,android}` | per-platform build and packaging |

Dependencies point one way: backends → `core`, `pacor-cli` → `pacor-packaging` → backends.
`core` deliberately knows nothing about any concrete package format, so **the configuration
for every platform parses on any host** — that is also what lets CI cross-check the Windows
and macOS backends from Linux.

## Environment and the checks that must be green

At minimum, `cargo build --workspace` must pass before you start changing things. The
pre-commit list is exactly what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings

RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-unknown-linux-gnu
```

Some of these are not ceremony:

- **The release pass is not a duplicate.** `#[cfg(debug_assertions)]` hides entire items from
  the debug build (the `trace` log level in `pacor-cli` is one), so a `dead_code` or `unused`
  warning introduced by that gate only ever appears in the release pass.
- **Cross-checking needs almost no cross toolchain.** Compression is pure Rust
  (`zlib-rs`) and the only C in the graph is `ring`, the crypto backend behind Linux's
  rustls — so the `aarch64-unknown-linux-gnu` check installs `gcc-aarch64-linux-gnu`
  and every other target needs nothing. A new dependency that needs a C toolchain or a
  system library invalidates this and is a change worth discussing on its own.
- **Every CI job names its target explicitly**, the host ones included
  (`cargo test --target x86_64-unknown-linux-gnu`). That is not about correctness — the
  command above builds the same thing — but about caches: `target/<triple>/…` is a path
  one workflow can fill in for another's job, while `target/debug` is only reachable by the
  job that runs it, and a cache written by a pull request is invisible to the next one.
- There is no `rustfmt.toml`; the default configuration (100 columns) applies. Do not
  re-wrap code by hand for looks.
- Do not habitually run a full `cargo build --release` locally: the release profile is
  `opt-level = "z"` + LTO + `codegen-units = 1`, so being slow is by design and CI absorbs
  it with caches.

## Code

- `unsafe`: production code currently has zero occurrences — keep it that way. The only
  permitted use is in tests, for `env::set_var`/`remove_var`, which edition 2024 requires to
  be `unsafe` (see `crates/pacor-packaging-android/tests/android_smoke.rs`), and it must be
  serialised — see "Tests" below.
- Errors: `thiserror` inside a crate, split by domain into `src/error/` submodules and
  re-exported from `src/error.rs`; `anyhow` at crate boundaries and in the CLI. An error
  message must explain **which path, which tool, what was expected** — never hand the user a
  bare `os error 2` that names neither the file nor the command it came from.
- No `unwrap()`/`expect()` in non-test code (two remain in the whole workspace, both
  provably infallible). Propagate with context instead.
- Every file starts with a `//!` module comment saying what the module is responsible for and
  why it is cut that way; public items get `///`.
- No magic strings: name them (e.g. `CONFIG_FILE_NAME`, `TEMPLATE_REF`,
  `ENV_GITHUB_MIRROR_BASE_URL`), and keep platform-specific strings inside their backend
  crate rather than leaking them into the CLI.
- Comments and identifiers are English. A comment explains **why**, not what the next line
  does.
- Platform-specific behaviour lives in the backend crate; `pacor-cli` contains no platform
  branches.
- Adding a platform means implementing `BundleBuilder` **and** extending
  `crates/pacor-packaging/src/lib.rs` (`packager_for`, `apply_format_overrides`) with its
  tests, plus `Platform` in `core`.

## Tests

- Unit tests live at the bottom of the file in `#[cfg(test)] mod tests`; cross-module and
  end-to-end tests live in `crates/<crate>/tests/*.rs` (currently only
  `pacor-packaging-android/tests/android_smoke.rs`).
- A test name should read as its assertion:
  `the_windows_version_is_always_four_numeric_components`,
  `sizes_are_rendered_in_binary_units`. No `test_` prefix is needed (existing ones may stay).
- **End-to-end tests must not need a real toolchain**: use a fixture project plus stub
  executables (`flutter`, `jarsigner`, `apksigner`) and assert the **complete argument
  vector** handed to them. That keeps `cargo test --workspace` runnable on any CI host, and
  it is the only defence against a silently changed flag.
- A test that does need real tooling is marked `#[ignore]` and its prerequisites are
  documented (e.g. `cargo test -p pacor-packaging-windows -- --ignored` needs a prepared
  NSIS/WiX directory, see `docs/WINDOWS.md`).
- Tests that mutate environment variables must run serially and restore state: use
  `temp-env`'s `with_vars` and serialise the test functions the way `serialised()` does in
  `android_smoke.rs` — `set_var` is `unsafe` in edition 2024 because concurrent calls are
  undefined behaviour.
- Fixing a bug starts with a failing test (a minimal fixture counts), then the fix.

## Language rules

- **Every document exists twice**: `docs/NAME.md` in English and `docs/NAME.zh-CN.md` in
  Chinese, and the same applies to `README.md` / `README.zh-CN.md`. Both start with the same
  three-line language bar (`<p align='center'>` with a link to the other language).
- **English is the pointer target**: code comments, docs cross-links, release notes and
  `README.md` all link to the English file (`docs/WINDOWS.md`), never to the Chinese one.
- **Code comments, log messages and this file are English only.** The same goes for commit
  messages, test names and identifiers.
- Chinese is allowed in exactly three places: `docs/*.zh-CN.md`, `README.zh-CN.md`, and test
  data that exists to exercise non-ASCII handling (a `zh_CN` app name in a `.desktop` test,
  a localized MSI property, a Windows error message captured verbatim). Keep such data as it
  is — translating it deletes the thing the test verifies.
- When you add a document, add both files and both language bars in the same commit, and
  link to it from the matching README.

## Documentation

- Change the documentation in the **same commit** as the behaviour. A document that
  disagrees with the code is a defect — this repository has shipped a README that claimed a
  feature the code had as a `TODO`, and that is not to happen again.
- Troubleshooting entries follow "symptom → cause → what to do", with a command that can be
  copied.
- Adding a platform or a format means touching four places: `docs/<PLATFORM>.md` (both
  languages), the matching section in both READMEs, the comments in
  `crates/pacor-core/src/config/template.toml`, and the doc link in the backend code.
- Do not translate the `docs/*.zh-CN.md` files mechanically when the English one changes:
  keep them in sync as prose, but do not let the English one drift ahead for long.

## Commit messages

Format: `<type>(<scope>): <summary>`

- `type`: `feat`, `fix`, `docs`, `refactor`, `test`, `ci`, `chore`, `perf` (the log currently
  uses `feat`, `fix`, `docs`, `ci`, `refactor`).
- `scope`: the affected area or crate — `windows`, `android`, `core`, `tools`, `downloads`,
  `build`, `ci`. Omit it when the change is broad.
- `summary`: lowercase, imperative, no trailing period, at most 72 characters.
- **The body explains why**, it does not restate the diff. An existing body is a few
  paragraphs of English prose that usually cover:
  1. what was wrong (with the output or number that was measured, when there is one);
  2. why this fix and not another;
  3. how it was verified (which real toolchain ran, which command, what result).
  Bullet lists are fine when several things are parallel, but the body is not a changelog of
  files.
- One commit does one thing. Formatting, an incidental comment fix, an unrelated rename: each
  gets its own commit.
- No `Co-authored-by` or generator signatures; no `--no-verify` to skip checks.
- Never commit `dist/`, `target/`, `tmp/`, probe projects or scratch scripts (`.gitignore`
  covers `target/`, `tmp/`, `/test_*`). Put experiments under `target/`.
- A real example to follow:

  ```text
  feat(android): package App Bundles and sign what is shipped

  The Android backend collected Flutter's APKs and got almost everything after that
  wrong. Signing was a `TODO` that logged a warning and returned `Ok`, ...

  Signing is now real and validated before Gradle runs: a configured `keystore_file`
  requires the alias and both passwords, ...

  Verified end to end against a real Flutter project (`flutter create
  --platforms=android`) here: 5.7 MB and 6.3 MB split APKs plus a 23.8 MB bundle, ...
  ```

## Branching and pull requests

`main` takes no direct change. **Nothing is committed or pushed to it** — every change
starts on its own branch and reaches `main` through a pull request whose `pr-check` is
green.

GitHub does not enforce that here: branch protection and rulesets are unavailable on a
private repository without a paid plan (`gh api
repos/{owner}/{repo}/branches/main/protection` answers 403, "Upgrade to GitHub Pro or make
this repository public"). The rule is therefore held by the people and agents working in
the repository rather than by the platform — which is all the more reason to follow it,
and to switch protection on, requiring `pr-check`, the day the repository goes public or
is upgraded.

```bash
git switch -c feat/aab-signing main
# ... work, commit ...
git push -u origin feat/aab-signing
gh pr create --base main --fill
```

- **Branch names describe the change, not the person**: `<type>/<slug>`, using the commit
  types (`feat/android-aab`, `fix/nsis-space-in-name`, `docs/english-docs`,
  `ci/upx-optional`). One branch per functional change — a branch that accumulates three
  unrelated fixes cannot be reviewed, and cannot be reverted without dragging the others
  along.
- **The pull request is the unit of review.** Its description says what changes and why, in
  the same terms as the commit body, so that the merged history stays readable on its own.
- **Keep the branch current by rebasing on `main`**, not by merging `main` into it, so the
  history stays linear and the diff stays reviewable.
- **No direct pushes, no force-pushes to `main`, no `--no-verify`.** An urgent one-line fix
  is still a pull request; with `pr-check` green it merges in under a minute, and labelling
  it `automerge` is what makes that automatic (a fork's pull request cannot merge: its token
  is read-only).
- **Agents follow this too.** Create the branch *before* the first commit rather than
  committing on `main` and moving the commits afterwards: a rebased-away commit still shows
  up in the reflog and in whoever is watching the terminal, and it means part of the work
  was written under different assumptions than the part that is reviewed.
- **The merged branch is deleted for you**: `delete-merged-branch.yml` removes the head ref of
  every merged pull request (it keeps long-lived names, forks, and a branch that another open
  pull request still uses). `gh pr merge --delete-branch` just does not wait for it.

The one commit that reaches `main` without a pull request is the release bump that
`publish.yml` fast-forwards in — after the release is public, and only when `main` has not
moved. It changes nothing but the version and the lock file, and it is tolerated because
no protection exists to bypass it; the day protection exists, either list
`github-actions[bot]` as an exception or make `publish.yml` open a pull request and tag
after that merges. A rule the repository's own automation ignores is not a rule.

## Releases and CI

- **The version lives only in `[workspace.package]` in `Cargo.toml`** (every crate uses
  `version.workspace = true`). After changing it, run `cargo update --workspace` so
  `Cargo.lock` follows, otherwise `--locked` builds fail.
- Releases are dispatched, not tagged: **Publish Release** takes a `bump_type` and a
  channel, bumps the version on a temporary `release` branch, checks and builds that
  branch, publishes a draft — which creates `vX` at that branch's head — and only then
  fast-forwards `main` onto the same commit. Never edit the version by hand, and never
  push a `v*` tag for a release — the tag is the workflow's output.
- Never move a published tag and never hand-edit release assets — re-publish by dispatching
  `publish` again.
- Workflows reference actions by **version** (`@v7`, `@v5`, `@v4`), never `@main`/`@latest`,
  and not by commit SHA either.
- After editing a workflow, run actionlint (CI's `workflow-lint` job runs it too):
  ```bash
  docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:latest -color
  ```
- `docs/CI.md` has the full picture: when each of the seven workflows runs, how the cache keys
  are built, and what the release and nightly jobs do.

## Known traps

- **CRLF.** `Cargo.toml` is CRLF in some Windows checkouts, which makes
  `sed '/^\[workspace\.package\]$/ ...'` match nothing — and the failure looks like a missing
  key. Strip it with `tr -d '\r'` before parsing and preserve the original line endings when
  writing; `publish.yml` does exactly that.
- **Symbol files.** Android's `--split-debug-info` writes into the project's `tmp/`, which
  `pacor clean` removes and `--skip-cleanup` keeps. `tmp/` is ignored by git; do not commit it.
- **Direct connections to GitHub** are reset on some networks (raw, API and release assets
  alike). The proxy and mirror recipes live in `docs/ENVIRONMENT_VARIABLES.md`; check there
  before working around it in code.
- **TLS differs by platform.** Windows and macOS use `native-tls` (Schannel,
  Security.framework); Linux uses rustls, because `native-tls` there is OpenSSL and
  `openssl-sys` makes every cross build need the target's headers. Both verify against
  the operating system's trust store, so a corporate CA installed system-wide works
  on all three — but a CA that only some tool trusts does not.
- **The platform status table** may only claim what has actually been built and run. That
  table is a release promise, not a roadmap.

## Before you commit

1. You are on a branch, not on `main`, and the branch is named for the change.
2. `cargo fmt --all -- --check`, `clippy` (debug and release) and `test --workspace` are green.
3. If behaviour changed, the documentation changed in the same commit (`docs/*.md` in both
   languages, both READMEs, the `template.toml` comments).
4. Every new or changed flag, environment variable and path is documented somewhere.
5. The commit message explains why and how it was verified, rather than listing the diff.
6. `git status` shows no `target/`, `tmp/`, probe project or scratch script.
7. New or moved documents come in pairs, and the links point at the English file.
