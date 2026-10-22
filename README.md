<p align='center'>
<img src='docs/pacor.svg' width='64' />
<br/>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
<a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust"></a>
<br/>
<span>English</span> | <a href="README.zh-CN.md">中文</a>
</p>

# Pacor

A cross-platform packaging tool for Flutter applications.

Pacor reads **one `pacor.toml`** describing every supported platform and turns
your Flutter build output into an installer for the target you choose:

| Target    | Output                                                  | Status        |
| --------- | ------------------------------------------------------- | ------------- |
| macOS     | `.dmg` (optional codesign + notarization) + updater `.tar.gz` | implemented   |
| Windows   | NSIS installer or per-machine MSI, plus a portable `.zip` | implemented   |
| Linux     | AppImage and/or `.deb`                                  | implemented   |
| Android   | per-ABI `.apk` and/or `.aab` (optional signing)          | implemented   |
| —         | RPM                                                     | not yet       |

> Early-stage project: features and configuration may still change.

## Installation

```bash
git clone https://github.com/pacor-rs/pacor.git
cd pacor
cargo build --release
```

The binary is written to `target/release/pacor`.

## Quick start

```bash
# 1. Create a commented pacor.toml in your Flutter project root
pacor init

# 2. Edit pacor.toml (app metadata, output dir, per-platform options)

# 3. Build for a target platform
pacor build --target linux      # formats from pacor.toml ([appimage, deb, ...])
pacor build --target windows
pacor build --target macos
pacor build --target android

# Omit --target to build for the host platform
pacor build
```

## Commands

