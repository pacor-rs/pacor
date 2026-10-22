<p align='center'>
<span>English</span> | <a href="ANDROID.zh-CN.md">中文</a>
</p>

# Pacor Android platform

This document covers the artifacts, configuration, signing flow, known limitations and
troubleshooting of `pacor build --target android` (`pacor android` is its alias, and takes
the same `--use-existing-flutter-build` and `--skip-cleanup` switches). The
fully commented configuration template is
[`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml),
and the GitHub mirror related environment variables are described in
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md).

> Unlike the other platforms, the Android backend **downloads no tools or templates** and
> does not use the `%APPDATA%\pacor` cache: it only invokes the local `flutter`, and the
> toolchain (for building and for signing) comes from the Flutter / Android SDK / JDK you
> already installed.

Android can produce two kinds of artifact, and **only APKs by default**:

| `formats` | Artifacts | Use |
| --- | --- | --- |
| `["apk"]` (default) | one `.apk` per ABI | direct installation: downloads from the website, enterprise/MDM distribution, sideloading, in-app self-update |
| `["apk", "aab"]` | the APKs above plus one `.aab` | an extra copy to upload to Google Play |
| `["aab"]` | only the `.aab` | projects distributed through Play only |

An AAB (Android App Bundle) is a **distribution format rather than an installable
package**: it packs every ABI, screen density and language into one file, and Google Play
generates the APK that a device actually installs from it. It cannot be installed
directly (local testing needs `bundletool build-apks` or a Play test track). It therefore
complements the APK instead of replacing it, and it is only worth enabling when you ship
to Play — some projects publish APKs only, and those keep the default.

## Artifacts

A successful build writes the artifacts of the selected formats into `[build] output_dir`
(`dist/` by default), each with a `.sha1` of the same name:

| File | Description |
| ---- | ---- |
| `<name>-<version>-<abi>.apk` | a per-ABI APK, one for each ABI in `target_abi` |
| `<name>-<version>.aab` | the Android App Bundle; one file no matter how many ABIs |
| the `.sha1` of the files above | a SHA1 checksum file named after the artifact |

`<name>` is the project name from `pubspec.yaml` (`name:`, not `display_name` from
`[app.i18n]`), `<version>` is the raw version (for example `1.0.0+1`), and `<abi>` is the
Android NDK ABI name (`arm64-v8a` / `armeabi-v7a` / `x86_64`). For example:

```
dist/probe_app-1.0.0+1-arm64-v8a.apk
dist/probe_app-1.0.0+1.aab
```

The `.sha1` is generated **after signing**: signing rewrites the file contents, so
computing the checksum first would publish a wrong value (the Windows backend signs
before it checksums for the same reason, see [WINDOWS.md](WINDOWS.md)).

pacor only copies the artifact out and renames it by the rules above; everything else
(`versionCode`, `versionName`, `applicationId`, the icon, the permissions) is decided
entirely by Flutter / Gradle.

## Configuration

```toml
[platforms.android]
enabled = true
target_abi = ["arm64-v8a"]        # default ["arm64-v8a"]; at least one entry
formats = ["apk"]                 # default ["apk"]; use ["apk", "aab"] to ship to Play
# flavor = "prod"                 # maps to flutter build … --flavor prod
# signing (optional; configuring keystore_file enables it for every selected format)
# keystore_file = "android/app/upload-keystore.jks"   # relative to the project root
# key_alias = "upload"
# signing_tool_path = "C:/Android/sdk/build-tools/35.0.0/apksigner.bat"  # only used for apk
# jarsigner_path = "C:/Program Files/Android/Android Studio/jbr/bin/jarsigner.exe"  # only used for aab
# keystore_password = "..."       # prefer PACOR_ANDROID_KEYSTORE_PASSWORD
# key_password = "..."            # prefer PACOR_ANDROID_KEY_PASSWORD
```

With `enabled = false` the target fails outright (`Platform not enabled: Android`), like
every other platform. `formats = []` is a configuration error: it would make the build
produce nothing, so it is rejected outright. `--format apk` / `--format aab` override
`formats` on the command line (same `--format` semantics as `--target linux`; repeat
`--format` for several values).

## Build flow

1. Validate the environment: `flutter` is on `PATH`, the project root has `pubspec.yaml`
   and an `android/` directory, and the output directory is writable (it is created when
   missing); the `formats`/`target_abi` emptiness is checked too, and when signing is
   enabled the `keystore` and the signing tool required by each selected format are
   validated separately (see below).
2. Run the equivalent commands:

   ```bash
   # when formats contains apk
   flutter build apk --release --split-per-abi --split-debug-info=tmp/ \
       [--flavor <flavor>] [--target-platform android-arm64,android-x64,…]
   # when formats contains aab (runs after the APK: the cheap step first)
   flutter build appbundle --release --split-debug-info=tmp/ \
       [--flavor <flavor>] [--target-platform android-arm64,android-x64,…]
   ```

   `--target-platform` takes its values from `target_abi` in a fixed order
   (`armeabi-v7a` → `arm64-v8a` → `x86_64`), so the same configuration produces exactly
   the same command line every time. For APKs it decides which files are produced; for an
   AAB it decides **which ABIs that single file contains** — so listing only
   `arm64-v8a` means the bundle on Play supports 64-bit devices only (Play splits further
   by device and language, so listing `armeabi-v7a` as well is usually the safer choice).
3. Locate each artifact (APKs: see "Locating the APK artifact", AAB: see "Locating the
   AAB artifact").
4. Copy it into `dist/`, sign it as configured, then generate the `.sha1`.
5. Unless `--skip-cleanup` is given, run `flutter clean` when the build finishes and
   delete the `tmp/` symbols directory.

### Debug symbols and `tmp/`

`--split-debug-info=tmp/` makes the Dart AOT symbols land in **`tmp/` under the project
root** (consistent with the Windows, macOS and Linux backends), which keeps the APK
smaller. These `.symbols` files are what `flutter symbolize` needs to restore a
production stack trace:

- they are deleted when the build finishes by default; use `--skip-cleanup`, or copy them
  out of `tmp/` first, to keep them. Note that `pacor clean` removes `dist/` and `build/`,
  and **not** `tmp/`;
- the Android backend **does not obfuscate Dart** (it passes no `--obfuscate`): the
  symbols restore method names and line numbers, and obfuscation would need
  `--obfuscate`, which is not a pacor configuration option today.

### Build mode

The Android backend always builds **release** (both APK and AAB), and
`[build] flutter_build_mode` and `PACOR_BUILD_MODE` have no effect on it — an APK's build
mode shows up in its file name rather than in a directory, so "Android always builds
release" is deliberate, but it also means those two configuration options are silently
ignored on Android.

### Locating the APK artifact

Flutter's Gradle plugin copies the split APKs into `build/app/outputs/flutter-apk/` and
names them `app<-abi>?<-flavor-name>?-<build-mode>.apk`. pacor tries, in order:

1. `app-<abi>-release.apk`;
2. `app-<abi>-<flavor>-release.apk` when `flavor` is configured;
3. the only file in the directory matching `app-<abi>-*-release.apk` (for example an
   artifact a `flavor` produced under a longer name).

More than one match fails the build and lists the candidates instead of picking one; no
match lists what the directory actually contains. That way a project that has a `flavor`
but forgot to put it into `pacor.toml` gets a readable error rather than a bare
`os error 2` (the localized "cannot find the file specified").

### Locating the AAB artifact

The path rules for an AAB differ from the APK's: there is a single file, and it lives in
a **directory that carries the build mode (and the flavor)**:

```
build/app/outputs/bundle/release/app-release.aab                  # no flavor
build/app/outputs/bundle/<flavor>Release/app-<flavor>-release.aab # with a flavor
```

(This matches the rule Flutter itself uses in `findBundleFile`; AGP versions differ in how
they treat the directory's case, so pacor tries `release/`, `<flavor>Release/` and
`<flavor>release/` exactly first, then does one case-insensitive scan.)

When the bundle sits in a flavor directory but the configuration does not mention
`flavor`, pacor **does not** guess: it fails, lists the `.aab` files it found and points
at `platforms.android.flavor` — such a file is usually another flavor's output or a
leftover from an earlier build, and packaging it silently would ship the wrong thing.
Several candidates behave the same way and list their names.

## ABI

| `target_abi` | `--target-platform` | Flutter artifact file name |
| --- | --- | --- |
| `"armeabi-v7a"` (alias `"arm"`) | `android-arm` | `app-armeabi-v7a-release.apk` |
| `"arm64-v8a"` (alias `"arm64"`) | `android-arm64` | `app-arm64-v8a-release.apk` |
| `"x86_64"` (alias `"x64"`) | `android-x64` | `app-x86_64-release.apk` |

`target_abi` is a **set**, and duplicates are removed; `[]` is a configuration error (the
old implementation silently fell back to "package arm64-v8a only" while still letting
Flutter build every ABI, which produced fewer artifacts than the configuration asked
for). A 32-bit `x86` cannot build a release, so it is not in the supported list (see the
[Flutter documentation](https://flutter.dev/to/android-supported-architectures)).

## Signing

**Configuring `keystore_file` enables signing**, for every format in `formats`; each item
below is required, and a missing one is an error rather than a silent skip:

| Item | Source |
| --- | --- |
| the keystore file | `platforms.android.keystore_file` (a relative path resolves against the project root and must exist) |
| the alias | `platforms.android.key_alias` |
| the keystore password | `PACOR_ANDROID_KEYSTORE_PASSWORD` first, then `platforms.android.keystore_password` |
| the key password | `PACOR_ANDROID_KEY_PASSWORD` first, then `platforms.android.key_password` |
| the APK signing tool | `signing_tool_path` → `apksigner` on `PATH` → the Android SDK build-tools |
| the AAB signing tool | `jarsigner_path` → `jarsigner` on `PATH` → `JAVA_HOME` or a common JDK location |

> Configuring only a tool path does not enable signing (unlike Windows, where both
> `certificate_path` and `signing_tool_path` count as enabling it): on Android the
> keystore is what signs, and the two tool paths are only lookup hints. Tools are
> validated **per format**: `formats = ["aab"]` does not require `apksigner`, and
> `formats = ["apk"]` does not require `jarsigner`.

`pacor check --target android` validates that the keystore exists, that the passwords are
provided and that the signing tool the selected formats need can be located, so a
configuration mistake shows up before the build starts (consistent with the `signtool`
validation in [WINDOWS.md](WINDOWS.md)).

### `apksigner` lookup order (APK)

1. `platforms.android.signing_tool_path` (must exist);
2. `apksigner` on `PATH` (on Windows `apksigner.bat` is tried as well);
3. the Android SDK build-tools, taking the highest version:

   ```
   <sdk>/build-tools/<version>/apksigner[.bat]
   ```

   `<sdk>` is taken from `ANDROID_HOME`, `ANDROID_SDK_ROOT`, then the `sdk.dir` in
   `<project root>/android/local.properties`.

The signing command (with `apksigner`'s argument style):

```
apksigner sign --ks <keystore> --ks-key-alias <alias> \
    --ks-pass <password source> --key-pass <password source> <apk>
```

The password source has two spellings: when the password comes from an environment
variable use `env:PACOR_ANDROID_KEYSTORE_PASSWORD` (it does not appear on the process
command line, which makes it the recommended path), and when it comes from the
configuration file use `pass:<plaintext>` (the same as the Windows backend passing the
password as a `signtool /p` argument: it shows up in the process list, so it is not
recommended).

The signature schemes (v1/v2/v3) are left to `apksigner`, which picks them from the APK's
own `minSdkVersion` / `targetSdkVersion`; pacor passes no `--v*-signing-enabled`. The
APK's zipalign alignment is done by AGP's release build, and `apksigner` re-signing
preserves it, so pacor needs no separate `zipalign` step.

Two exceptions, both so that `dist/` contains only files pacor reports and checksums:

- **v4 signing is explicitly disabled** (`--v4-signing-enabled false`). It is one of
  `apksigner`'s default behaviours, and it writes an `<apk>.idsig` next to the APK (for
  `adb install --incremental`). That file is neither in pacor's artifact list nor covered
  by a `.sha1`, so leaving it there would make the output directory hold more files than
  pacor declares. If you need it, run `apksigner` yourself.
- **`JAVA_HOME` is filled in by pacor**: `apksigner.bat` is only a wrapper that invokes
  `java`, and with neither `JAVA_HOME` nor that command on `PATH` it exits with
  `ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH`.
  Flutter uses Android Studio's JBR when it runs Gradle for itself, but a terminal that
  only runs pacor usually has nothing configured, so pacor finds a JDK on its own (same
  order as above) and passes it to `apksigner` through `JAVA_HOME`; when `JAVA_HOME` is
  already set, it is left alone.

### `jarsigner` lookup order and AAB signing (AAB)

An AAB is a JAR and takes a **JAR signature**; `apksigner`'s signature schemes do not
apply to it, so it goes through `jarsigner`:

1. `platforms.android.jarsigner_path` (must exist);
2. `jarsigner` on `PATH`;
3. `JAVA_HOME/bin/jarsigner[.exe]`, then the usual JDK locations: on Windows
   `%ProgramFiles%\Android\Android Studio\jbr\bin\jarsigner.exe` (and a per-user install
   under `%LOCALAPPDATA%`), on macOS
   `/Applications/Android Studio.app/Contents/jbr/Contents/Home`, on Linux
   `/opt/android-studio/jbr` and `/usr/lib/jvm/*`.

The signing command:

```
jarsigner -keystore <keystore> -storepass[:env] <password> -keypass[:env] <password> \
    -digestalg SHA-256 <aab> <alias>
```

A password that comes from an environment variable uses
`-storepass:env PACOR_ANDROID_KEYSTORE_PASSWORD` / `-keypass:env PACOR_ANDROID_KEY_PASSWORD`
(measured as working with the JDK 21 `jarsigner`), while one from the configuration file
is passed literally. `-digestalg` is pinned to SHA-256 so that the SHA-1 older JDKs default
to does not make Play reject the bundle. The signature algorithm is **not** pinned:
`jarsigner` derives it from the private key (SHA256withRSA for RSA, SHA256withECDSA for
EC), and naming SHA256withRSA explicitly failed every EC keystore with "signature
algorithm SHA256withRSA not compatible with key".

**Any JAR signature Gradle left behind is stripped before signing.** `flutter build appbundle`
signs the AAB with the project's Gradle signing configuration (the **debug** keystore when
nothing is configured), and `jarsigner` **appends** a signature rather than replacing one, so
without this step a published AAB would carry both the debug certificate and the upload key
— measured: after a plain append the bundle holds `META-INF/ANDROIDD.*` and
`META-INF/UPLOAD.*` at the same time. pacor therefore deletes
`META-INF/MANIFEST.MF` and `META-INF/*.SF|RSA|DSA|EC|SIG` before signing and lets
`jarsigner` rebuild the manifest: the AAB that is published has exactly one signer, the
configured upload key (the rest of `META-INF/`, such as `META-INF/services/…`, is payload
and is preserved). APKs need no such step: `apksigner` rewrites the APK's signature block.

> Play App Signing: what is uploaded uses the **upload key**, and Google re-signs the APK
> it distributes to users with the app signing key. pacor therefore only has to make sure
> the bundle is "correctly signed with the upload key"; it neither needs to nor can
> replace the final signature.

### Signer verification and the debug signing warning

When `keystore_file` is **not** configured, pacor does not modify the artifact, and its
signature is entirely up to the project's `android/app/build.gradle(.kts)`. The template
Flutter generates uses the **debug keystore** for release builds (the original comment in
it: *Signing with the debug keys for now*), so an APK and an AAB without configured
signing are debug signed by default: Google Play rejects them outright, and switching to a
real key later cannot cover an upgrade either (APK).

So when signing is not configured, pacor inspects:

- APK: `apksigner verify --print-certs`;
- AAB: `jarsigner -verify -verbose -certs` (the signature Gradle wrote into the bundle is
  left exactly as it is; pacor only reads it).

A signer subject of `CN=Android Debug` produces an explicit warning pointing at
`platforms.android.keystore_file`; for an AAB it adds that "Play requires an upload key
signature". When the corresponding tool cannot be found the check is skipped (logged at
debug level only) and does not fail the build.

## Known limitations

| Limitation | Notes |
| ---- | ---- |
| an AAB cannot be installed directly | it is Play's distribution format; local verification needs `bundletool build-apks` or a Play test track |
| `aab` has to be enabled explicitly | `formats = ["apk"]` is the default: a project that only publishes APKs need not pay for a file it will not use |
| no Dart obfuscation | `--obfuscate` is not passed; the symbols still go to `tmp/` and are deleted when the build finishes by default (`--skip-cleanup` keeps them) |
| the build mode is ignored | always release, `flutter_build_mode` / `PACOR_BUILD_MODE` have no effect |
| `[app.i18n]` is not used | neither artifact has an install UI, so localization is decided by the app itself and `AndroidManifest.xml` |
| a single flavor | `flavor` is one value, and a build produces the APK/AAB of one flavor only |
| the artifact contents are not validated | `versionCode`/`applicationId` are not modified, and existing files in `dist/` are not checked for whether this build produced them |
| no `.idsig` is produced | v4 signing is off, so `dist/` only holds artifacts pacor reports and checksums (see "Signing") |
| Play uploads are not managed | pacor only produces and signs the AAB; uploading, Play App Signing and test tracks are yours to do |

## Common failures and troubleshooting

### `Platform tool not found: flutter (required for Android)`

`flutter` is not on `PATH`. Android packaging needs only `flutter` — **not `adb`** (the
old implementation listed `adb` as required, so a machine with the Android SDK installed
but `platform-tools` missing from `PATH` could not package at all).

### `Configuration validation failed: platforms.android.formats`

`formats = []` was configured. Delete the line to go back to the default `["apk"]`, or
write `["apk", "aab"]`.

### `Configuration validation failed: platforms.android.target_abi`

`target_abi = []` was configured. Deleting the line restores the default `arm64-v8a`.

### `Flutter did not produce an APK for <abi>`

Flutter produced no APK for that ABI, or the name it produced does not match what was
expected (most often because the project has a `flavor` that is missing from
`pacor.toml`). The error lists the directory contents and the candidates; check
`platforms.android.flavor` and retry. `several APKs match <abi>` means the directory holds
several candidates (for example several flavors built at once), in which case `flavor` has
to be set explicitly.

### `Flutter did not produce an Android App Bundle`

The same, but for `build/app/outputs/bundle/`. The error gives the expected path
(including `<flavor>Release/app-<flavor>-release.aab` when a flavor is in play) and the
`.aab` files that actually exist; when those `.aab` files sit in a flavor directory, the
error points at `platforms.android.flavor`. `several App Bundles match` means there are
several candidates, and `flavor` has to be set explicitly here too.

### Gradle download timeout (`Connection timed out`)

The real Android build is done by Gradle, and the Gradle distribution and the dependencies
(AGP, Kotlin, AndroidX) come from `services.gradle.org`, `google()` and `mavenCentral()`.
**pacor neither proxies nor configures those repositories**, so on a restricted network
what you see is Gradle's own error:

```
Exception in thread "main" java.net.ConnectException: Connection timed out: connect
    at org.gradle.wrapper.Download.downloadInternal(Download.java:58)
```

That one comes from the Gradle **wrapper**, which means even the Gradle distribution could
not be downloaded; when the error is `Could not resolve com.android.tools.build:gradle`, it
is the dependency repositories. Both need a mirror, and both are configured under the
project's `android/` (pacor is not involved and needs no reconfiguration):

1. the Gradle distribution, `android/gradle/wrapper/gradle-wrapper.properties`:

   ```properties
   # Tencent Cloud and Huawei Cloud carry every version; the Aliyun mirror does not have -all for all of them
   distributionUrl=https\://mirrors.cloud.tencent.com/gradle/gradle-8.12-all.zip
   ```

2. the dependency repositories, both in `pluginManagement.repositories` of
   `android/settings.gradle.kts` **and** in `allprojects.repositories` of
   `android/build.gradle.kts` (the AGP/Kotlin plugins resolve through the former, so both
   need the entries):

   ```kotlin
   maven { url = uri("https://maven.aliyun.com/repository/google") }
   maven { url = uri("https://maven.aliyun.com/repository/public") }
   maven { url = uri("https://maven.aliyun.com/repository/gradle-plugin") }
   // keep the official repositories as a fallback
   google()
   mavenCentral()
   ```

   They can also go into a user-level `~/.gradle/init.gradle`, covering every project at
   once. Flutter's own engine artifacts go through `FLUTTER_STORAGE_BASE_URL` (pacor is
   not involved there either). Measured on this machine: this configuration turns the
   first release build of a fresh Flutter project from "stuck downloading" into something
   that completes (see the audit record at the end).

### `apksigner could not verify the APK: ERROR: JAVA_HOME is not set ...`

`apksigner` needs a JDK. pacor fills one in automatically when the environment has no
`JAVA_HOME` (see "Signing"), so this usually only appears when pacor **cannot find** any
JDK: install a JDK or Android Studio, or set `JAVA_HOME`.

### `platforms.android.keystore_file` errors

`... does not exist` means check the path (relative to the project root);
`set PACOR_ANDROID_KEYSTORE_PASSWORD ...` means the password is missing
(`... to sign APKs` / `... to sign App Bundles` tells you which format needs it). A
missing item among the four required ones fails before the build starts.

### `no signing tool found: install the Android SDK build-tools ...`

`apksigner` is neither on `PATH` nor in the SDK build-tools. Install the build-tools with
the Android SDK Manager, or set `platforms.android.signing_tool_path`.

### `no App Bundle signing tool found: install a JDK ...`

`jarsigner` is neither on `PATH` nor in `JAVA_HOME` or the usual JDK locations. Install a
JDK (or Android Studio, which ships the JBR), or set `platforms.android.jarsigner_path`.

### `... is not a readable Android App Bundle (expected a zip)`

The file in `build/app/outputs/bundle/...` is not a valid zip/AAB (something else
overwrote it, for example). Run `flutter build appbundle` again, or drop
`--use-existing-flutter-build`.

### The APK or AAB is debug signed (`CN=Android Debug`)

The project has no release signing configured, or `pacor.toml` has no `keystore_file`.
Generate an upload keystore the way Flutter documents it, then set
`platforms.android.keystore_file` / `key_alias` and the two password environment
variables. For an AAB this matters especially: Play only accepts bundles signed with the
upload key.

## Development verification

```bash
cargo test -p pacor-packaging-android
```

Neither suite **needs a real Flutter installation, Android SDK or JDK** (the end-to-end
tests place their own `flutter` and `jarsigner` stubs), so they run as part of
`cargo test --workspace` on the three CI platforms:

- the unit tests (`src/`): the ABI and file-name mapping; APK artifact lookup (exact name,
  flavor, a unique candidate, several candidates failing, an empty directory, a missing
  directory); AAB artifact lookup (`release/`, a flavor directory with different case, a
  flavor that is not configured, several candidates, a missing file); JAR signature
  stripping (which entries count, payload preservation, no temporary file left behind on
  failure); the ordering of the `apksigner` SDK candidates; the unescaping of
  `local.properties`; parsing the `--print-certs` and `jarsigner -verify` output; debug
  certificate recognition; both spellings of the password source; locating `jarsigner`
  under a JDK; the configuration defaults, deduplication and relative-path resolution; and
  that an empty `target_abi`/empty `formats` is a configuration error.
- the end-to-end tests (`tests/android_smoke.rs`): with a hand-written fixture project and
  three stubs that only record their arguments (`flutter`, `jarsigner`, `apksigner`), they
  drive `validate` → `build` → artifact collection and naming → signing → `.sha1` →
  `clean`, and assert the full argument vector of `flutter build apk`/`appbundle` (only the
  APK one carries `--split-per-abi`), the full `apksigner` signing arguments (including
  `--v4-signing-enabled false`), that both formats are packaged, that a bundle-only
  configuration produces no APK, that AAB signing strips the Gradle signature first and
  keeps the payload, that an APK/AAB pacor did not sign stays untouched, that the checksums
  cover the published files, that `clean` removes `tmp/`, and the error paths for an empty
  `formats`, an empty `target_abi`, a missing keystore, a missing password, a missing ABI
  artifact and a missing AAB.

What still has no automated test is the **real chain** (`flutter build apk`/`appbundle`
actually producing a package, and the two signing tools actually signing): it needs Flutter
+ Android SDK + JDK on the runner, and `.github/workflows` does not carry that toolchain
today. The chain has been run by hand on this machine and is recorded at the end of this
document (including a real Gradle build, real `apksigner`/`jarsigner` signing and an
independent verification).

For a quick regression you can reuse an existing build and skip the slow Gradle step:

```powershell
pacor check --target android
pacor build --target android --use-existing-flutter-build --skip-cleanup
```

### AAB support (added the same day)

The audit above covered only the "APKs only" implementation. AAB support was added
afterwards along the same checklist, and it surfaced two more problems that only a real
toolchain exposes:

| Finding | Symptom (measured with the real `jarsigner`) | Handling |
| ---- | ---- | ---- |
| `jarsigner` **appends** a signature instead of replacing one | after signing an AAB that Gradle had already signed (with the debug key) using the upload key, the bundle holds `META-INF/ANDROIDD.*` and `META-INF/UPLOAD.*` at the same time — shipping that means shipping the debug certificate along with it | strip `META-INF/MANIFEST.MF` and `META-INF/*.SF\|RSA\|DSA\|EC\|SIG` before signing (a `zip` rewrite) and let `jarsigner` rebuild the manifest; measured: the published bundle keeps only `META-INF/UPLOAD.*`, and `jarsigner -verify` reports only `CN=My Upload Key` |
| the ABI semantics of an AAB differ from the APK's | an AAB is a single file, so `--target-platform` becomes "which ABIs the bundle contains" | `target_abi` serves both as the APK's output dimension and as the content limit of the AAB, and this document plus the template comment state that "keeping only arm64 makes the bundle on Play support 64-bit devices only" |

How the real chain was verified (this machine, Windows + JDK 21 from the Android Studio
JBR):

```powershell
# 1) create two keystores: one pretending to be the Android debug key, one as the upload key
keytool -genkeypair -keystore debug.keystore -storepass android -keypass android `
  -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" -keyalg RSA -validity 10000
keytool -genkeypair -keystore upload.jks -storepass <store password> -alias upload `
  -dname "CN=My Upload Key,OU=Mobile,O=Example Ltd,C=CN" -keyalg RSA -validity 10000
# 2) sign a stand-in AAB (a zip) by hand with jarsigner and put it into build/app/outputs/bundle/release/
# 3) run pacor without a keystore: it should warn about CN=Android Debug and leave the bundle's signature untouched
# 4) run again with keystore_file: it should strip the old signature, re-sign with the upload key, and .sha1 should cover the signed bytes
jarsigner -verify -verbose -certs dist/<name>-<version>.aab   # only CN=My Upload Key should remain
```

### Measured packaging run (same day, real Flutter project)

The steps above used stand-in artifacts. Afterwards the complete chain was run on a real
project created with `flutter create --platforms=android` (`pacor_probe`,
`formats = ["apk", "aab"]`, `target_abi = ["arm64-v8a", "armeabi-v7a"]`):

| Step | Result |
| ---- | ---- |
| `flutter build apk` (through pacor) | `app-armeabi-v7a-release.apk` 5.7 MB and `app-arm64-v8a-release.apk` 6.3 MB |
| `flutter build appbundle` (through pacor) | `app-release.aab` 23.8 MB |
| the `dist/` artifacts | 2 APKs and 1 AAB, each with a `.sha1`; all three checksums match the published files |
| no keystore configured | all three artifacts are correctly recognised as debug signed (`C=US, O=Android, CN=Android Debug`) and warned about |
| keystore configured | the 2 APKs re-signed with the real `apksigner` and the AAB with the real `jarsigner`; independent verification: the APK's `Signer #1` and the AAB's `X.509` both show only `CN=Pacor Upload Key`, and the AAB's `META-INF/` holds only `MANIFEST.MF`/`UPLOAD.SF`/`UPLOAD.RSA` (Gradle's `ANDROIDD.*` has been stripped) |
| artifact contents (`aapt2 dump badging`) | `com.example.pacor_probe`, `versionCode=1`, `versionName=1.0.0`, `minSdkVersion=21`, and each APK's `native-code` contains only its own ABI |
| the `tmp/` symbols | `app.android-arm64.symbols` (1.39 MB) and `app.android-arm.symbols` (1.17 MB), kept with `--skip-cleanup` |

This round exposed two more problems that only a real toolchain produces, both fixed:

| Finding | Symptom | Handling |
| ---- | ---- | ---- |
| `apksigner.bat` needs `JAVA_HOME` | the terminal had neither `java` nor `JAVA_HOME` (Flutter uses Android Studio's JBR when it runs Gradle for itself; pacor does not) → `apksigner` exited printing `ERROR: JAVA_HOME is not set...`, and pacor could only report a useless `apksigner verify failed` (that error was on **stdout**, while the fallback at the time read stderr only) | pacor resolves the JDK location itself and passes it to `apksigner` through `JAVA_HOME` (left alone when already set); the failure reason is now taken from stderr, then stdout |
| `apksigner` writes an `.idsig` by default | v4 signing is on by default, so `dist/` gained an `<apk>.idsig` (58 KB / 54 KB) that was neither in the artifact list nor covered by a `.sha1` | `--v4-signing-enabled false` is passed when signing, so `dist/` only keeps the files pacor reports and checksums |
| the order of the real signer DN | the real output is `C=US, O=Android, CN=Android Debug`, unlike the `CN=Android Debug, O=…` the stand-in assumed | the debug check matches a substring rather than comparing whole strings (it already did; the real output and a test now pin it) |

> No Android device or emulator was connected to this machine (`adb devices` is empty), so
> there was no `adb install` verification; the APKs' installability is confirmed only
> indirectly, through `apksigner verify` and `aapt2 dump badging`. The first Gradle build
> also got stuck downloading (a `services.gradle.org` timeout and slow Maven
> dependencies); after switching to a mirror as described in "Gradle download timeout",
> the first build finished in 275 s.

## Appendix: defects found in the 2026-10-17 audit and how they were handled

The audit was based on the `README.md`, `README.zh-CN.md`, `docs/MSI.md`,
`docs/WINDOWS.md` and `docs/ENVIRONMENT_VARIABLES.md` of the time, plus
`crates/pacor-packaging-android` (a single 250-line file with 0 tests). The environment
measured: Windows + Rust 1.99 + Flutter 3.32.8, with the Android SDK installed on this
machine (`platform-tools/adb.exe` present but not on `PATH`); to skip the time
`flutter build` takes, `--use-existing-flutter-build` drove the "collect → copy → sign"
stages directly, with stand-in APKs placed in `build/app/outputs/flutter-apk/`.

| Defect | Symptom at the time (measured) | Handling now |
| ---- | ---- | ---- |
| signing was an empty implementation (`TODO` + `warn!` + `Ok(())`) | with a keystore (whose path did not even exist) and both passwords configured, the exit code was 0, `Build Completed Successfully` was printed, and the APK in `dist/` had exactly the SHA1 of Flutter's output — not a single byte changed | `apksigner` signing is implemented; an incomplete signing configuration or an unavailable tool **fails the build**; cases the signer check does not cover produce the debug signing warning |
| the signing gate was inverted | `keystore_file` set and a password missing → hard failure `keystore_password not configured`; all passwords present → signing silently skipped | all four required items are validated in `validate()`; a failing `sign()` fails the build |
| the `pacor init` template taught users to write an invalid TOML key | `PACOR_ANDROID_KEYSTORE_PASSWORD="..."` under `[platforms.android]` was silently ignored by serde (no `deny_unknown_fields`), and `pacor config --full` looked fine and did not show it — the user believed signing was configured | the template now uses `keystore_password` / `key_password` comments and explains the environment variables' precedence |
| `adb` was mandatory | `platform-tools\adb.exe` existed on this machine but was not on `PATH`, so both `pacor check --target android` and `pacor build --target android` exited in the `validate` stage with `Platform tool not found: adb (required for Android)`; adding `platform-tools` to `PATH` made it succeed immediately | the only required tool left is `flutter` |
| the symbols went into a `TempDir` and were deleted immediately | `--split-debug-info <OS temp directory>`, and the directory was dropped when `build_apk` returned; even `--skip-cleanup` could not keep it, so a production stack trace could never be restored | they go to the project's `tmp/`; `clean()` removes it and `--skip-cleanup` keeps it |
| `flavor` was ignored and artifacts were located by a hard-coded file name | a project with a product flavor always failed with a context-free `os error 2` (the localized "cannot find the file specified"), and the ABIs copied up to that point stayed in `dist/` | a `flavor` setting was added and `--flavor` is passed through; artifact lookup has three fallbacks and its error lists the candidates or the directory |
| `target_abi = []` silently packaged arm64 only | the directory held APKs for 3 ABIs, `pacor config` showed `target_abi = []`, `dist/` had only `arm64-v8a`, and the exit code was 0 | an empty list is a configuration error |
| `target_abi` was a `HashSet`, so the order was not stable | four consecutive `pacor config --full` runs on the same configuration alternated between `["arm64-v8a","armeabi-v7a"]` and the reverse; the order of the `--target-platform` values, of the artifacts, and of which ABI was reported first on failure were all random | it is a `BTreeSet` now, with a fixed order |
| a needless `Arch::current()` on the Android path | `AndroidBuilder::new` called it, although an Android build directory has nothing to do with the host architecture; on a host that is not one of `{x86_64, aarch64}` it failed outright with `Unsupported architecture` | the host architecture is no longer queried (a `FlutterProject::android_build_path` was added) |
| the backend had 0 tests and CI had no Android job | `cargo test -p pacor-packaging-android -- --list` → `0 tests, 0 benchmarks`; grepping `.github` found no `android`/`apk`/`adb` | 51 unit tests and 12 end-to-end tests were added, running as part of `cargo test --workspace` on the three CI platforms; a toolchain job with a real Flutter/SDK is still not there |
| the documentation disagreed with the implementation | the README status table said "implemented" and the project-layout comment said "signing", while signing was a `TODO`; the README listed `pacor android` as a public command although it was `hide = true`; `--sign-apk` only logged a WARN | the READMEs gained an Android section linking to this document; `--sign-apk` was removed and `pacor android` was unhidden from its help (the README had always treated it as public); the limitations are recorded in one place, this document |
