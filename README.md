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
| Windows   | NSIS installer + portable `.zip`                        | implemented   |
| Linux     | AppImage and/or `.deb`                                  | implemented   |
| Android   | per-ABI `.apk`                                          | implemented   |
| —         | MSI, RPM                                                | not yet       |

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
| `pacor clean [--target OS]`          | Remove the output directory and Flutter build artifacts  |
| `pacor cache clean [--dry-run]`      | Remove downloaded tools and templates from the cache     |
| `pacor check [--target OS]`          | Validate the project, configuration and build tools      |
| `pacor config [--full]`              | Print the effective configuration                        |

`pacor clean` touches the project; `pacor cache clean` empties the shared cache
under the user configuration directory (default icon set, build templates,
`linuxdeploy`, NSIS). Everything removed is fetched again on demand, so
`--dry-run` lists it first.

Global flags: `--config <path>` (defaults to `pacor.toml` in the project root),
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

[platforms.macos]
minimum_system_version = "10.14"

[platforms.android]
target_abi = ["arm64-v8a"]
```

Secrets are read from the environment first and are never written back:
`PACOR_WINDOWS_CERT_PASSWORD`, `PACOR_ANDROID_KEYSTORE_PASSWORD`,
`PACOR_ANDROID_KEY_PASSWORD`. See
[docs/ENVIRONMENT_VARIABLES.md](docs/ENVIRONMENT_VARIABLES.md) for the GitHub
mirror variables. `PACOR_OUTPUT_DIR` and `PACOR_BUILD_MODE` override the
corresponding `[build]` keys.

## Project layout

Pacor is a Cargo workspace. Each crate has a single responsibility:

```
crates/
├── pacor-core/                 # config schema, domain model, shared helpers
├── pacor-packaging/            # target registry (`packager_for`)
├── pacor-packaging-linux/      # AppImage, DEB, icons, desktop entry
├── pacor-packaging-macos/      # DMG, plist, localization, signing, notarization
├── pacor-packaging-windows/    # NSIS, portable ZIP, signing
├── pacor-packaging-android/    # APK assembly, ABIs, signing
└── pacor-cli/                  # clap arguments, dispatch, output, binary `pacor`
```

Dependencies flow in one direction: `pacor-core` ← `pacor-packaging-*` ←
`pacor-packaging` ← `pacor-cli`.

### Build templates

Build templates and the fallback icon set live in a separate repository,
[`pacor-rs/tools`](https://github.com/pacor-rs/tools). They are fetched from a
**tag**, never a branch: `pacor-core/src/tools.rs` pins the tag this build is
written against, and every backend derives its download URLs from it.

That keeps the templates and the code that renders them in lockstep — a template
change cannot reach a pacor build that does not understand it. It also means the
tag is part of each cache path (`<tools>/deb/v1`, `<tools>/icons/v1`, …), so
bumping the tag is all it takes to invalidate stale copies; tag contents are
immutable, so nothing else is required.

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

## License

MIT — see [LICENSE](LICENSE).