| Command                              | Description                                              |
| ------------------------------------ | -------------------------------------------------------- |
| `pacor init [--force]`               | Write the commented configuration template               |
| `pacor build [--target OS] [--format FMT]...` | Build an installer                              |
| `pacor android`                      | Alias for `pacor build --target android`                 |
| `pacor clean [--target OS]`          | Remove build artifacts (with `--target`, only that target's) |
| `pacor cache clean [--dry-run]`      | Remove downloaded tools and templates from the cache     |
| `pacor check [--target OS]`          | Validate the project, configuration and build tools      |
| `pacor config [--full]`              | Print the effective configuration                        |

`pacor clean` touches the project; `pacor cache clean` empties the shared cache
under the user configuration directory (default icon set, build templates,
`linuxdeploy`, NSIS). Everything removed is fetched again on demand, so
`--dry-run` lists it first.

Global flags: `--config <path>` (defaults to `pacor.toml` in the project root; an
explicitly named file must exist),
`--command-output <none|normal|verbose>` (external command output),
`-l/--log-level <error|warn|info|debug>`.

All platform sections are parsed on every host, so the same `pacor.toml` works
on macOS, Linux and Windows — only the section selected by `--target` is used.
Packaging backends are always compiled, so an unsupported target fails at
runtime with a clear "tool X not found" error rather than being silently absent.

## Configuration

`pacor.toml` lives in the Flutter project root. Run `pacor init` to generate a
fully commented template, or read the reference template at
[`crates/pacor-core/src/config/template.toml`](crates/pacor-core/src/config/template.toml).
A trimmed example:

```toml
[app]
default_locale = "en-US"
url = "https://example.com"

[app.i18n.en-US]
name = "My App"
display_name = "My App"
publisher = "My Company"

[build]
flutter_build_mode = "release"
output_dir = "dist"

[platforms.linux]
formats = ["appimage", "deb"]

[platforms.windows]
installer_format = "nsis"
build_portable = true
# certificate_path = "cert.pfx"        # + PACOR_WINDOWS_CERT_PASSWORD to sign
# timestamp_url = "http://timestamp.digicert.com"

[platforms.macos]
minimum_system_version = "10.14"

[platforms.android]
target_abi = ["arm64-v8a"]
formats = ["apk"]                                  # or ["apk", "aab"] for Google Play
# flavor = "prod"
# keystore_file = "android/app/upload-keystore.jks"  # + key_alias and the two passwords
```

`flutter_build_mode` selects the Flutter build mode for the Windows, macOS and
Linux backends and the directory their bundles are read from (for example
`build/windows/x64/runner/Debug`); Android always builds a release APK.

Secrets are read from the environment first and are never written back:
`PACOR_WINDOWS_CERT_PASSWORD`, `PACOR_ANDROID_KEYSTORE_PASSWORD`,
`PACOR_ANDROID_KEY_PASSWORD`. See
[docs/ENVIRONMENT_VARIABLES.md](docs/ENVIRONMENT_VARIABLES.md) for the GitHub
mirror variables, and [docs/ANDROID.md](docs/ANDROID.md) for the APK signing
configuration. `PACOR_OUTPUT_DIR` and `PACOR_BUILD_MODE` override the
corresponding `[build]` keys.

## Windows packages

`pacor build --target windows` writes an NSIS installer and, unless
`build_portable = false`, a portable ZIP into the output directory, each with a
`.sha1` file next to it:

```
dist/MyApp-1.0.0+1-x64-setup.exe
dist/MyApp-1.0.0+1-x64-portable.zip
```

The installer is per-user (`%LOCALAPPDATA%`) and needs no administrator rights;
it offers desktop and Start Menu shortcuts, refuses to install or uninstall while
the application is running, and is localized from `[app.i18n]`. The portable
archive contains everything below a single `<name>-<version>-<arch>/` directory,
so extracting it cannot scatter the application.

Code signing is driven by `platforms.windows` and signs the application
executable *before* the installer is compiled, because NSIS embeds the file
contents — signing afterwards would leave the installed copy unsigned. The
installer is signed as well, and `timestamp_url` keeps both signatures valid past
the certificate's expiry. `signtool.exe` is looked up in the configured path,
then `PATH`, then the installed Windows SDKs.

`makensis` from `PATH` is used when its plugin directory is writable; otherwise
pacor downloads a pinned NSIS build into
`%APPDATA%\pacor\windows\nsis\<template tag>` alongside the templates it renders.
`installer_format` picks between them: `nsis` installs per user (below
`%LOCALAPPDATA%`, no administrator rights), `msi` installs per machine (below
`%ProgramFiles%`, needs them) and produces one package per configured locale,
because `light` cannot merge localizations that define the same identifiers.
[`docs/MSI.md`](docs/MSI.md) records the toolchain, the template contract and the
constraints the implementation is built around.

[`docs/WINDOWS.md`](docs/WINDOWS.md) documents the artifacts, the signing
options, the cache layout and the failures that layout explains.

## Android packages

`pacor build --target android` (or `pacor android`) builds one release APK per
configured ABI in a single Flutter invocation and writes them, each with a
`.sha1` next to it, into the output directory:

```
dist/MyApp-1.0.0+1-arm64-v8a.apk
```

An Android App Bundle — the format Google Play publishes from — is opt-in,
because it cannot be installed directly and a project that only ships APKs should
not pay for building one:

```toml
[platforms.android]
formats = ["apk", "aab"]     # default is ["apk"]
```

which adds `dist/MyApp-1.0.0+1.aab` from `flutter build appbundle` in the same
run. `target_abi` selects the ABIs (`armeabi-v7a`, `arm64-v8a`, `x86_64`; at least
one) — for the bundle it decides which ABIs that single file contains — and
`flavor` is passed through to `flutter build … --flavor`.

Signing is driven by `platforms.android`: setting `keystore_file` enables it for
every configured format, and `key_alias` plus both passwords
(`PACOR_ANDROID_KEYSTORE_PASSWORD`, `PACOR_ANDROID_KEY_PASSWORD`) are then
required, so a misconfigured keystore fails before the build rather than shipping
an unsigned artifact. APKs go through `apksigner` (Android SDK build-tools), App
Bundles through `jarsigner` (a JDK); each is looked up in its configured path,
then `PATH`, then its usual home. A bundle is stripped of the signature Gradle
produced before pacor signs it, so the published bundle carries exactly one
signer — `jarsigner` adds signatures rather than replacing them, and Flutter's own
template signs release builds with the **debug** certificate.

Without a configured keystore the artifact keeps whatever signature Gradle
produced, and pacor inspects it and warns when it carries that debug certificate.
Flutter's Dart AOT symbols are split into `tmp/` so they stay out of the
artifacts, and they are deleted by the cleanup step — pass `--skip-cleanup` to
keep them for `flutter symbolize`.

Android always builds release (the build mode is part of the file name) and does
not use `[app.i18n]`. [`docs/ANDROID.md`](docs/ANDROID.md) documents the
configuration, both signing toolchains, the known limitations and the failures
the implementation explains.

## Project layout

Pacor is a Cargo workspace. Each crate has a single responsibility:

```
crates/
├── pacor-core/                 # config schema, domain model, shared helpers
├── pacor-packaging/            # target registry (`packager_for`)
├── pacor-packaging-linux/      # AppImage, DEB, icons, desktop entry
├── pacor-packaging-macos/      # DMG, plist, localization, signing, notarization
├── pacor-packaging-windows/    # NSIS, MSI, portable ZIP, signtool signing
├── pacor-packaging-android/    # APK/AAB assembly, ABIs, signing
└── pacor-cli/                  # clap arguments, dispatch, output, binary `pacor`
```

Dependencies flow in one direction: `pacor-core` ← `pacor-packaging-*` ←
`pacor-packaging` ← `pacor-cli`.

Inside the Windows backend: `nsis/` owns the installer (toolchain resolution,
template provisioning, script rendering), `signing.rs` the Authenticode step,
`flutter.rs` the `flutter build` invocation and `runner.rs` everything that is
read back out of the generated runner project.

### Build templates

Build templates and the fallback icon set live in a separate repository,
[`pacor-rs/tools`](https://github.com/pacor-rs/tools). They are fetched from a
**tag**, never a branch: `pacor-core/src/tools.rs` pins the tag this build is
written against, and every backend derives its download URLs from it.

That keeps the templates and the code that renders them in lockstep — a template
change cannot reach a pacor build that does not understand it. It also means the
tag is part of each cache path (`<tools>/deb/1.1`, `<tools>/icons/1.1`, …), so
bumping the tag is all it takes to invalidate stale copies; tag contents are
immutable, so nothing else is required.

Tags are GNU-style version numbers (`MAJOR.MINOR`, optionally with a patch level)
rather than the `v`-prefixed form: a backward-compatible template addition is a
minor bump, an incompatible one a major bump. `v1` predates the convention and
holds the tree that `1.0` would name.

When a template change is needed:

1. change the templates in `pacor-rs/tools` and create a new tag, updating the
   tag message with the contract it describes (tags are never moved);
2. bump `TEMPLATE_REF` in `crates/pacor-core/src/tools.rs`.

Templates that use a variable pacor does not provide fail the build rather than
baking the placeholder into the generated source, so a mismatch is loud.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Opt-in end-to-end check: compiles the rendered NSIS script with a real
# `makensis` (needs a prepared toolchain directory, see docs/WINDOWS.md)
cargo test -p pacor-packaging-windows -- --ignored
```

Because every backend is compiled on every host, the checks above on Linux
already type-check the macOS and Windows backends. There are no C dependencies
(compression uses the pure-Rust `zlib-rs` backend), so the other targets can be
cross-checked without any cross toolchain:

```bash
rustup target add x86_64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin
cargo check --workspace --target x86_64-pc-windows-msvc
cargo check --workspace --target x86_64-apple-darwin
cargo check --workspace --target aarch64-apple-darwin
```

## Continuous integration

Eight workflows under `.github/workflows` cover the checks, the release, cache
housekeeping and branch cleanup; **[docs/CI.md](docs/CI.md)** describes each of them, the
release procedure and the caching scheme.

In short: a pull request runs `fmt`, `clippy`, the test suite and a release build on
Linux, macOS and Windows, plus a release-profile lint pass (the `#[cfg(debug_assertions)]`
code path) and three cross-target `cargo check` runs; a `v*` tag is published with six
binaries, per-asset `.sha256` files and a combined `SHA256SUMS`; `main` is rebuilt every
day as the `nightly` pre-release. A release is one dispatch of **Publish Release**: it bumps
the version, checks and builds a temporary branch, publishes the draft, and only then
fast-forwards `main` and tags it, so the tag and the manifest cannot disagree.

## License

MIT — see [LICENSE](LICENSE).
