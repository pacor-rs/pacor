<p align='center'>
<a href="MSI.md">English</a> | <span>中文</span>
</p>

# Pacor MSI 安装器设计方案

> 状态：**已实现**（M1 完成）。`installer_format = "msi"` 会为每个 `[app.i18n]`
> 语言产出一个全机安装的 MSI。本文档记录选型、契约、实测结论与后续计划，
> 使用说明见 [WINDOWS.md](WINDOWS.zh-CN.md#msi-安装包)。
>
> 适用范围：**仅 per-machine（全机安装）**。按项目决定，**不实现 per-user 安装**——
> 这同时消掉了 per-user 才需要的一大堆 MSI 约束（见 7.5）。

## 1. 目标

在现有 Windows 后端（[docs/WINDOWS.md](WINDOWS.zh-CN.md)）之上增加 MSI 产物，供企业环境通过
GPO / Intune / SCCM 部署，并让安装过程可完全脚本化：

| 目标 | 说明 |
| ---- | ---- |
| 单文件产物 | `<name>-<version>-<arch>.msi` + `.sha1`，CAB 内嵌 |
| 静默可自动化 | `msiexec /i /qn`、`/x /qn`、`/a` 解包，全程无对话框 |
| 标准化卸载 | ARP 条目、卸载、修复都由 Windows Installer 负责 |
| 可升级 | 同一 `UpgradeCode` 下按版本升级，无需用户先卸载 |
| 本地化 | 安装界面与产品信息跟随 `[app.i18n]` |
| 签名 | 复用现有 `signing.rs`：先签主程序，再签 MSI |

## 2. 与 NSIS 的关系

两者并存、各自独立配置，但**同一个应用应当只选一种**：

| | NSIS（现有） | MSI（本方案） |
| --- | --- | --- |
| 安装位置 | `%LOCALAPPDATA%\<name>`（per-user） | `%ProgramFiles%\<name>`（per-machine） |
| 权限 | 无需管理员 | **需要管理员**（UAC 提权） |
| 注册表 | `HKCU\Software\<name>` + HKCU 卸载项 | `HKLM\Software\<name>` + HKLM 卸载项 |
| 静默安装 | `/S` 能装完，但模板的 `MessageBox` 不被抑制，进程停在对话框上 | `/qn` 完全无对话框，退出码即结果 |
| 部署 | 手动/脚本 | GPO / Intune / SCCM / `msiexec` |
| 适合 | 普通用户自助安装、无需管理员 | 企业统一部署 |

因为安装位置和注册表位置不同，两种安装包可以在同一台机器上并存（互不覆盖），但会出现
两条「应用和功能」记录与两份程序。文档与 `pacor check` 都应提示这一点；可选的后续增强是
在 MSI 里加一个 `LaunchCondition`，检测到 NSIS 版已安装（`HKCU\Software\<name>`）时拒绝安装。

## 3. 工具链：固定 WiX v3.14

沿用 NSIS 的做法：**下载固定版本到缓存目录，不依赖宿主已安装的任何东西**。

| 方案 | 结论 |
| --- | --- |
| **WiX v3.14.1 binaries**（采用） | `wix314-binaries.zip`（39.4 MB，2024-03-23，MS-RL）解压即用 `candle.exe`/`light.exe`/`heat.exe` 与 `WixUIExtension.dll`；**不需要 .NET SDK、不需要安装程序**；WixUI 的多语言字符串内嵌在扩展里（zip 中没有任何 `.wxl`） |
| WiX v7（2026-04） | 维护中，但只有 `wix-cli-x64.msi`（12.4 MB）或 NuGet 包，需要安装 CLI / .NET SDK + 联网还原，破坏「缓存内自包含」模式；模板方言也不同（`<Package>`/wixext）。作为未来迁移目标，改动面已被隔离在工具链 provider + 模板里 |
| msitools `wixl` | WiX 方言子集，**没有 WixUI 扩展**（无标准界面与本地化），Windows 上无官方固定二进制 |
| 手写 MSI（自己构造 OLE 复合文档与 MSI 表） | 要自己承担 CAB、InstallExecuteSequence、组件规则，且没有 ICE 校验兜底 |

缓存布局（与 NSIS 同构，tag 参与路径）：

```
<用户配置目录>/pacor/windows/wix/<模板 tag>/   # 解压后的 WiX 二进制
<用户配置目录>/pacor/windows/nsis/<模板 tag>/  # 现有 NSIS + 模板
```

`pacor cache clean` 一并清除。

## 4. 模板契约：新 tag `1.1`（已发布）

模板 tag 不可变（且采用 GNU 风格版本号，见 [README](../README.zh-CN.md#构建模板)），MSI 需要
新增模板文件，因此发了 `1.1`：

```
windows/wix/main.wxs           # 产品、界面、升级、快捷方式、ARP，含 {{占位符}}
windows/wix/culture.wxl.tmpl   # 每种语言渲染一份 <culture>.wxl
```

`1.1` 内容 = `1.0`（即历史上的 `v1`）的 `windows/nsis/**` 与 `linux/**` **逐字节不变**
（11 个 blob 的 SHA 与 `v1` 相同，可用 `gh api repos/pacor-rs/tools/compare/v1...1.1` 复核）
+ 上述新增 + README 补充的 WiX 契约说明。`TEMPLATE_REF` 从 `v1` 提到 `1.1` 后，缓存路径变化
会同时失效旧的 NSIS 缓存并重新下载——这正是设计意图（一个 pacor 版本只认一个模板契约）。

两份模板的权威副本同时放在本仓库的 `tools/windows/wix/` 下（与 `tools/macos/` 的存放方式
一致），修改模板时以此为准，再按上面的流程发新 tag。

占位符（沿用 `render_template`，未替换即构建失败）：

`main.wxs`（**每个语言渲染一份**，因为 `Product/@Language` 无法本地化）：

| 占位符 | 来源 |
| ------ | ---- |
| `{{APP_NAME}}` | `pubspec.yaml` 项目名（安装目录名、注册表键） |
| `{{PRODUCT_VERSION}}` | 归一化后的四段数字版本（见 5.2） |
| `{{PRODUCT_LANGUAGE}}` | `Locale::as_windows_lcid()`，如 `2052` |
| `{{APP_EXECUTABLE}}` | 运行器可执行文件名（复用 `runner.rs`） |
| `{{APP_URL}}` | `[app].url` |
| `{{APP_ICON_PATH}}` | `windows/runner/resources/app_icon.ico` |
| `{{UPGRADE_CODE}}` | `msi_upgrade_code` 或按 5.3 推导 |
| `{{INSTALL_DIR_COMPONENT_GUID}}` `{{START_MENU_COMPONENT_GUID}}` `{{DESKTOP_COMPONENT_GUID}}` | 模板自身组件的 UUIDv5（以 UpgradeCode 为前缀） |
| `{{PAYLOAD_COMPONENT_GROUP}}` | harvester 定义的组件组 Id（`AppPayload`） |

构建输出目录不经过占位符：它由 `candle -dSourceDir=` 传入，生成的 `payload.wxs` 里用
`$(var.SourceDir)\…` 引用，因此生成物不含机器相关路径（见 7.1）。

`culture.wxl`（每种语言渲染一份，承载所有面向用户的文案）：

| 占位符 | 来源 |
| ------ | ---- |
| `{{CULTURE}}` | `Locale::as_wix_culture()`，如 `zh-cn` |
| `{{MSI_CODEPAGE}}` | `msi_codepage` 或 `Locale::as_msi_codepage()`（见 5.4） |
| `{{APP_DISPLAY_NAME}}` `{{APP_PUBLISHER}}` `{{APP_DESCRIPTION}}` `{{APP_COPYRIGHT}}` | 该语言的 `[app.i18n.<locale>]`，缺失时回退到默认语言 |
| `{{FEATURE_MAIN}}` `{{UNINSTALL_SHORTCUT}}` `{{DOWNGRADE_ERROR}}` | pacor 内置文案（`msi/generator.rs` 的小表，`main.wxs` 里以 `!(loc.…)` 引用） |

WixUI 自己的界面文案（Welcome、Next 等）由扩展按 `-cultures:` 提供，不在模板里。

### 4.1 多语言：每种语言一个 MSI

实测 `light -cultures:en-us;zh-cn` 配两份 `-loc` 会报
`LGHT0100: The localization identifier 'ProductName' has been duplicated`（light 一次只接受
一套本地化）。因此：

- 每种语言构建一个 MSI：`<name>-<version>-<arch>-<locale>.msi`（与 tauri 的做法一致）；
- 语言码用 `Locale::as_wix_culture()`（新增，`en-US` → `en-us`、`zh-CN` → `zh-cn`，
  与既有 `as_apple()`/`as_posix()` 对称，现有测试已在守护这类「每消费方一种拼写」的约定）；
- 单文件多语言需要 `torch`/`pyro` 语言变换或 Burn bootstrapper，列入 M3。

## 5. 配置

```toml
[platforms.windows]
installer_format = "msi"       # nsis | msi
# msi_upgrade_code = "..."     # 可选；缺省自动推导
# msi_codepage = 65001         # 可选；缺省用 locale 自己的代码页
```

### 5.1 不加 `install_scope`

因为只做 per-machine，**不引入 `install_scope` 配置项**：MSI 恒为 per-machine，NSIS 恒为
per-user。避免了一个只能对一种格式生效的开关。

### 5.2 版本号

`VIProductVersion` 与 MSI 的 `ProductVersion` 都只接受四段数字，因此把现有
`nsis/generator.rs` 的 `nsis_product_version` 提升为
`pacor_core::packaging::windows_version()`，两条产物线共用：

- `1.0.0+1` → `1.0.0.1`；`1.0.0` → `1.0.0.0`；`1.2.0-beta.1+7` → `1.2.0.7`；超过 65535 截断；
- MSI 里配 `AllowSameVersionUpgrades="yes"`：否则只改 `+build`（只影响第 4 段，而 MSI 只比较
  前三段）不会触发升级。

### 5.3 `msi_upgrade_code` 缺省推导

`UpgradeCode` 必须跨版本稳定。未配置时按 UUIDv5 推导，无需用户编 GUID：

```
uuid_v5(NAMESPACE, "pacor:windows:msi:<name>:<publisher>")
```

`sha1` 已是 `pacor-core` 的依赖，UUIDv5 不需要新依赖；实现按 RFC 4122 的字节序，并用
RFC 的 DNS 向量做单测（`python.org` → `886313e1-3b8a-5372-9b90-0c9aee199e5d`）。改名或改
发布者会改变 UpgradeCode，文档中说明，并可用 `msi_upgrade_code` 钉住。

### 5.4 `msi_codepage`：字符串代码页

Windows Installer 一个包里只能用**一个**代码页存放所有字符串，WiX 默认从 culture 推导。
pacor 显式写入，因为这是 `LGHT0311` 的解释项，也是出口：

| locale | 代码页 |
| ------ | ------ |
| `en-US` `en-GB` | 1252（Windows-1252） |
| `zh-CN` | 936（GBK） |
| `zh-TW` | 950（Big5） |
| `ja-JP` | 932（Shift-JIS） |

实测 `©`（U+00A9）不在 CP936 中（CP950 也只会把它降级成 `c`），因此一个带
`版权所有 © 2025 …` 的 zh-CN 包会以
`LGHT0311: A string was provided with characters that are not available in the specified
database code page '936'` 失败。处理方式有两种：

1. 改写文案（`版权所有 (C) 2025 …`）——推荐，因为 MSI 的代码页处理本身就不适合承载
   任意 Unicode；
2. `msi_codepage = 65001` 改用 UTF-8。实测可行：包能通过 ICE 校验，`msiexec /a` 正常解包，
   中文 `ProductName` 也能正确回读。代码页写在 `WixLocalization/@Codepage` 上而不是
   `Product` 上，因为 `main.wxs` 被多种语言共用，而 `.wxl` 本来就是每个语言一份。

## 6. 打包流水线

```
flutter build windows --<mode>
  → 定位可执行文件（复用 runner.rs）
  → 签名主程序（NSIS 也要求如此：MSI 会把文件内容嵌进 CAB）
  → harvest(构建输出)                     → payload.wxs
  → 每个语言渲染 main.wxs（带 LCID）+ <culture>.wxl
  → candle -arch <x64|arm64> payload.wxs  → payload.wixobj（各语言共用）
  → 每个语言 candle main.wxs              → <culture>-main.wixobj
  → 每个语言 light -ext WixUIExtension -cultures:<locale> -loc <culture>.wxl
          -sice:ICE60 -sice:ICE61 -pdbout <临时目录>
  → 校验 light 退出码为 0（ICE 错误视为失败，见 8.1）
  → 签名 MSI → 写 .sha1
```

`light` 默认会在产物旁写一个 `.wixpdb` 绑定数据库，用 `-pdbout` 把它放进临时目录，
否则它会被当成用户可见的产物（`dist/` 里多出无用的文件）。

签名不在 `MsiBuilder` 里做：`lib.rs` 先把安装器都构建出来，再统一签名并重新生成校验和
（签名会改写文件，NSIS 与 MSI 走同一条路径）。

代码结构与 `nsis/` 对称：

```
crates/pacor-packaging-windows/src/
├── msi/
│   ├── mod.rs        MsiBuilder：工具链准备、candle/light 调度、产物与校验和
│   ├── toolchain.rs  固定 wix314-binaries.zip + 模板 → 缓存
│   ├── harvest.rs    构建输出 → 目录树 + 每文件一组件（纯函数，重点单测）
│   └── generator.rs  main.wxs / culture.wxl 渲染 + 内置 UI 文案
pacor-core:
    packaging::windows_version()、deterministic_uuid_v5()
    i18n::Locale::as_wix_culture()、as_windows_lcid()、as_msi_codepage()
    config::WindowsConfig::resolved_upgrade_code()、resolved_msi_codepage()
```

模板骨架（与 `tools/windows/wix/main.wxs` 一致，此处省略 `!(loc.…)` 的绑定细节）：

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

两个快捷方式组件分别位于 `DirectoryRef Id="ApplicationProgramsFolder"` 与
`DirectoryRef Id="DesktopFolder"` 下，用 HKCU 注册表值做 KeyPath（原因见 7.2），此处省略。

`payload.wxs` 由 harvester 生成：`<DirectoryRef Id="INSTALLDIR">` 下按构建输出的目录层级嵌套
`<Directory>`，每个文件一个 `<Component Guid="{uuid_v5}"><File KeyPath="yes"/></Component>`，最后用
`<ComponentGroup>` 汇总所有组件 Id（**每个组件都必须被 feature 引用**，否则 `LGHT0267`）。

## 7. MSI 语义要点

### 7.1 harvester 的硬约束（全部实测踩过）

| 约束 | 原因 |
| ---- | ---- |
| 标识符 ≤ 72 字符、只允许 `[A-Za-z0-9_.]`（**不能有 `-`**） | `CNDL1026`；深层路径如 `data/flutter_assets/packages/cupertino_icons/assets/CupertinoIcons.ttf` 会超长。方案：截断 basename + UUIDv5 前 8 位十六进制 |
| 每个文件一个组件 | WiX/MSI 组件规则；`File` 即 KeyPath |
| 每个组件都必须被 feature 引用 | `LGHT0267` orphaned component |
| 组件 Guid 建议确定性生成 | `heat -gg` 每次构建都换 GUID，破坏升级/修复的可预测性；用 UUIDv5(相对路径) 可稳定 |
| `Source` 用 `$(var.SourceDir)\相对路径` | 生成物不含机器相关绝对路径，构建可复现性更好，GUID 也不随检出位置变化 |

### 7.2 快捷方式组件必须用 HKCU KeyPath

即使整个包是 per-machine，位于 `ProgramMenuFolder`/`DesktopFolder` 的快捷方式组件仍被 ICE 视为
「用户配置目录」组件：**KeyPath 必须落在 HKCU**（实测 `ICE38` + `ICE43`）。因此：

- 载荷组件（Program Files）：文件 KeyPath；
- 快捷方式组件：`<RegistryValue Root="HKCU" … KeyPath="yes" />` + `<RemoveFolder On="uninstall">`；
- `ProgramMenuFolder`/`DesktopFolder` 在 per-machine 安装下解析到「所有用户」的开始菜单与桌面。

### 7.3 ARP 与图标

- `DisplayName` / `Publisher` / `DisplayVersion` / `EstimatedSize` 由 Windows Installer 自动写入
  `HKLM\...\Uninstall\<ProductCode>`，无需手写（NSIS 模板里那一段注册表代码可以不做）；
- `ARPINSTALLLOCATION` 若直接写 `[INSTALLDIR]` 会触发 `CNDL1077`（属性值里不能引用属性），
  需用 type 51 自定义动作在 `CostFinalize` 之前设置，或干脆不设；
- `ARPNOMODIFY` 等 ARP 属性由 `WixUI_InstallDir` 扩展自己设置，重复 authoring 会得到
  `LGHT0091 Duplicate symbol 'Property:ARPNOMODIFY'`（实测），因此模板不要写它们。

### 7.4 位数与权限

- 不传 `-arch x64` 时 candle 产出 **Intel(32 位) 包**：实测 ARP 落在
  `HKLM\Software\WOW6432Node\...\Uninstall`（而不是原生视图）；把组件装进 64 位目录还会触发
  ICE80（已知规则，本方案始终传 `-arch` 以规避）。取值就是载荷自身的架构——candle 认的
  `x64` / `arm64`，与产物名、构建目录同源的那个 `Arch`——因此 ARM64 构建产出的包会被 Windows
  登记为 ARM64，而不是被当成 32 位-on-ARM 去安装和修复的 Intel 包；
- per-machine 安装**需要管理员**：非提升会话下 `msiexec /i /qn` 失败于
  `InstallFinalize`，日志给出 `Error 1925: You do not have sufficient privileges to complete this
  installation for all users of the machine`（实测，退出码 1603，回滚干净）。
  构建 MSI 本身不需要提权，只有安装需要。

### 7.5 与 per-user 无关的约束

per-user 安装会额外引入两条硬约束，**本方案因只做 per-machine 而不受影响**，但留档以免日后
重开 per-user：

- `ICE38`：安装到用户配置目录的**每个组件**都必须用 HKCU 注册表值做 KeyPath（不能是文件）——
  意味着每个文件再写一条 HKCU 值；
- `ICE64`：用户配置目录下的**每个目录**都必须登记在 `RemoveFile` 表里；
- 另外 `ICE91`（per-user 目录下的文件不会随 per-machine 安装复制）也需要显式抑制。

## 8. 验证方案

### 8.1 判定成功的条件

`light` 会**先写出 MSI，再报告 ICE 错误**（实测：ICE 报错的运行中产物仍然存在，退出码非 0）。
因此成功判定必须是「退出码 0 **且** ICE 错误数为 0」，不能只看文件是否存在。

只白名单两条 ICE，其余 ICE 一律视为失败：

| ICE | 处置 | 原因 |
| --- | ---- | ---- |
| `ICE60` | `-sice:ICE60` | Flutter 资产里的 `.otf` 字体没写 Language 列，与本安装包无关 |
| `ICE61` | `-sice:ICE61` | `AllowSameVersionUpgrades="yes"` 必然触发（VersionMax 等于当前版本） |

### 8.2 单元测试

已覆盖：版本归一化与截断、UUIDv5（含 RFC 4122 向量与稳定性）、标识符合法化/截断/去重、
目录树与组件生成（含「每个组件都被 feature 引用」）、XML 转义、空目录告警、
占位符渲染与转义、`as_wix_culture`/`as_windows_lcid`/`as_msi_codepage`、UpgradeCode 推导、
代码页解析、ICE 白名单与缓存路径。

### 8.3 端到端（opt-in，沿用 NSIS 的模式）

`PACOR_MSI_TOOLCHAIN_DIR` 指向准备好的 WiX 目录（见附录），
`an_msi_is_built_end_to_end` 用一份合成载荷构建两个语言的 MSI 并检查：

1. `light` 退出码 0 且 ICE 为 0（`light` 的退出码是唯一判据，见 8.1）；
2. 产物名带 `<locale>`，`.sha1` 存在，且 `.wixpdb` 没有漏到输出目录；
3. `msiexec /a <msi> /qn TARGETDIR=<tmp>` 行政安装解包 → 与构建输出**逐文件比对**
   （这条不需要提权，适合 CI）；

人工验证时还可以补：

4. 提权会话下 `msiexec /i /qn` → 断言 `%ProgramFiles%\<name>`、`HKLM` 的 ARP 条目
   （注意读 **64 位**注册表视图）、所有用户桌面/开始菜单快捷方式；
5. `msiexec /x /qn` → 断言目录、ARP、快捷方式全部清除。

第 4、5 步需要管理员会话，本机（非提升）无法执行，列为 M2.5。

## 9. 分阶段落地

| 阶段 | 内容 | 状态 |
| ---- | ---- | ---- |
| M0 | 固定 WiX、harvester 原型、per-machine 配方、ICE 清零、`/a` 比对、权限与位数结论 | **已完成**（脚本见附录） |
| M1 | `[platforms.windows]` 配置 + `msi/` 模块 + 模板 `1.1`（已发布） + 每语言 MSI + 签名 + `pacor check` 提示工具链 + 文档 | **已完成**（`6d56558`） |
| M2 | 单元测试（harvester/生成器/工具链/代码页/Lcid）+ opt-in 端到端（真实 candle/light + `/a` 比对） | **已完成** |
| M2.5 | 提升权限会话下补跑 `msiexec /i` / `/x`，确认 ARP 与快捷方式落点 | 待做（本机无法提权） |
| M3 | 可选：`installer_formats` 集合（NSIS 与 MSI 同时产出）、语言变换单文件多语言、NSIS 版共存检测（`LaunchCondition`）、VC++ 运行时合并模块 | 待定 |

## 10. 实测结论

在示例项目 `pacor-demo`（Flutter 3.32.8，`version: 1.0.0+1`，`[app.i18n]` 为 en-US + zh-CN）
上以 WiX 3.14.1.8722 实测：

| 结论 | 证据 |
| ---- | ---- |
| WiX 二进制自包含、无需安装 | `candle/light` 直接运行并输出 `version 3.14.1.8722` |
| 自研 harvester 可过 ICE | 12 文件 → 12 组件，`light exit=0`、ICE errors=0 |
| 深层路径 ID 超长 | `CNDL1026`（78 字符），截断 + 十六进制后缀后 0 告警 |
| 标识符不能含 `-` | 用连字符做 ID 时 candle 失败，改为 `.` 后通过 |
| 快捷方式组件的 KeyPath 必须是 HKCU | 用 HKLM 时 `ICE38` + `ICE43` |
| `ARPNOMODIFY` 与 WixUI 冲突 | `LGHT0091 Duplicate symbol 'Property:ARPNOMODIFY'` |
| `ARPINSTALLLOCATION=[INSTALLDIR]` 非法 | `CNDL1077` |
| 多语言 `-loc` 冲突 | `-cultures:en-us;zh-cn` + 两份 `.wxl` → `LGHT0100` |
| 32 位/64 位包差异 | 默认 Intel 包 ARP 在 `WOW6432Node`；传 `-arch`（x64/arm64）后在原生视图（`Template=x64;…`） |
| light 先出产物后报 ICE | ICE 报错运行中 `msi` 仍存在，退出码 204 |
| `/a` 解包可用于校验 | 两个语言的包各 12 个文件，与构建目录逐一对齐 |
| per-machine 需要提权 | 非提升会话 `Error 1925`，退出码 1603，回滚干净 |
| `light` 默认在产物旁写 `.wixpdb` | 需要 `-pdbout` 重定向，否则 `dist/` 会多出该文件 |
| MSI 一个包只有一个字符串代码页 | zh-CN 文案含 `©` → `LGHT0311`（CP936 无 `©`） |
| 包属性符合设计 | `UpgradeCode` 两语言一致、`ProductVersion=1.0.0.1`、`ALLUSERS=1`（即 per-machine）、`ProductLanguage` 为 1033/2052 |

**仍未验证**（本机为非提升会话）：提升权限下的完整安装/卸载、ARP 条目与快捷方式的实际落点、
`MajorUpgrade` 的升级路径（装 `1.0.0.0` 再装 `1.0.0.1`）。这些是 M2.5 的验收条件。

## 11. 风险与待决

1. **WiX v3 已冻结**（2024-03 最后一版）：MSI 格式本身多年未变，风险低；若未来需迁移 v7，
   改动面限于 `msi/toolchain.rs` 与模板方言。
2. **MSI 不可复现构建**：`ProductCode="*"` + 时间戳使每次构建的 `.sha1` 不同（NSIS 同样如此），
   发布流程不要假设同输入同哈希。
3. **VC++ 运行时**：Flutter Windows 应用依赖 MSVC 运行时，干净系统上可能缺失。MSI 可用
   VC++ 合并模块或 app-local 部署解决；**NSIS 路径今天同样缺这一步**，建议一并决策。
4. **与 NSIS 版共存**：两者安装位置不同，可并存但会各有 ARP 记录；M3 的共存检测是可选缓解。
5. **每语言一个 MSI** 会让产物数量随 `[app.i18n]` 增长；若不可接受，需在 M3 引入语言变换。
6. **per-user 明确不做**：需要 per-user 的企业部署只能改用 NSIS 包（或另立需求重新设计，
   代价见 7.5）。

## 附录：spike 复现

本次 spike 在 Windows 上完成，脚本放在临时目录（未纳入仓库）：

```
%TEMP%\wix314\            # 解压自 wix314-binaries.zip
%TEMP%\msi-spike\
  harvest.ps1             # per-user 版 harvester（含 HKCU KeyPath 与 RemoveFolder）
  harvest-machine.ps1     # per-machine 版 harvester（推荐形状）
  main.wxs                # per-user 模板（WixUI + ARP + 快捷方式）
  main-machine.wxs        # per-machine 模板（推荐形状）
  en-US.wxl / zh-CN.wxl   # 本地化
  machine.ps1             # 端到端：candle → light → /a 比对 → /i → /x
```

复现步骤：下载并解压 WiX → 用 `flutter build windows --release` 产出构建目录 →
`harvest-machine.ps1 -SourceDir <构建目录> -OutFile payload.wxs` →
`candle -arch x64`（ARM64 机器上为 `-arch arm64`）编译两个 `.wxs` → `light -ext WixUIExtension -cultures:<culture> -loc <culture>.wxl
-sice:ICE60 -sice:ICE61` → `msiexec /a`（解包校验）/ `/i /qn`（需提权）/ `/x /qn`。

WiX 下载地址：<https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip>。

下载内容在解压前会与固定的 SHA-256（`crates/pacor-packaging-windows/src/msi/toolchain.rs` 中的
`WIX_ARCHIVE_SHA256`）比对，理由与 NSIS 归档相同：发行版资产随时可能被其所有者替换，而
`candle.exe`、`light.exe` 随后会在构建机上执行。上游有意变更后需要更新该固定值时：

```bash
cargo test -p pacor-packaging-windows -- --ignored the_pinned_wix_digest_matches_the_published_archive
# 或者不检出仓库：
curl -sL https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip | sha256sum
```
