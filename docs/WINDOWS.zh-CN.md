<p align='center'>
<a href="WINDOWS.md">English</a> | <span>中文</span>
</p>

# Pacor Windows 平台说明

本文档说明 `pacor build --target windows` 的产物、配置、工具链缓存与常见排查方法。
配置项的完整注释模板见 [`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml)，
GitHub 镜像相关环境变量见 [ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.zh-CN.md)。

> MSI 安装包（`installer_format = "msi"`，全机安装）的设计与实测结论见 [MSI.md](MSI.zh-CN.md)。

## 产物

一次成功的构建会在 `[build] output_dir`（默认 `dist/`）下写入：

| 文件 | 说明 |
| ---- | ---- |
| `<name>-<version>-<arch>-setup.exe` | NSIS 安装包（`installer_format = "nsis"`，用户级安装，无需管理员权限） |
| `<name>-<version>-<arch>-<locale>.msi` | MSI 安装包（`installer_format = "msi"`，全机安装，每个语言一个） |
| `<name>-<version>-<arch>-portable.zip` | 便携版；`build_portable = false` 时不生成 |
| 上述文件的 `.sha1` | 与产物同名的 SHA1 校验文件 |

`<arch>` 取自宿主架构（`x64` 或 `arm64`），`<version>` 是 `pubspec.yaml` 中的原始版本号
（例如 `1.0.0+1`）。

便携版 ZIP 内所有条目都位于单层根目录 `<name>-<version>-<arch>/` 下，
因此解压不会把应用文件散落到解压目录中。

安装包的行为由固定版本的 NSIS 模板决定，当前包括：

- 安装到 `%LOCALAPPDATA%\<name>`（用户级、无需管理员权限），并写入 `HKCU` 卸载信息；
- 组件页可选择桌面快捷方式与开始菜单快捷方式；
- 安装/卸载前检测应用是否正在运行；安装前检查至少 500 MB 可用空间；
- 安装完成后可勾选立即启动；
- 按 `[app.i18n]` 生成本地化的安装界面与版本信息。

## 配置

```toml
[platforms.windows]
enabled = true
installer_format = "nsis"        # nsis | msi
build_portable = true
# 签名相关（可选）
# signing_tool_path = "C:/Program Files (x86)/Windows Kits/10/bin/10.0.22621.0/x64/signtool.exe"
# certificate_path = "cert.pfx"
# certificate_password = "..."   # 建议改用 PACOR_WINDOWS_CERT_PASSWORD 环境变量
# timestamp_url = "http://timestamp.digicert.com"
# MSI 专用（可选）
# msi_upgrade_code = "{11223344-5566-7788-99AA-BBCCDDEEFF00}"
# msi_codepage = 65001
```

## MSI 安装包

`installer_format = "msi"` 时每个 `[app.i18n]` 语言各产出一个
`<name>-<version>-<arch>-<locale>.msi`（`light` 一次只能链接一套本地化文件，见
[MSI.md](MSI.zh-CN.md)）。与 NSIS 包的关键差异：

| | NSIS | MSI |
| --- | --- | --- |
| 安装位置 | `%LOCALAPPDATA%\<name>` | `%ProgramFiles%\<name>` |
| 权限 | 无需管理员 | **需要管理员**（`msiexec` 会要求提权） |
| 静默安装/卸载 | `/S` 会停在模板的对话框上 | `/qn` 全程无对话框，退出码即结果 |
| 部署 | 手动或脚本 | GPO / Intune / SCCM / `msiexec` |

同一个应用建议只选一种格式：两者安装位置与注册表位置不同，可以并存，但会留下两条
「应用和功能」记录与两份程序。

MSI 的字符串使用**每个语言自己的代码页**（zh-CN 为 936、zh-TW 为 950、ja-JP 为 932、
en-* 为 1252），因此本地化文案必须能被该代码页编码。`©` 不在 936 中，zh-CN 包里若出现
`©` 会以 `LGHT0311` 失败；此时可以改写文案（常见做法是用 `(C)`），或设置
`msi_codepage = 65001` 改用 UTF-8。

### 代码签名

配置了 `certificate_path` 或 `signing_tool_path` 即开启签名，此时
`certificate_path` 与证书口令（`PACOR_WINDOWS_CERT_PASSWORD` 优先于
`certificate_password`）都是必填项，缺失会直接报错而不是静默跳过。

签名工具按以下顺序查找：

1. `signing_tool_path`（必须存在）；
2. `PATH` 中的 `signtool.exe`；
3. Windows SDK 中的 `.../Windows Kits/<10|8.1>/bin/<版本>/<x64|x86>/signtool.exe`，
   取版本号最高的一个。

签名使用 `signtool` 风格参数：

```
signtool sign /fd SHA256 /td SHA256 /tr <timestamp_url> /f <certificate_path> /p <password> <binary>
```

`timestamp_url` 默认为 `http://timestamp.digicert.com`；设为空字符串可关闭时间戳
（不建议：没有时间戳的签名会在证书过期后失效）。

`pacor check --target windows` 会一并校验签名配置（证书是否存在、口令是否提供、签名工具能否
找到），因此可以在构建前发现配置问题。

签名顺序很重要：**先签名应用主程序，再编译安装包**。NSIS 在编译时把文件内容嵌入安装包，
如果先固化安装包再签名，用户安装得到的仍是不带签名的程序。安装包自身在生成后也会签名，
并且由于签名会改写文件，`.sha1` 是在签名完成之后才生成的。

### 构建模式

`[build] flutter_build_mode`（`release` / `debug` / `profile`，可用 `PACOR_BUILD_MODE` 覆盖）
决定 `flutter build windows --<mode>`，并决定读取哪个构建目录
（`build/windows/<arch>/runner/<Release|Debug|Profile>`）。
`release` 与 `profile` 会附加 `--obfuscate --split-debug-info=tmp/`，调试符号写入项目下的
`tmp/`（构建结束时由清理步骤删除，`--skip-cleanup` 可保留；`pacor clean` 只删除 `dist/` 与
`build/`，不含 `tmp/`）；`debug` 不能混淆，因此不附加这些参数。
Android 后端不使用构建模式（始终 release，不混淆，符号同样写入 `tmp/`），见
[ANDROID.md](ANDROID.zh-CN.md)。

## 可执行文件与版权信息的识别

Flutter 生成的可执行文件名来自 `windows/CMakeLists.txt` 中的 `BINARY_NAME`，
`flutter create` 会用项目名填写它，但项目改名后两者可能不一致，因此 pacor 依次尝试：

1. `windows/CMakeLists.txt` 中的 `BINARY_NAME`；
2. `pubspec.yaml` 中的项目名；
3. 构建目录下唯一的 `.exe`。

若都不匹配（例如目录里存在多个 `.exe`），会给出包含候选列表的报错。

`Runner.rc` 中的 `LegalCopyright` 仅作为 `[app.i18n.<locale>] copyright` 的后备值，
读取失败不会中断构建；文件可以是 UTF-8（含 BOM）或 Visual Studio 保存的 UTF-16。

## NSIS 工具链与缓存

`makensis` 的选取顺序：

1. `PATH` 中的 `makensis`，前提是它看起来是一个 NSIS 3 安装（存在 NSIS 3 自带的
   `Plugins\x86-unicode` 目录）且该目录可写，以便放入插件；
2. pacor 自己缓存的 NSIS 3（从 tauri-apps 的 `nsis-3` 发行版下载），任何情况下都可写。

若系统级 NSIS 需要管理员权限才能写入插件目录，或它其实是 NSIS 2（无法编译 `Unicode True`
模板），pacor 会给出警告并改用自身缓存，而不会构建失败。

缓存位于 `<用户配置目录>/pacor/windows/nsis/<模板 tag>`，在 Windows 上即
`%APPDATA%\pacor\windows\nsis\1.1\`。该目录同时保存 NSIS 发行版与固定 tag 的模板：

```
makensis.exe                  # NSIS 发行版（必要时下载）
template.nsi                  # 安装包脚本模板
Plugins/nsProcess.dll         # 从模板 tag 下载的插件（原始位置）
Plugins/nsProcess.nsh
languages/<NSIS 语言>.nsh     # 各语言的 SECTION_*/DESC_*/MSG_* 字符串
```

模板与渲染它们的代码一起版本化，tag 固定在
[`crates/pacor-core/src/tools.rs`](../crates/pacor-core/src/tools.rs) 的 `TEMPLATE_REF`
（GNU 风格版本号，当前为 `1.1`；仓库为 [pacor-rs/tools](https://github.com/pacor-rs/tools)）。
`pacor cache clean` 会删除整个缓存，下次构建重新下载。

使用缓存中的 NSIS 时，该目录就是 `$NSISDIR`，插件会被安装到
`%APPDATA%\pacor\windows\nsis\<模板 tag>\Plugins\x86-unicode\nsProcess.dll`；
使用系统级 NSIS 时，插件则被安装到该系统安装的对应目录。

### MSI 的工具链缓存

MSI 使用固定版本的 WiX（`wix314-binaries.zip`，约 39 MB，解压即用、不依赖已安装的
.NET SDK 或安装程序），缓存在 `%APPDATA%\pacor\windows\wix\1.1\`，与 NSIS 同构：

```
candle.exe / light.exe / ...   # WiX 工具链（必要时下载）
main.wxs                       # 产品、界面、升级、快捷方式模板
culture.wxl.tmpl               # 本地化模板，每个语言渲染一份
.extracted                     # 表示工具链已解压的标记
```

模板与工具链放在同一个 tag 目录下：tag 变化会同时失效两者。首次 MSI 构建前
`pacor check` 会提示工具链将被下载；离线预置的方式与 NSIS 相同（放好文件并创建
`.extracted`）。

### 编译安装包时的三处关键位置

失败时最值得检查的就是下面三点：

1. `template.nsi` 由 pacor 生成脚本时读取；
2. `nsProcess.nsh` 被模板以 `!include "nsProcess.nsh"` 引用，因此 pacor 会把它复制到
   **生成脚本所在的临时目录**旁边（`!include` 会先在脚本目录查找）；
3. `nsProcess.dll` 必须位于 `makensis` 的 `$NSISDIR\Plugins\x86-unicode\`。
   模板使用 `Unicode True`，`makensis` 只在该目录查找插件，把它放在脚本目录或缓存里都不会
   被找到。

同理，模板引用的 `SECTION_MAIN`、`DESC_*`、`MSG_*` 等 `LangString` 由模板 tag 中的
`languages/<语言>.nsh` 定义（NSIS 自带的是 `Contrib\Language files\*.nsh`，只提供 MUI 通用
字符串），因此渲染出的 `!include` 指向缓存中的这些文件，且必须是绝对路径。

### 语言映射

| `[app.i18n]` locale | NSIS 语言 |
| ------------------- | --------- |
| `en-US`、`en-GB` | `English` |
| `zh-CN` | `SimpChinese` |
| `zh-TW` | `TradChinese` |
| `ja-JP` | `Japanese` |

默认 locale 的字符串最先包含，其余按 locale 排序，因此安装界面语言列表稳定。

### 版本号

安装包里的版本号字段只接受四个数字段，而 pubspec 版本是 semver。pacor 会做归一化：
`1.0.0+1` → `1.0.0.1`，`1.0.0` → `1.0.0.0`，`1.2.0-beta.1+7` → `1.2.0.7`，
超过 65535 的段会被截断。NSIS 的 `VIProductVersion` 与 MSI 的 `ProductVersion`
共用这一套归一化；安装界面与卸载信息中仍显示原始的 `1.0.0+1`。

## 安装与卸载（实测行为）

以下行为由真实的 `flutter create` 示例项目实际安装/卸载验证过（对应固定模板 `1.1`）：

- 安装到 `%LOCALAPPDATA%\<name>`，并创建本地化快捷方式：桌面
  `$(APP_DISPLAY_NAME).lnk`、开始菜单文件夹 `<name>\`（含应用与卸载两个快捷方式）；
  安装后的程序可以直接运行；
- 卸载会删除安装目录、桌面与开始菜单快捷方式，以及「应用和功能」中的注册项；
- **模板里的 `MessageBox` 在静默模式下不会被抑制**，因此 `setup.exe /S`、
  `Uninstall.exe /S` 都会在完成实际操作后停在对话框上、进程不退出：
  安装停在 `Function .onInstSuccess` 的「是否立即启动」，卸载停在卸载段末尾的
  「卸载完成」。脚本化调用时请以**文件系统状态**（安装目录是否出现/消失）判断是否完成，
  而不是等进程退出，并在确认结果后结束该进程；
- `Uninstall.exe /S` 会把自身复制到 `%TEMP%\~nsu.tmp\Un_A.exe` 再从那里执行，
  因此启动的进程约 2 秒后即返回，真正的删除在后台继续（实测数十秒）；
- 卸载后可能残留一个只含 `InstallDir`、`Version` 两个值的 `HKCU\Software\<name>`：
  模板用 `DeleteRegKey /ifempty` 清理它，而键内仍有值，因此不会被删除。
  这是固定模板 `1.1` 的行为，可手工删除该键。

## 清理

- `pacor clean`：删除输出目录与 Flutter 构建产物（带 `--target` 时只删该目标的构建目录与产物）；
  `tmp/` 里的调试符号由构建结束时的清理步骤删除，`--skip-cleanup` 可保留；
- `pacor cache clean [--dry-run]`：删除 `%APPDATA%\pacor` 下的共享缓存（含 NSIS、WiX 与模板）。

## 常见问题

### `Plugin not found, cannot call nsProcess::_FindProcess`

`nsProcess.dll` 没有被放到 `makensis` 的 `$NSISDIR\Plugins\x86-unicode\`。
使用系统级 NSIS 且该目录不可写时会出现这种情况；pacor 会回退到自身缓存并重新构建，
若仍然出现，请删除缓存（`pacor cache clean`）后重试。

### `Error: invalid VIProductVersion format, should be X.X.X.X`

`pubspec.yaml` 的版本号中含非数字段。pacor 已对 `+build` 与预发布后缀做归一化，
若仍报错请检查版本号本身是否符合 pubspec 的 semver 约定。

### `No application executable found in ...`

构建目录中没有 `.exe`，或目录不存在。使用 `--use-existing-flutter-build` 时需要先执行
`flutter build windows`；也可检查 `windows/CMakeLists.txt` 中的 `BINARY_NAME`
是否与 `pubspec.yaml` 的项目名一致。

### 安装包图标缺失

`windows/runner/resources/app_icon.ico` 是安装包与快捷方式图标，必须存在；
可用 `flutter create --platforms=windows .` 重新生成。

### 找不到 `signtool`

安装 Windows SDK，或设置 `platforms.windows.signing_tool_path` 指向已有的
`signtool.exe`。pacor 会在 SDK 的 `…\Windows Kits\<10|8.1>\bin\<版本>\x64\` 中查找，
因此通常不必手动配置。

### 浏览器能打开 GitHub，但 pacor 下载失败

pacor 的下载客户端只读 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 环境变量，**不读 Windows 的
PAC 自动配置脚本或系统代理设置**，而浏览器读的是后者。所以「浏览器能访问」并不说明 pacor 能访问。

先从「Internet 选项 → 连接 → 局域网设置」看到 PAC 地址，再从 PAC 里找到实际代理，然后在运行
pacor 的终端里导出：

```powershell
$env:HTTPS_PROXY = "http://127.0.0.1:1086"
$env:HTTP_PROXY  = "http://127.0.0.1:1086"
```

注意：**配置了镜像时 pacor 不使用代理**，两者不能叠加。代理能直连 GitHub 时，用代理、不设镜像
是最优组合——实测冷缓存下模板 2 s、39 MB 的 WiX 工具链 26 s 下载完成，随后 MSI 与 NSIS 构建都
在 1 分钟内结束。详见
[ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.zh-CN.md#http_proxy--https_proxy标准代理变量)。

### 下载 NSIS 或模板失败（网络受限）

pacor 固定的下载地址位于 `github.com` 与 `raw.githubusercontent.com`，在受限网络中可能
无法访问。可以按 [ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.zh-CN.md) 配置镜像：

- 模板（raw 链接）：`PACOR_GITHUB_RAW_MIRROR_TEMPLATE`；
- NSIS 发行版（release 资源）：`PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` 或
  `PACOR_GITHUB_MIRROR_BASE_URL`；
- WiX 工具链同样是 release 资源（`wixtoolset/wix3` 的 `wix3141rtm`），也走上面两个镜像变量。

一个变量覆盖全部下载的写法：

```powershell
$env:PACOR_GITHUB_MIRROR_BASE_URL = "https://gh.my-website.ccwu.cc"
```

两点实测经验（详见 [ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.zh-CN.md#镜像选型要注意的两件事实测)）：

- 模板文件只有几 KB，任何能连通的镜像都够用；NSIS 发行版约 2 MB，冷启动整条链路实测 1 分钟内完成；
- **WiX 工具链约 39 MB，是对镜像的考验**：镜像带宽不足或中途断流时下载会中断；pacor 会把已收到的
  字节保留下来并在下一次尝试时断点续传（最多 4 次），因此单次断流不再让构建失败。真正不行的是
  完全无法交付该文件的镜像，这类网络下更省事的是离线预置（见下）。

也可以离线预置：把 NSIS 发行版与模板按上面的布局放进
`%APPDATA%\pacor\windows\nsis\<模板 tag>\`，并创建一个空的 `.extracted` 标记文件；
已存在的模板文件本来就会被跳过，NSIS 则因该标记而不再下载。MSI 的
`%APPDATA%\pacor\windows\wix\<模板 tag>\` 同理（放好解压后的 WiX 工具链、`main.wxs`、
`culture.wxl.tmpl` 与 `.extracted`）。

### `error LGHT0311 ... database code page '936'`（MSI）

本地化文案里有该语言代码页无法表示的字符。最常见的是 `©`（不在 CP936 中；zh-TW 的 CP950
也只会把它降级成 `c`）。两种处理方式：

1. 改写文案，例如 `版权所有 (C) 2025 ...`——推荐；
2. 设置 `msi_codepage = 65001`，让整包改用 UTF-8。

`light` 会指出出错的行号，但不会指出是哪个字符。

### `Error 1925: You do not have sufficient privileges ...`（MSI）

MSI 是全机安装包，`msiexec /i` 需要管理员权限；在非提升的会话里（例如自动化账户）会失败于
`InstallFinalize`，退出码 1603，并干净回滚。构建 MSI 本身不需要提权，只有安装需要。
不需要提权的校验方式是行政安装（解包）：

```powershell
msiexec /a .\dist\<name>-<version>-<arch>-<locale>.msi /qn TARGETDIR=C:\temp\unpacked
```

它按安装布局解出包内文件，可用于与 `build\windows\x64\runner\Release` 逐文件比对。

## 开发验证

`pacor-packaging-windows` 带有几个默认忽略的端到端测试：两个使用真实的 `makensis`（渲染出的
脚本能否编译、`NsisBuilder::build` 全流程），另一个使用真实的 WiX 构建两个语言的 MSI，并用
`msiexec /a` 与构建目录逐文件比对：

```powershell
$env:PACOR_NSIS_TOOLCHAIN_DIR = "$env:TEMP\pacor-nsis"
$env:PACOR_MSI_TOOLCHAIN_DIR = "$env:APPDATA\pacor\windows\wix\1.1"
cargo test -p pacor-packaging-windows -- --ignored
```

NSIS 目录需要包含 `makensis.exe`（来自
<https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip>，
解压后去掉顶层 `nsis-3.08/` 目录）、模板 tag 的 `template.nsi`、
`Plugins/nsProcess.nsh`、`Plugins/nsProcess.dll` 与 `languages/*.nsh`
（<https://github.com/pacor-rs/tools> 的对应 tag）。WiX 目录需要包含 `candle.exe`、
`light.exe`、`WixUIExtension.dll`，以及模板 tag 的 `main.wxs` 与 `culture.wxl.tmpl`。
