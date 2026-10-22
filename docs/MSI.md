<p align='center'>
<span>English</span> | <a href="MSI.zh-CN.md">中文</a>
</p>

# Pacor MSI installer design

> Status: **implemented** (M1 done). `installer_format = "msi"` produces one per-machine
> MSI for each `[app.i18n]` language. This document records the toolchain choice, the
> contract, the measured conclusions and the follow-up plan; usage is in
> [WINDOWS.md](WINDOWS.md#msi-installer).
>
> Scope: **per-machine only**. By project decision, **per-user installation is not implemented**
> — which also removes the pile of MSI constraints that only per-user needs (see 7.5).

## 1. Goals

Add MSI artifacts to the existing Windows backend ([docs/WINDOWS.md](WINDOWS.md)), for enterprise
deployments through GPO / Intune / SCCM, with an installation that is fully scriptable:

| Goal | Description |
| ---- | ---- |
| Single-file artifact | `<name>-<version>-<arch>.msi` + `.sha1`, CAB embedded |
| Silent, automatable | `msiexec /i /qn`, `/x /qn`, `/a` extraction, no dialog at any point |
| Standard uninstall | the ARP entry, uninstall and repair are the Windows Installer's job |
| Upgradable | versions upgrade under the same `UpgradeCode`, without the user uninstalling first |
| Localization | installer UI and product information follow `[app.i18n]` |
| Signing | reuses the existing `signing.rs`: sign the main executable, then the MSI |

## 2. Relationship to NSIS

The two coexist and are configured independently, but **one application should pick only one**:

| | NSIS (existing) | MSI (this design) |
| --- | --- | --- |
| Install location | `%LOCALAPPDATA%\<name>` (per-user) | `%ProgramFiles%\<name>` (per-machine) |
| Privileges | administrator not needed | **administrator required** (UAC elevation) |
| Registry | `HKCU\Software\<name>` + HKCU uninstall entry | `HKLM\Software\<name>` + HKLM uninstall entry |
| Silent install | `/S` finishes the install, but the template's `MessageBox` is not suppressed and the process stops on the dialog | `/qn` has no dialog at all; the exit code is the result |
| Deployment | manual / scripts | GPO / Intune / SCCM / `msiexec` |
| Best for | self-service installation by users, no administrator | centralized enterprise deployment |

Because the install and registry locations differ, the two packages can coexist on the same machine
(neither overwrites the other), but that leaves two "Apps & features" entries and two copies of the
program. The documentation and `pacor check` should point this out; an optional follow-up is a
`LaunchCondition` that refuses to install when the NSIS version is already there
(`HKCU\Software\<name>`).

## 3. Toolchain: pin WiX v3.14

Following the NSIS approach: **download a pinned version into the cache directory, and depend on
nothing installed on the host**.

| Option | Conclusion |
| --- | --- |
| **WiX v3.14.1 binaries** (adopted) | `wix314-binaries.zip` (39.4 MB, 2024-03-23, MS-RL) unpacks straight into `candle.exe`/`light.exe`/`heat.exe` and `WixUIExtension.dll`; **no .NET SDK and no installer needed**; WixUI's localized strings are embedded in the extension (the zip contains no `.wxl` at all) |
| WiX v7 (2026-04) | actively maintained, but only ships `wix-cli-x64.msi` (12.4 MB) or NuGet packages, which needs a CLI / .NET SDK installation plus an online restore and breaks the "self-contained in the cache" model; the template dialect differs as well (`<Package>`/wixext). As a future migration target, the blast radius is isolated to the toolchain provider and the templates |
| msitools `wixl` | a subset of the WiX dialect, **no WixUI extension** (no standard UI and no localization), and no official pinned Windows binary |
| hand-rolling the MSI (constructing the OLE compound document and the MSI tables yourself) | you own CAB, InstallExecuteSequence and the component rules, with no ICE validation as a safety net |

Cache layout (isomorphic to NSIS, the tag is part of the path):

```
<user config dir>/pacor/windows/wix/<template tag>/   # unpacked WiX binaries
<user config dir>/pacor/windows/nsis/<template tag>/  # existing NSIS + templates
```

`pacor cache clean` removes both.

## 4. Template contract: a new tag `1.1` (published)

Template tags are immutable (and use GNU-style version numbers, see
[README](../README.md#build-templates)); MSI needs new template files, so `1.1` was published:

```
windows/wix/main.wxs           # product, UI, upgrade, shortcuts, ARP; contains {{placeholders}}
windows/wix/culture.wxl.tmpl   # rendered once per language as <culture>.wxl
```

The content of `1.1` = `1.0` (the historical `v1`) with `windows/nsis/**` and `linux/**`
**byte-identical** (the SHA of all 11 blobs matches `v1`; verify with
`gh api repos/pacor-rs/tools/compare/v1...1.1`) + the additions above + the WiX contract notes
added to the README. Once `TEMPLATE_REF` moves from `v1` to `1.1`, the changed cache path
invalidates the old NSIS cache and downloads it again — which is the intent (one pacor version
recognises exactly one template contract).

Authoritative copies of both templates also live in this repository under `tools/windows/wix/`
(the same arrangement as `tools/macos/`); edit those, then publish a new tag as described above.

Placeholders (`render_template` as before; an unreplaced placeholder fails the build):

`main.wxs` (**rendered once per language**, because `Product/@Language` cannot be localized):

| Placeholder | Source |
| ------ | ---- |
| `{{APP_NAME}}` | project name from `pubspec.yaml` (install directory name, registry key) |
| `{{PRODUCT_VERSION}}` | normalized four-part numeric version (see 5.2) |
| `{{PRODUCT_LANGUAGE}}` | `Locale::as_windows_lcid()`, e.g. `2052` |
| `{{APP_EXECUTABLE}}` | runner executable file name (reuses `runner.rs`) |
| `{{APP_URL}}` | `[app].url` |
| `{{APP_ICON_PATH}}` | `windows/runner/resources/app_icon.ico` |
| `{{UPGRADE_CODE}}` | `msi_upgrade_code`, or derived as in 5.3 |
| `{{INSTALL_DIR_COMPONENT_GUID}}` `{{START_MENU_COMPONENT_GUID}}` `{{DESKTOP_COMPONENT_GUID}}` | UUIDv5 of the template's own components (prefixed with the UpgradeCode) |
| `{{PAYLOAD_COMPONENT_GROUP}}` | id of the component group the harvester defines (`AppPayload`) |

The build output directory does not go through a placeholder: it is passed as
`candle -dSourceDir=` and referenced from the generated `payload.wxs` as
`$(var.SourceDir)\…`, so the generated file carries no machine-specific paths (see 7.1).

`culture.wxl` (rendered once per language, carrying every user-facing string):

| Placeholder | Source |
| ------ | ---- |
| `{{CULTURE}}` | `Locale::as_wix_culture()`, e.g. `zh-cn` |
| `{{MSI_CODEPAGE}}` | `msi_codepage`, or `Locale::as_msi_codepage()` (see 5.4) |
| `{{APP_DISPLAY_NAME}}` `{{APP_PUBLISHER}}` `{{APP_DESCRIPTION}}` `{{APP_COPYRIGHT}}` | the language's `[app.i18n.<locale>]`, falling back to the default language when missing |
| `{{FEATURE_MAIN}}` `{{UNINSTALL_SHORTCUT}}` `{{DOWNGRADE_ERROR}}` | strings built into pacor (a small table in `msi/generator.rs`, referenced from `main.wxs` as `!(loc.…)`) |

WixUI's own UI strings (Welcome, Next and so on) are supplied by the extension via `-cultures:` and
are not in the template.

### 4.1 Multiple languages: one MSI per language

Measured on this machine: `light -cultures:en-us;zh-cn` with two `-loc` arguments reports
`LGHT0100: The localization identifier 'ProductName' has been duplicated` (light accepts only one
set of localizations at a time). Therefore:

- one MSI per language: `<name>-<version>-<arch>-<locale>.msi` (the same approach tauri takes);
- the language code uses `Locale::as_wix_culture()` (new; `en-US` → `en-us`, `zh-CN` → `zh-cn`,
  symmetrical with the existing `as_apple()`/`as_posix()`; existing tests already guard this
  "one spelling per consumer" convention);
- multi-language in a single file needs `torch`/`pyro` language transforms or a Burn bootstrapper,
  which is scheduled for M3.

## 5. Configuration

```toml
[platforms.windows]
installer_format = "msi"       # nsis | msi
# msi_upgrade_code = "..."     # optional; derived automatically by default
# msi_codepage = 65001         # optional; defaults to the locale's own code page
```

### 5.1 No `install_scope`

Because this is per-machine only, **no `install_scope` configuration key is introduced**: the MSI is
always per-machine and NSIS is always per-user. That avoids a switch that would only apply to one
format.

### 5.2 Version number

`VIProductVersion` and the MSI `ProductVersion` both accept only four numeric parts, so the existing
`nsis_product_version` from `nsis/generator.rs` was promoted to
`pacor_core::packaging::windows_version()`, shared by both artifact lines:

- `1.0.0+1` → `1.0.0.1`; `1.0.0` → `1.0.0.0`; `1.2.0-beta.1+7` → `1.2.0.7`; anything above 65535 is
  clamped;
- the MSI sets `AllowSameVersionUpgrades="yes"`: otherwise changing only `+build` (which affects
  only the fourth part, while the MSI compares just the first three) would not trigger an upgrade.

### 5.3 Default derivation of `msi_upgrade_code`

The `UpgradeCode` must stay stable across versions. When it is not configured it is derived with
UUIDv5, so the user never has to invent a GUID:

```
uuid_v5(NAMESPACE, "pacor:windows:msi:<name>:<publisher>")
```

`sha1` is already a dependency of `pacor-core`, so UUIDv5 needs no new dependency; the
implementation follows the RFC 4122 byte order and is unit tested against the RFC's DNS vector
(`python.org` → `886313e1-3b8a-5372-9b90-0c9aee199e5d`). Renaming the app or changing the publisher
changes the UpgradeCode — documented, and it can be pinned with `msi_upgrade_code`.

### 5.4 `msi_codepage`: the string code page

A Windows Installer package stores all its strings in **one** code page, and WiX derives it from the
culture by default. pacor writes it explicitly, because it explains `LGHT0311` and is also the way
out of it:

| locale | Code page |
| ------ | ------ |
| `en-US` `en-GB` | 1252 (Windows-1252) |
| `zh-CN` | 936 (GBK) |
| `zh-TW` | 950 (Big5) |
| `ja-JP` | 932 (Shift-JIS) |

Measured on this machine: `©` (U+00A9) is not in CP936 (CP950 only degrades it to `c`), so a zh-CN
package containing `版权所有 © 2025 …` fails with
`LGHT0311: A string was provided with characters that are not available in the specified
database code page '936'`. There are two ways to handle it:

1. rewrite the text (`版权所有 (C) 2025 …`) — recommended, because the MSI code-page handling is not
   suited to carrying arbitrary Unicode anyway;
2. `msi_codepage = 65001` switches to UTF-8. Measured on this machine: the package passes ICE
   validation, `msiexec /a` extracts it correctly, and a Chinese `ProductName` reads back correctly.
   The code page is written on `WixLocalization/@Codepage` rather than on `Product`, because
   `main.wxs` is shared by several languages while a `.wxl` is already per language.

## 6. Packaging pipeline

```
flutter build windows --<mode>
  → locate the executable (reuses runner.rs)
  → sign the main executable (NSIS requires it too: the MSI embeds the file contents in the CAB)
  → harvest(build output)                 → payload.wxs
  → render main.wxs per language (with the LCID) + <culture>.wxl
  → candle -arch <x64|arm64> payload.wxs  → payload.wixobj (shared by all languages)
  → candle main.wxs per language          → <culture>-main.wixobj
  → per language: light -ext WixUIExtension -cultures:<locale> -loc <culture>.wxl
          -sice:ICE60 -sice:ICE61 -pdbout <temp dir>
  → check that light exits 0 (an ICE error counts as failure, see 8.1)
  → sign the MSI → write the .sha1
```

By default `light` writes a `.wixpdb` binding database next to the artifact; `-pdbout` moves it into
a temporary directory, otherwise it is treated as a user-visible artifact (a useless file shows up
in `dist/`).

Signing does not happen inside `MsiBuilder`: `lib.rs` builds every installer first, then signs them
all and regenerates the checksums (signing rewrites the file, and NSIS and MSI take the same path).

The code structure mirrors `nsis/`:

```
crates/pacor-packaging-windows/src/
├── msi/
│   ├── mod.rs        MsiBuilder: toolchain setup, candle/light orchestration, artifacts and checksums
│   ├── toolchain.rs  pinned wix314-binaries.zip + templates → cache
│   ├── harvest.rs    build output → directory tree + one component per file (pure function, the unit-test focus)
│   └── generator.rs  main.wxs / culture.wxl rendering + built-in UI strings
pacor-core:
    packaging::windows_version(), deterministic_uuid_v5()
    i18n::Locale::as_wix_culture(), as_windows_lcid(), as_msi_codepage()
    config::WindowsConfig::resolved_upgrade_code(), resolved_msi_codepage()
```

Template skeleton (identical to `tools/windows/wix/main.wxs`; the `!(loc.…)` binding details are
omitted here):

```xml
<Product Id="*" Name="!(loc.ProductName)" Language="{{PRODUCT_LANGUAGE}}"
         Version="{{PRODUCT_VERSION}}" Manufacturer="!(loc.Manufacturer)"
         UpgradeCode="{{UPGRADE_CODE}}">
  <Package InstallerVersion="500" Compressed="yes" Platform="x64"
           InstallScope="perMachine" InstallPrivileges="elevated" />
  <MajorUpgrade AllowSameVersionUpgrades="yes" DowngradeErrorMessage="!(loc.DowngradeError)" />
  <MediaTemplate EmbedCab="yes" CompressionLevel="high" />
  <Icon Id="AppIcon.ico" SourceFile="{{APP_ICON_PATH}}" />
  <Property Id="ARPPRODUCTICON" Value="AppIcon.ico" />
  <Property Id="ARPURLINFOABOUT" Value="{{APP_URL}}" />
  <Property Id="ARPCOMMENTS" Value="!(loc.LegalCopyright)" />
  <UIRef Id="WixUI_InstallDir" />
  <Property Id="WIXUI_INSTALLDIR" Value="INSTALLDIR" />
  <Directory Id="TARGETDIR" Name="SourceDir">
    <Directory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLDIR" Name="{{APP_NAME}}">
        <Component Id="cmp_INSTALLDIR" Guid="{{INSTALL_DIR_COMPONENT_GUID}}">
          <RemoveFolder Id="RemoveINSTALLDIR" On="uninstall" />
          <RegistryValue Root="HKLM" Key="Software\{{APP_NAME}}" Name="InstallDir"
                         Type="string" Value="[INSTALLDIR]" KeyPath="yes" />
        </Component>
      </Directory>
    </Directory>
    <Directory Id="ProgramMenuFolder">
      <Directory Id="ApplicationProgramsFolder" Name="!(loc.ProductName)" />
    </Directory>
    <Directory Id="DesktopFolder" />
  </Directory>
  <Feature Id="MainFeature" Title="!(loc.FeatureMain)" Level="1">
    <ComponentGroupRef Id="{{PAYLOAD_COMPONENT_GROUP}}" />
    <ComponentRef Id="cmp_INSTALLDIR" />
    <ComponentRef Id="cmp_StartMenuShortcut" />
    <ComponentRef Id="cmp_DesktopShortcut" />
  </Feature>
</Product>
```

The two shortcut components live under `DirectoryRef Id="ApplicationProgramsFolder"` and
`DirectoryRef Id="DesktopFolder"` respectively, and use an HKCU registry value as KeyPath (the
reason is in 7.2); they are omitted here.

`payload.wxs` is generated by the harvester: `<Directory>` elements nest under
`<DirectoryRef Id="INSTALLDIR">` following the directory hierarchy of the build output, one
`<Component Guid="{uuid_v5}"><File KeyPath="yes"/></Component>` per file, and a final
`<ComponentGroup>` collects all component ids (**every component must be referenced by a feature**,
otherwise `LGHT0267`).

## 7. MSI semantics worth knowing

### 7.1 Hard constraints on the harvester (all hit in practice)

| Constraint | Reason |
| ---- | ---- |
| identifiers ≤ 72 characters, only `[A-Za-z0-9_.]` (**no `-`**) | `CNDL1026`; deep paths such as `data/flutter_assets/packages/cupertino_icons/assets/CupertinoIcons.ttf` exceed the limit. The fix: truncate the basename + the first 8 hex digits of a UUIDv5 |
| one component per file | the WiX/MSI component rules; the `File` is the KeyPath |
| every component must be referenced by a feature | `LGHT0267` orphaned component |
| component GUIDs should be deterministic | `heat -gg` changes the GUID on every build, which breaks the predictability of upgrade/repair; UUIDv5(relative path) is stable |
| `Source` uses `$(var.SourceDir)\relative\path` | the generated file contains no machine-specific absolute paths, which makes the build more reproducible and keeps the GUIDs independent of the checkout location |

### 7.2 Shortcut components need an HKCU KeyPath

Even though the whole package is per-machine, ICE still treats shortcut components under
`ProgramMenuFolder`/`DesktopFolder` as "user profile" components: **the KeyPath must be in HKCU**
(`ICE38` + `ICE43`, measured on this machine). Therefore:

- payload components (Program Files): the file is the KeyPath;
- shortcut components: `<RegistryValue Root="HKCU" … KeyPath="yes" />` + `<RemoveFolder On="uninstall">`;
- under a per-machine installation, `ProgramMenuFolder`/`DesktopFolder` resolve to the "all users"
  Start menu and desktop.

### 7.3 ARP and the icon

- `DisplayName` / `Publisher` / `DisplayVersion` / `EstimatedSize` are written automatically by
  Windows Installer to `HKLM\...\Uninstall\<ProductCode>`, so there is nothing to author by hand
  (the registry block in the NSIS template can be dropped);
- writing `[INSTALLDIR]` into `ARPINSTALLLOCATION` directly triggers `CNDL1077` (a property value
  cannot reference a property); it needs a type 51 custom action before `CostFinalize`, or simply
  is not set at all;
- ARP properties such as `ARPNOMODIFY` are set by the `WixUI_InstallDir` extension itself, and
  authoring them again yields `LGHT0091 Duplicate symbol 'Property:ARPNOMODIFY'` (measured), so the
  template must not write them.

### 7.4 Bitness and privileges

- without `-arch x64`, candle emits an **Intel (32-bit) package**: measured on this machine, the
  ARP entry lands in `HKLM\Software\WOW6432Node\...\Uninstall` (not the native view); installing
  components into a 64-bit directory also triggers ICE80 (a known rule, which this design avoids by
  always passing `-arch`). The value is the payload's own architecture — `x64` or `arm64`, the
  spelling candle expects, taken from the same `Arch` that names the artifact and the build
  directory — so an ARM64 build produces a package Windows registers as ARM64 rather than an Intel
  one it would install and repair as 32-bit-on-ARM;
- a per-machine installation **needs administrator**: in a non-elevated session `msiexec /i /qn`
  fails at `InstallFinalize` and the log gives `Error 1925: You do not have sufficient privileges to complete this
  installation for all users of the machine` (measured, exit code 1603, rolled back cleanly).
  Building the MSI does not need elevation; only installing it does.

### 7.5 Constraints only per-user installations bring

A per-user installation brings two extra hard constraints; **this design is unaffected because it is
per-machine only**, but they are recorded here in case per-user is revisited later:

- `ICE38`: **every component** installed into a user profile directory must use an HKCU registry
  value as its KeyPath (a file will not do) — which means one more HKCU value per file;
- `ICE64`: **every directory** under a user profile directory must be listed in the `RemoveFile`
  table;
- `ICE91` (files under a per-user directory are not copied by a per-machine installation) also needs
  an explicit suppression.

## 8. Verification plan

### 8.1 What counts as success

`light` **writes the MSI first and reports ICE errors afterwards** (measured: in a failing run the
artifact still exists while the exit code is non-zero). Success therefore has to mean "exit code 0
**and** zero ICE errors"; whether the file exists is not enough.

Only two ICE checks are whitelisted; every other ICE is a failure:

| ICE | Handling | Reason |
| --- | ---- | ---- |
| `ICE60` | `-sice:ICE60` | the `.otf` fonts in Flutter's assets carry no Language column, which has nothing to do with this package |
| `ICE61` | `-sice:ICE61` | `AllowSameVersionUpgrades="yes"` necessarily triggers it (VersionMax equals the current version) |

### 8.2 Unit tests

Covered: version normalization and clamping, UUIDv5 (including the RFC 4122 vector and stability),
identifier sanitization/truncation/deduplication, directory tree and component generation
(including "every component is referenced by a feature"), XML escaping, the empty-directory
warning, placeholder rendering and escaping,
`as_wix_culture`/`as_windows_lcid`/`as_msi_codepage`, UpgradeCode derivation, code-page resolution,
the ICE whitelist and the cache paths.

### 8.3 End to end (opt-in, following the NSIS pattern)

`PACOR_MSI_TOOLCHAIN_DIR` points at a prepared WiX directory (see the appendix);
`an_msi_is_built_end_to_end` builds an MSI in two languages from a synthetic payload and checks:

1. `light` exits 0 and ICE errors are 0 (the `light` exit code is the only criterion, see 8.1);
2. the artifact name carries `<locale>`, the `.sha1` exists, and no `.wixpdb` leaked into the output
   directory;
3. `msiexec /a <msi> /qn TARGETDIR=<tmp>` administrative installation → a **file-by-file
   comparison** with the build output (this one needs no elevation, so it suits CI);

A manual verification can additionally cover:

4. in an elevated session, `msiexec /i /qn` → assert `%ProgramFiles%\<name>`, the `HKLM` ARP entry
   (read the **64-bit** registry view), and the all-users desktop/Start menu shortcuts;
5. `msiexec /x /qn` → assert that the directory, the ARP entry and the shortcuts are all gone.

Steps 4 and 5 need an administrator session and cannot run on this machine (non-elevated), so they
are scheduled as M2.5.

## 9. Staged delivery

| Stage | Content | Status |
| ---- | ---- | ---- |
| M0 | pin WiX, harvester prototype, the per-machine recipe, ICE down to zero, the `/a` comparison, the privilege and bitness conclusions | **done** (scripts in the appendix) |
| M1 | the `[platforms.windows]` configuration + the `msi/` module + template `1.1` (published) + one MSI per language + signing + `pacor check` hinting at the toolchain + documentation | **done** (`6d56558`) |
| M2 | unit tests (harvester/generator/toolchain/code page/Lcid) + the opt-in end-to-end test (real candle/light + the `/a` comparison) | **done** |
| M2.5 | re-run `msiexec /i` / `/x` in an elevated session and confirm where the ARP entry and the shortcuts land | to do (this machine cannot elevate) |
| M3 | optional: an `installer_formats` set (produce NSIS and MSI together), language transforms for a single multi-language file, coexistence detection with the NSIS version (`LaunchCondition`), the VC++ runtime merge module | undecided |

## 10. Measured conclusions

Measured on the example project `pacor-demo` (Flutter 3.32.8, `version: 1.0.0+1`, `[app.i18n]` with
en-US + zh-CN) using WiX 3.14.1.8722:

| Conclusion | Evidence |
| ---- | ---- |
| the WiX binaries are self-contained, nothing to install | `candle/light` run directly and print `version 3.14.1.8722` |
| the hand-written harvester passes ICE | 12 files → 12 components, `light exit=0`, ICE errors=0 |
| deep-path ids exceed the limit | `CNDL1026` (78 characters); after truncation + a hex suffix, zero warnings |
| identifiers cannot contain `-` | candle fails when an id uses a hyphen, and passes once it uses `.` |
| a shortcut component's KeyPath must be HKCU | with HKLM, `ICE38` + `ICE43` |
| `ARPNOMODIFY` conflicts with WixUI | `LGHT0091 Duplicate symbol 'Property:ARPNOMODIFY'` |
| `ARPINSTALLLOCATION=[INSTALLDIR]` is invalid | `CNDL1077` |
| multiple `-loc` arguments conflict | `-cultures:en-us;zh-cn` + two `.wxl` files → `LGHT0100` |
| the 32-bit/64-bit package difference | the default Intel package puts the ARP entry in `WOW6432Node`; with `-arch x64` it is in the native view (`Template=x64;…`) |
| light writes the artifact before reporting ICE | in the failing run the `msi` still exists, exit code 204 |
| the `/a` extraction can be used for verification | 12 files in each language's package, matching the build directory one for one |
| per-machine needs elevation | in a non-elevated session, `Error 1925`, exit code 1603, rolled back cleanly |
| `light` writes a `.wixpdb` next to the artifact by default | `-pdbout` has to redirect it, otherwise the file shows up in `dist/` |
| an MSI package has a single string code page | zh-CN text containing `©` → `LGHT0311` (CP936 has no `©`) |
| the package properties match the design | `UpgradeCode` identical in both languages, `ProductVersion=1.0.0.1`, `ALLUSERS=1` (i.e. per-machine), `ProductLanguage` 1033/2052 |

**Still unverified** (this machine is a non-elevated session): a full install/uninstall with
elevation, where the ARP entry and the shortcuts actually land, and the `MajorUpgrade` upgrade path
(install `1.0.0.0`, then install `1.0.0.1`). Those are the acceptance criteria for M2.5.

## 11. Risks and open questions

1. **WiX v3 is frozen** (the last release was 2024-03): the MSI format itself has not changed in
   years, so the risk is low; a future move to v7 would touch only `msi/toolchain.rs` and the
   template dialect.
2. **MSI builds are not reproducible**: `ProductCode="*"` plus a timestamp make the `.sha1` differ
   on every build (NSIS behaves the same way), so a release process must not assume the same input
   gives the same hash.
3. **VC++ runtime**: a Flutter Windows application depends on the MSVC runtime, which a clean system
   may lack. The MSI can solve it with the VC++ merge module or an app-local deployment; **the NSIS
   path is missing this step today as well**, so it should be decided for both at once.
4. **Coexistence with the NSIS version**: the install locations differ, so they can coexist but each
   has its own ARP entry; the coexistence check in M3 is an optional mitigation.
5. **One MSI per language** makes the number of artifacts grow with `[app.i18n]`; if that is
   unacceptable, language transforms have to arrive in M3.
6. **per-user is explicitly out of scope**: an enterprise deployment that needs per-user has to use
   the NSIS package instead (or reopen the design as a separate requirement, at the cost described
   in 7.5).

## Appendix: reproducing the spike

This spike was done on Windows, with the scripts in a temporary directory (not committed):

```
%TEMP%\wix314\            # unpacked from wix314-binaries.zip
%TEMP%\msi-spike\
  harvest.ps1             # the per-user harvester (with the HKCU KeyPath and RemoveFolder)
  harvest-machine.ps1     # the per-machine harvester (the recommended shape)
  main.wxs                # the per-user template (WixUI + ARP + shortcuts)
  main-machine.wxs        # the per-machine template (the recommended shape)
  en-US.wxl / zh-CN.wxl   # localization
  machine.ps1             # end to end: candle → light → /a comparison → /i → /x
```

Reproduction steps: download and unpack WiX → produce the build directory with
`flutter build windows --release` →
`harvest-machine.ps1 -SourceDir <build dir> -OutFile payload.wxs` →
compile both `.wxs` files with `candle -arch x64` (or `-arch arm64` on an ARM64 machine) → `light -ext WixUIExtension -cultures:<culture> -loc <culture>.wxl
-sice:ICE60 -sice:ICE61` → `msiexec /a` (extraction check) / `/i /qn` (needs elevation) / `/x /qn`.

WiX download: <https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip>.

The download is checked against a pinned SHA-256 (`WIX_ARCHIVE_SHA256` in
`crates/pacor-packaging-windows/src/msi/toolchain.rs`) before anything is unpacked, for the same
reason as the NSIS archive: a release asset can be replaced by its owner at any time, and
`candle.exe` and `light.exe` are then run on the building machine. To refresh the pin after an
intentional upstream change:

```bash
cargo test -p pacor-packaging-windows -- --ignored the_pinned_wix_digest_matches_the_published_archive
# or, without a checkout:
curl -sL https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip | sha256sum
```
