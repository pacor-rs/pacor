<p align='center'>
<span>English</span> | <a href="MACOS.zh-CN.md">中文</a>
</p>

# Pacor macOS platform

This document covers the artifacts, configuration, signing and notarization flow, cache
layout and troubleshooting of `pacor build --target macos` (which takes the same
`--use-existing-flutter-build` and `--skip-cleanup` switches as the other targets). The
fully commented configuration template is
[`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml),
and the GitHub mirror related environment variables are described in
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md).

> The macOS backend **builds on macOS only**. Before anything is built, `validate` looks for
> `flutter`, `xcodebuild` and `hdiutil` on `PATH`; `codesign` is added when
> `codesign_identity` is configured and `xcrun` when `notarize_credentials` is. A missing
> tool fails the build with `Platform tool not found: <tool> (required for macOS)` before
> Flutter runs, rather than inside the `bash` call that needed it.

The only output format is `dmg`; `--format` accepts nothing else for this target.

## Artifacts

A successful build writes into `[build] output_dir` (`dist/` by default), each artifact
with a `.sha1` of the same name:

| File | Description |
| ---- | ---- |
| `<name>-<version>-<arch>.dmg` | the installer image, with the `.app` inside it |
| `<name>-<version>-<arch>.app.tar.gz` | the updater archive: the same `.app`, tarred and gzipped |
| the `.sha1` of the files above | a SHA1 checksum file named after the artifact |

`<name>` is the project name from `pubspec.yaml` (`name:`, not `display_name` from
`[app.i18n]`), `<version>` is the raw version (for example `1.0.0+1`), and `<arch>` is the
host architecture as `x64` or `arm64` — this backend packages the architecture it runs on.
For example:

```
dist/MyApp-1.0.0+1-arm64.dmg
dist/MyApp-1.0.0+1-arm64.app.tar.gz
```

The checksums are written last, after signing and notarization, because both rewrite the
bytes they cover.

## Configuration

```toml
[platforms.macos]
enabled = true
minimum_system_version = "10.14"
category = "public.app-category.entertainment"
# bundle_identifier = "com.example.myapp"
# dmg_background = "assets/dmg-background.png"
# dmg_window_size = [500, 350]
# codesign_identity = "Developer ID Application: ..."
# codesign_entitlements = "macos/Runner/Release.entitlements"
# notarize_credentials = "AC_PASSWORD"
```

| Key | Effect |
| --- | --- |
| `enabled` | Whether `--target macos` builds anything; `false` fails with `Platform not enabled: macOS` |
| `bundle_identifier` | Written into the built `Info.plist` as `CFBundleIdentifier`. Without it the identifier Flutter generated stays |
| `minimum_system_version` | `LSMinimumSystemVersion`; default `10.14`, which is what Flutter's own macOS template targets |
| `category` | `LSApplicationCategoryType`, an App Store category identifier; default `public.app-category.entertainment` |
| `dmg_background` | Background image of the installer window. Unset by default |
| `dmg_window_size` | Installer window size. Unset by default |
| `codesign_identity` | A `codesign` identity; enables signing of the `.app` and of the `.dmg` |
| `codesign_entitlements` | Entitlements file for the app signature. Unset uses the file Flutter generated for the build mode |
| `notarize_credentials` | A `notarytool` keychain profile; enables notarization |

The three bundle metadata keys are written into the `Info.plist` Xcode produced, in place:
the entries pacor does not set — booleans, integers, nested dictionaries,
`CFBundleExecutable`, the localizations below — are left exactly as they were. A key whose
setting is absent keeps the value the build wrote.

Setting `dmg_background` or `dmg_window_size` has a second effect: pacor drops its
`--sandbox-safe` flag for that `create-dmg` run, because the window is laid out by a Finder
AppleScript and `--sandbox-safe` is what skips it. That AppleScript needs a GUI session with
Finder automation permission, which is why neither option is set by default.

## Build steps

1. **Install the packaging tools.** `create-dmg.sh` and its `support/` templates are embedded
   in the binary and written into the cache directory (below) on every run, so a cached copy
   is never older than the pacor that is running.
2. **`flutter build macos`** with `--release`, `--profile` or `--debug` from
   `[build] flutter_build_mode`, plus `--obfuscate --split-debug-info=tmp/` for the modes
   that are AOT. With `--use-existing-flutter-build` this is skipped and the bundle already
   in `build/macos/Build/Products/<mode>/` is packaged instead.
3. **Localization.** One `<language-id>.lproj/InfoPlist.strings` per configured locale, named
   with Apple language IDs (`zh-Hans`, not `zh-CN`), plus `CFBundleLocalizations` and
   `CFBundleDevelopmentRegion` in `Info.plist`.
4. **Bundle metadata**, the three keys from the table above, written into the same
   `Info.plist`.
5. **Code signing** of the bundle, when `codesign_identity` is set (see below).
6. **DMG creation** with the bundled `create-dmg`: the `.app` is copied into a temporary
   staging directory first, so the Flutter build output is never modified. The image is named
   `<name> Installer` and gets the usual Applications drop link.
7. **DMG signing**, when an identity is configured: the image is the outer seal of the chain.
8. **Notarization and stapling**, when `notarize_credentials` is set.
9. **Updater archive** and the checksums of both artifacts.
10. **Cleanup** of `tmp/`, where `--split-debug-info` puts the Dart AOT symbols;
    `--skip-cleanup` keeps them for `flutter symbolize`.

## Signing

Signing is driven by `codesign_identity`. Every run uses

```
codesign --force --timestamp --options runtime --sign <identity> \
    [--entitlements <file>] <target>
```

The hardened runtime and a timestamp are unconditional, because a signature without them
cannot be notarized — which is the only reason to configure an identity.

A signature seals the bytes of everything it contains, so the order is not cosmetic:

1. the nested code under `Contents/Frameworks` — `.framework` and `.app` bundles and
   `.dylib` files — **deepest first**, so an item inside another is sealed before its
   container. These are signed **without** entitlements;
2. the `.app` bundle itself, with the entitlements;
3. `codesign --verify --deep --strict --verbose=2` over the bundle. A verification failure
   fails the build: an app Gatekeeper would refuse helps nobody;
4. the `.dmg`, after it has been built. Signing only the image — which is what this backend
   used to do — produces a valid-looking DMG whose payload is unsigned, and `notarytool`
   rejects exactly that with "code object is not signed at all".

Entitlements live inside the signature, so re-signing without them would drop the sandbox and
network access the app was built with. `codesign_entitlements` wins when set and has to name
an existing file; otherwise `macos/Runner/Release.entitlements` is used, or
`DebugProfile.entitlements` for debug and profile builds — the file Xcode would have signed
with.

## Notarization

`notarize_credentials` names a `notarytool` keychain profile, created once with

```
xcrun notarytool store-credentials AC_PASSWORD \
    --apple-id <id> --team-id <team> --password <app-specific password>
```

The build then runs

```
xcrun notarytool submit <dmg> --keychain-profile <profile> --wait --output-format json
xcrun stapler staple <dmg>
```

`notarytool submit --wait` exits 0 whether Apple accepted the upload or rejected it, so the
exit code alone cannot decide whether a build may be shipped: the verdict is read from the
JSON, and a rejection is reported with its submission id — the one thing needed to ask Apple
why:

```
Apple rejected the notarization submission <id> with status "Invalid"; run
`xcrun notarytool log <id> --keychain-profile <profile>` to see why
```

Without this, a rejected upload surfaced only as the failure of the `stapler` run that
followed, which says nothing about the cause. `--verbose` on the pacor command line adds
`--verbose` to both `notarytool` and `stapler`.

## Cache layout

Everything the backend does not download lives in

```
~/Library/Application Support/pacor/macos/
  create-dmg.sh                    # the embedded script, refreshed on every run
  support/template.applescript
  support/eula-resources-template.xml
```

`pacor cache clean` removes the whole cache directory (with `--dry-run` to list what would
go); the next build writes the files again. Nothing else is cached: the Flutter build output
lives in the project and is removed by `clean_build_directories` (`build/macos`).

## Reusing an existing Flutter build

`--use-existing-flutter-build` packages the bundle already in
`build/macos/Build/Products/<mode>/` and skips step 2 above. Every other step still runs —
the packaging tools are installed, the bundle is localized and gets its metadata, it is
signed, and the DMG, the updater archive and the checksums are produced. A missing
`<mode>` directory fails with `Flutter build output directory does not exist`.

## Known limitations

- **macOS host only.** The crate is compiled and tested on every host, but a build needs
  `xcodebuild` and `hdiutil`.
- **Nested code signing covers one level.** The plan walks the entries directly inside
  `Contents/Frameworks`; code nested deeper than that is covered by the bundle's signature
  but not signed on its own.
- **The ticket is stapled to the image.** The `.app` inside the updater archive is not
  stapled: it is replaced in place by the updater, and Gatekeeper checks the copy on disk.
- **`Info.plist` has to be XML.** A binary plist cannot be edited in place; the read failure
  names the path and the `plutil -convert xml1` command that fixes it.
- **The window layout needs Finder automation.** Without it, leave `dmg_background` and
  `dmg_window_size` unset and the DMG uses the sandbox-safe path.
- **`[app.i18n]` localizes the bundle, not the image.** The volume name and the artifact
  names use the project name.

## Troubleshooting

| Symptom | Cause | What to do |
| --- | --- | --- |
| `Platform tool not found: hdiutil (required for macOS)` | The host is not macOS, or `PATH` does not have the tool | Run the build on macOS and check with `which hdiutil`; `codesign` is only required when signing and `xcrun` only when notarizing |
| `failed to read .../Info.plist (... `plutil -convert xml1` ...)` | The build wrote a binary plist, which is not text and cannot be edited in place | Convert it (`plutil -convert xml1 <the path>`) and build again |
| `Apple rejected the notarization submission <id>` | Apple's log holds the reason, not the exit code | `xcrun notarytool log <id> --keychain-profile <profile>` |
| `code object is not signed at all` from `notarytool` | The DMG's payload is unsigned | Configure `codesign_identity`: pacor signs nested code, then the bundle, then the image, in that order |
| Gatekeeper refuses the downloaded DMG or app | The copy is unsigned, or signed but not notarized and stapled | Configure both `codesign_identity` and `notarize_credentials`; check the result with `spctl -a -vvv -t install dist/<name>-<version>-<arch>.dmg` |
| The installer window ignores `dmg_background` / `dmg_window_size` | The window is laid out by a Finder AppleScript, which needs a GUI session and Finder automation permission | Run the build in a logged-in session, or unset both options and accept the default layout |
| `codesign_entitlements` points at a path that is not a file | The path is resolved from the project root | Fix the path, or drop the key to use the entitlements file Flutter generated |
| `Flutter build output directory does not exist` with `--use-existing-flutter-build` | `build/macos/Build/Products/<mode>/` is not there | Build once without the switch |
| `could not read the notarization result: ...` | Apple changed the JSON shape of `notarytool` | The raw output is in the error message; the submission itself is unaffected and can be checked with `xcrun notarytool log` |
