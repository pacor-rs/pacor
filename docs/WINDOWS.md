<p align='center'>
<span>English</span> | <a href="WINDOWS.zh-CN.md">中文</a>
</p>

# Pacor on Windows

This document describes the artifacts, configuration, toolchain caches and common
troubleshooting for `pacor build --target windows`.
For the fully commented template of every configuration key, see
[`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml);
for the environment variables that control the GitHub mirrors, see
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md).

> The design of the MSI installer (`installer_format = "msi"`, per-machine install) and the
> results measured on this machine are in [MSI.md](MSI.md).

## Artifacts

A successful build writes the following into `[build] output_dir` (default `dist/`):

| File | Notes |
| ---- | ----- |
| `<name>-<version>-<arch>-setup.exe` | NSIS installer (`installer_format = "nsis"`, per-user install, no administrator rights required) |
| `<name>-<version>-<arch>-<locale>.msi` | MSI installer (`installer_format = "msi"`, per-machine install, one per locale) |
| `<name>-<version>-<arch>-portable.zip` | Portable build; not produced when `build_portable = false` |
| `.sha1` of each file above | SHA1 checksum file with the same name as the artifact |

`<arch>` comes from the host architecture (`x64` or `arm64`), `<version>` is the raw version
from `pubspec.yaml` (for example `1.0.0+1`).

Every entry in the portable ZIP sits under a single root directory
`<name>-<version>-<arch>/`, so extracting it does not scatter the application's files
across the extraction directory.

The installer's behaviour is defined by a pinned NSIS template and currently includes:

- installs to `%LOCALAPPDATA%\<name>` (per-user, no administrator rights) and writes the
  uninstall information to `HKCU`;
- the components page offers a desktop shortcut and a Start-menu shortcut;
- detects whether the application is running before installing or uninstalling, and checks
  for at least 500 MB of free space before installing;
- can launch the application immediately after installation if the box is checked;
- generates the localized installer UI and version information from `[app.i18n]`.

## Configuration

```toml
[platforms.windows]
enabled = true
installer_format = "nsis"        # nsis | msi
build_portable = true
# Signing (optional)
# signing_tool_path = "C:/Program Files (x86)/Windows Kits/10/bin/10.0.22621.0/x64/signtool.exe"
# certificate_path = "cert.pfx"
# certificate_password = "..."   # prefer the PACOR_WINDOWS_CERT_PASSWORD environment variable
# timestamp_url = "http://timestamp.digicert.com"
# MSI-specific (optional)
# msi_upgrade_code = "{11223344-5566-7788-99AA-BBCCDDEEFF00}"
# msi_codepage = 65001
```

## MSI installer

With `installer_format = "msi"`, every `[app.i18n]` locale produces its own
`<name>-<version>-<arch>-<locale>.msi` (`light` can only link one set of localization files
at a time, see [MSI.md](MSI.md)). The key differences from the NSIS package:

| | NSIS | MSI |
| --- | --- | --- |
| Install location | `%LOCALAPPDATA%\<name>` | `%ProgramFiles%\<name>` |
| Privileges | no administrator required | **administrator required** (`msiexec` asks for elevation) |
| Silent install/uninstall | `/S` stops at a dialog of the template | `/qn` runs without any dialog, the exit code is the result |
| Deployment | manual or scripted | GPO / Intune / SCCM / `msiexec` |

Prefer a single format per application: the two install to different locations and use
different registry locations, so they can coexist — but they leave two "Apps & features"
entries and two copies of the program.

MSI strings use **the code page of each locale** (936 for zh-CN, 950 for zh-TW, 932 for
ja-JP, 1252 for en-*), so the localized text must be encodable in that code page. `©` is not
in 936, so a zh-CN package that contains `©` fails with `LGHT0311`; in that case reword the
text (using `(C)` is the common choice) or set `msi_codepage = 65001` to switch to UTF-8.

### Code signing

Signing is enabled by configuring `certificate_path` or `signing_tool_path`, and then
`certificate_path` and the certificate password (`PACOR_WINDOWS_CERT_PASSWORD` takes
precedence over `certificate_password`) are both required: a missing value fails the build
instead of silently skipping signing.

The signing tool is looked up in this order:

1. `signing_tool_path` (must exist);
2. `signtool.exe` on `PATH`;
3. `.../Windows Kits/<10|8.1>/bin/<version>/<x64|x86>/signtool.exe` in the Windows SDK,
   taking the highest version.

Signing uses `signtool`-style arguments:

```
signtool sign /fd SHA256 /td SHA256 /tr <timestamp_url> /f <certificate_path> /p <password> <binary>
```

`timestamp_url` defaults to `http://timestamp.digicert.com`; setting it to an empty string
disables timestamping (not recommended: a signature without a timestamp stops being valid
once the certificate expires).

`pacor check --target windows` validates the signing configuration as part of the same run
(does the certificate exist, is a password provided, can the signing tool be found), so
configuration problems surface before the build.

The signing order matters: **sign the application binary first, then compile the installer**.
NSIS embeds the file contents into the installer at compile time, so signing the installer
afterwards still leaves users with an unsigned program. The installer itself is signed as
well once it has been generated, and because signing rewrites the file, the `.sha1` is
produced only after signing has finished.

### Build mode

`[build] flutter_build_mode` (`release` / `debug` / `profile`, overridable with
`PACOR_BUILD_MODE`) decides `flutter build windows --<mode>` and which build directory is
read (`build/windows/<arch>/runner/<Release|Debug|Profile>`).
`release` and `profile` add `--obfuscate --split-debug-info=tmp/`, and the debug symbols go
to `tmp/` inside the project (removed by the cleanup step at the end of the build and kept
by `--skip-cleanup`; `pacor clean` only removes `dist/` and `build/`, not `tmp/`); `debug`
cannot be obfuscated, so those flags are not added.
The Android backend does not use a build mode (always release, no obfuscation, symbols go to
`tmp/` as well), see [ANDROID.md](ANDROID.md).

## Detecting the executable and the copyright information

The executable name Flutter generates comes from `BINARY_NAME` in `windows/CMakeLists.txt`;
`flutter create` fills it in from the project name, but the two can disagree after a rename,
so pacor tries, in order:

1. `BINARY_NAME` from `windows/CMakeLists.txt`;
2. the project name from `pubspec.yaml`;
3. the only `.exe` in the build directory.

If none of them matches (because the directory holds several `.exe` files, for example), the
error lists the candidates.

`LegalCopyright` in `Runner.rc` is only a fallback for `[app.i18n.<locale>] copyright`, and
failing to read it does not abort the build; the file can be UTF-8 (with a BOM) or the UTF-16
that Visual Studio saves.

## NSIS toolchain and cache

The order in which `makensis` is selected:

1. `makensis` on `PATH`, provided it looks like an NSIS 3 installation (the
   `Plugins\x86-unicode` directory that ships with NSIS 3 exists) and that directory is
   writable, so plugins can be placed in it;
2. the NSIS 3 that pacor caches itself (downloaded from tauri-apps' `nsis-3` release), which
   is always writable.

If the system-wide NSIS needs administrator rights to write to its plugin directory, or is in
fact NSIS 2 (which cannot compile a `Unicode True` template), pacor warns and uses its own
cache instead of failing the build.

The cache lives in `<user config directory>/pacor/windows/nsis/<template tag>`, which on
Windows is `%APPDATA%\pacor\windows\nsis\1.1\`. That directory holds both the NSIS
distribution and the templates for the pinned tag:

```
makensis.exe                  # NSIS distribution (downloaded when needed)
template.nsi                  # installer script template
Plugins/nsProcess.dll         # plugin downloaded from the template tag (original location)
Plugins/nsProcess.nsh
languages/<NSIS language>.nsh     # SECTION_*/DESC_*/MSG_* strings per language
```

The templates are versioned together with the code that renders them, and the tag is pinned
in `TEMPLATE_REF` in
[`crates/pacor-core/src/tools.rs`](../crates/pacor-core/src/tools.rs)
(GNU-style version number, currently `1.1`; the repository is
[pacor-rs/tools](https://github.com/pacor-rs/tools)). `pacor cache clean` deletes the whole
cache, and the next build downloads it again.

When the cached NSIS is used, that directory is `$NSISDIR` and the plugin is installed to
`%APPDATA%\pacor\windows\nsis\<template tag>\Plugins\x86-unicode\nsProcess.dll`; when the
system-wide NSIS is used, the plugin is installed into the corresponding directory of that
installation.

### The MSI toolchain cache

MSI uses a pinned version of WiX (`wix314-binaries.zip`, about 39 MB, unpacked and used
as-is, with no dependency on an installed .NET SDK or installer), cached in
`%APPDATA%\pacor\windows\wix\1.1\` and structured like the NSIS one:

```
candle.exe / light.exe / ...   # WiX toolchain (downloaded when needed)
main.wxs                       # product, UI, upgrade and shortcut template
culture.wxl.tmpl               # localization template, rendered once per language
.extracted                     # marker that the toolchain has been extracted
```

The templates and the toolchain share one tag directory: a tag change invalidates both.
Before the first MSI build, `pacor check` says that the toolchain will be downloaded; seeding
it offline works the same way as for NSIS (place the files and create `.extracted`).

### Three key locations when compiling the installer

These three are what is worth checking first when a build fails:

1. `template.nsi` is read by pacor while it generates the script;
2. `nsProcess.nsh` is referenced by the template through `!include "nsProcess.nsh"`, so pacor
   copies it next to **the temporary directory that holds the generated script** (`!include`
   looks in the script's directory first);
3. `nsProcess.dll` must be in `$NSISDIR\Plugins\x86-unicode\` of `makensis`.
   The template uses `Unicode True`, and `makensis` looks for plugins only in that directory;
   putting it in the script directory or in the cache will not be found.

In the same way, the `LangString` entries the template references (`SECTION_MAIN`, `DESC_*`,
`MSG_*`) are defined by `languages/<language>.nsh` from the template tag (the ones NSIS ships
are `Contrib\Language files\*.nsh`, which only provide the generic MUI strings), so the
rendered `!include` points at those files in the cache and must be an absolute path.

### Language mapping

| `[app.i18n]` locale | NSIS language |
| ------------------- | ------------- |
| `en-US`, `en-GB` | `English` |
| `zh-CN` | `SimpChinese` |
| `zh-TW` | `TradChinese` |
| `ja-JP` | `Japanese` |

The default locale's strings are included first and the rest follow in locale order, so the
installer's language list is stable.

### Version numbers

The version fields inside an installer accept four numeric components only, while the
pubspec version is semver. pacor normalizes them: `1.0.0+1` → `1.0.0.1`, `1.0.0` → `1.0.0.0`,
`1.2.0-beta.1+7` → `1.2.0.7`, and a component above 65535 is clamped. NSIS's
`VIProductVersion` and MSI's `ProductVersion` share this normalization; the installer UI and
the uninstall information still show the original `1.0.0+1`.

## Install and uninstall (measured on this machine)

The following behaviour was verified by actually installing and uninstalling a real
`flutter create` sample project (against the pinned template `1.1`):

- installs to `%LOCALAPPDATA%\<name>` and creates localized shortcuts: a desktop
  `$(APP_DISPLAY_NAME).lnk` and a Start-menu folder `<name>\` (holding an application and an
  uninstall shortcut); the installed program runs directly;
- uninstalling removes the install directory, the desktop and Start-menu shortcuts, and the
  "Apps & features" entry;
- **the template's `MessageBox` is not suppressed in silent mode**, so `setup.exe /S` and
  `Uninstall.exe /S` both stop at a dialog once the actual work is done and the process does
  not exit: the install stops at `Function .onInstSuccess` asking whether to launch the
  application, the uninstall at the "uninstall complete" dialog at the end of its section.
  When calling them from a script, decide by **file system state** (does the install
  directory appear or disappear) instead of waiting for the process to exit, and terminate
  the process once the result is confirmed;
- `Uninstall.exe /S` copies itself to `%TEMP%\~nsu.tmp\Un_A.exe` and runs from there, so the
  process it starts returns after about 2 seconds while the real deletion continues in the
  background (measured: tens of seconds);
- after uninstalling, an `HKCU\Software\<name>` key holding only the two values `InstallDir`
  and `Version` may remain: the template cleans it up with `DeleteRegKey /ifempty`, but the
  key still has values, so it is not deleted. This is the behaviour of the pinned template
  `1.1`; the key can be deleted by hand.

## Cleanup

- `pacor clean`: removes the output directory and the Flutter build artifacts (with
  `--target`, only that target's build directory and artifacts); the debug symbols in `tmp/`
  are removed by the build's own cleanup step unless `--skip-cleanup` is given;
- `pacor cache clean [--dry-run]`: removes the shared cache under `%APPDATA%\pacor`
  (including NSIS, WiX and templates).

## Troubleshooting

### `Plugin not found, cannot call nsProcess::_FindProcess`

`nsProcess.dll` was not placed in `$NSISDIR\Plugins\x86-unicode\` of `makensis`.
This happens with a system-wide NSIS whose directory is not writable; pacor falls back to its
own cache and rebuilds, and if the error still appears, delete the cache
(`pacor cache clean`) and retry.

### `Error: invalid VIProductVersion format, should be X.X.X.X`

The version in `pubspec.yaml` contains a non-numeric component. pacor already normalizes
`+build` and pre-release suffixes, so if the error persists, check whether the version itself
follows pubspec's semver rules.

### `No application executable found in ...`

There is no `.exe` in the build directory, or the directory does not exist. With
`--use-existing-flutter-build` you have to run `flutter build windows` first; you can also
check whether `BINARY_NAME` in `windows/CMakeLists.txt` matches the project name in
`pubspec.yaml`.

### The installer icon is missing

`windows/runner/resources/app_icon.ico` is the icon of the installer and of the shortcuts and
must exist; `flutter create --platforms=windows .` regenerates it.

### `signtool` cannot be found

Install the Windows SDK, or set `platforms.windows.signing_tool_path` to an existing
`signtool.exe`. pacor looks in the SDK's `…\Windows Kits\<10|8.1>\bin\<version>\x64\`, so
configuring it by hand is usually unnecessary.

### The browser can open GitHub, but pacor's download fails

pacor's download client only reads the `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` environment
variables and **does not read Windows' PAC automatic configuration script or the system proxy
settings**, while the browser reads the latter. So "the browser can reach it" does not mean
pacor can.

First read the PAC address from Internet Options → Connections → LAN settings, find the actual
proxy in the PAC, then export it in the terminal that runs pacor:

```powershell
$env:HTTPS_PROXY = "http://127.0.0.1:1086"
$env:HTTP_PROXY  = "http://127.0.0.1:1086"
```

Note: **pacor does not use a proxy when a mirror is configured**; the two cannot be combined.
When the proxy can reach GitHub directly, using the proxy and setting no mirror is the best
combination — measured on this machine with a cold cache, the templates took 2 s and the
39 MB WiX toolchain 26 s to download, after which both the MSI and the NSIS build finished
within a minute. For details see
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md#http_proxy--https_proxy-standard-proxy-variables).

### Downloading NSIS or the templates fails (restricted network)

The download addresses pacor pins are on `github.com` and `raw.githubusercontent.com`, which
may be unreachable on a restricted network. Configure a mirror as described in
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md):

- templates (raw links): `PACOR_GITHUB_RAW_MIRROR_TEMPLATE`;
- the NSIS distribution (release asset): `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` or
  `PACOR_GITHUB_MIRROR_BASE_URL`;
- the WiX toolchain is a release asset as well (`wix3141rtm` from `wixtoolset/wix3`), so it
  goes through the same two mirror variables.

One variable that covers every download:

```powershell
$env:PACOR_GITHUB_MIRROR_BASE_URL = "https://gh.my-website.ccwu.cc"
```

Two things measured on this machine (details in
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md#two-things-to-watch-when-choosing-a-mirror-measured)):

- the template files are only a few KB, so any mirror that connects at all is good enough; the
  NSIS distribution is about 2 MB and the whole cold-start chain measured under a minute;
- **the WiX toolchain is about 39 MB and that is what tests a mirror**: a mirror with too
  little bandwidth or one that drops mid-transfer interrupts the download; pacor keeps the
  bytes already received and resumes from there on the next attempt (up to 4 attempts), so a
  single drop no longer fails the build. What genuinely does not work is a mirror that cannot
  deliver the file at all — on such a network, seeding the cache offline is less trouble (see
  below).

You can also seed the cache offline: place the NSIS distribution and the templates in
`%APPDATA%\pacor\windows\nsis\<template tag>\` following the layout above, and create an empty
`.extracted` marker file; existing template files are skipped anyway, and the NSIS download is
skipped because of the marker. The same applies to MSI's
`%APPDATA%\pacor\windows\wix\<template tag>\` (place the extracted WiX toolchain, `main.wxs`,
`culture.wxl.tmpl` and `.extracted`).

### `error LGHT0311 ... database code page '936'` (MSI)

Some localized text contains a character that the locale's code page cannot represent. The
usual one is `©` (not in CP936; zh-TW's CP950 only degrades it to `c`). Two ways to handle it:

1. reword the text, for example `版权所有 (C) 2025 ...` — recommended;
2. set `msi_codepage = 65001` so the whole package uses UTF-8.

`light` reports the line number that fails but not which character it is.

### `Error 1925: You do not have sufficient privileges ...` (MSI)

An MSI is a per-machine package, so `msiexec /i` needs administrator rights; in a
non-elevated session (an automation account, for example) it fails at `InstallFinalize` with
exit code 1603 and rolls back cleanly. Building an MSI does not need elevation, only
installing it does. The way to verify without elevation is an administrative install
(unpacking):

```powershell
msiexec /a .\dist\<name>-<version>-<arch>-<locale>.msi /qn TARGETDIR=C:\temp\unpacked
```

It unpacks the files in the package according to the install layout, which makes a
file-by-file comparison with `build\windows\x64\runner\Release` possible.

## Development verification

`pacor-packaging-windows` ships several end-to-end tests that are ignored by default: two use
a real `makensis` (whether the rendered script compiles, and the whole `NsisBuilder::build`
flow), and another builds two-language MSIs with a real WiX and compares them file by file
with the build directory using `msiexec /a`:

```powershell
$env:PACOR_NSIS_TOOLCHAIN_DIR = "$env:TEMP\pacor-nsis"
$env:PACOR_MSI_TOOLCHAIN_DIR = "$env:APPDATA\pacor\windows\wix\1.1"
cargo test -p pacor-packaging-windows -- --ignored
```

The NSIS directory must contain `makensis.exe` (from
<https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip>, with the
top-level `nsis-3.08/` directory removed after extraction), the template tag's `template.nsi`,
`Plugins/nsProcess.nsh`, `Plugins/nsProcess.dll` and `languages/*.nsh` (from the matching tag
of <https://github.com/pacor-rs/tools>). The WiX directory must contain `candle.exe`,
`light.exe`, `WixUIExtension.dll` and the template tag's `main.wxs` and `culture.wxl.tmpl`.
