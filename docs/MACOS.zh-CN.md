<p align='center'>
<a href="MACOS.md">English</a> | <span>中文</span>
</p>

# Pacor macOS 平台

本文记录 `pacor build --target macos` 的产物、配置、签名与公证流程、缓存布局与故障排查
（它和其他目标一样接受 `--use-existing-flutter-build` 与 `--skip-cleanup`）。带完整注释的
配置模板见
[`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml)，
GitHub 镜像相关的环境变量见 [ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.md)。

> macOS 后端**只能在 macOS 上构建**。在开始构建之前，`validate` 会在 `PATH` 上查找
> `flutter`、`xcodebuild` 和 `hdiutil`；配置了 `codesign_identity` 时再加 `codesign`，
> 配置了 `notarize_credentials` 时再加 `xcrun`。缺少工具会以
> `Platform tool not found: <tool> (required for macOS)` 直接失败，而不是等到真正需要它的那次
> `bash` 调用里才报错。

该目标只有 `dmg` 一种输出格式，`--format` 不接受其它值。

## 产物

构建成功后，产物写入 `[build] output_dir`（默认 `dist/`），每个产物都带一个同名 `.sha1`：

| 文件 | 说明 |
| ---- | ---- |
| `<name>-<version>-<arch>.dmg` | 安装镜像，`.app` 就在其中 |
| `<name>-<version>-<arch>.app.tar.gz` | 更新归档：同一个 `.app` 打成 tar 再 gzip |
| 上面两者的 `.sha1` | 以产物命名的 SHA1 校验文件 |

`<name>` 取自 `pubspec.yaml` 的 `name:`（不是 `[app.i18n]` 的 `display_name`），
`<version>` 是原始版本号（例如 `1.0.0+1`），`<arch>` 是运行构建的主机架构，写作 `x64`
或 `arm64`——这个后端打包的就是它自己运行的架构。例如：

```
dist/MyApp-1.0.0+1-arm64.dmg
dist/MyApp-1.0.0+1-arm64.app.tar.gz
```

校验值最后才写：签名和公证都会改写它们覆盖的字节。

## 配置

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

| 配置项 | 作用 |
| --- | --- |
| `enabled` | `--target macos` 是否构建；为 `false` 时以 `Platform not enabled: macOS` 失败 |
| `bundle_identifier` | 作为 `CFBundleIdentifier` 写入构建产出的 `Info.plist`；不设则保留 Flutter 生成的标识 |
| `minimum_system_version` | `LSMinimumSystemVersion`；默认 `10.14`，与 Flutter 自带 macOS 模板一致 |
| `category` | `LSApplicationCategoryType`，App Store 分类标识；默认 `public.app-category.entertainment` |
| `dmg_background` | 安装窗口的背景图；默认不设 |
| `dmg_window_size` | 安装窗口尺寸；默认不设 |
| `codesign_identity` | `codesign` 身份；设置后 `.app` 与 `.dmg` 都会被签名 |
| `codesign_entitlements` | `.app` 签名用的 entitlements 文件；不设则用 Flutter 按构建模式生成的那个 |
| `notarize_credentials` | `notarytool` 的钥匙串配置名；设置后启用公证 |

三个 bundle 元数据项是**就地**写入 Xcode 产出的 `Info.plist`：pacor 不设置的那些条目——
布尔值、整数、嵌套字典、`CFBundleExecutable`、下面的本地化——一律原样保留；没有配置的项
保持构建写入的值。

设置 `dmg_background` 或 `dmg_window_size` 还有第二个影响：那一次 `create-dmg` 不再带
`--sandbox-safe`，因为窗口布局由 Finder 的 AppleScript 完成，而 `--sandbox-safe` 恰恰会跳过
它。该 AppleScript 需要已登录的图形会话与 Finder 自动化权限，所以这两项默认都不设置。

## 构建步骤

1. **准备打包工具。** `create-dmg.sh` 及其 `support/` 模板内嵌在二进制里，每次运行都重写到
   缓存目录（见下），因此缓存里的脚本永远不会比正在运行的 pacor 旧。
2. **`flutter build macos`**，模式取自 `[build] flutter_build_mode`（`--release` /
   `--profile` / `--debug`），对 AOT 模式再加 `--obfuscate --split-debug-info=tmp/`。带
   `--use-existing-flutter-build` 时跳过这一步，直接打包
   `build/macos/Build/Products/<mode>/` 里已有的 bundle。
3. **本地化。** 每种语言写一个 `<language-id>.lproj/InfoPlist.strings`，目录名用 Apple 语言
    ID（`zh-Hans`，不是 `zh-CN`），并在 `Info.plist` 里写入 `CFBundleLocalizations` 与
   `CFBundleDevelopmentRegion`。
4. **bundle 元数据**，即上表的三个配置项，写入同一个 `Info.plist`。
5. **签名 `.app`**（配置了 `codesign_identity` 时，见下）。
6. **生成 DMG**，用内置的 `create-dmg`：先把 `.app` 复制到临时暂存目录，因此 Flutter 的
   构建产物不会被改动。镜像卷名为 `<name> Installer`，并带常规的 Applications 拖放入口。
7. **签名 DMG**（配置了身份时）：镜像是整条签名链的最外层。
8. **公证与装订票据**（配置了 `notarize_credentials` 时）。
9. **更新归档**以及两个产物的校验值。
10. **清理** `tmp/`——`--split-debug-info` 把 Dart AOT 符号写在那里；`--skip-cleanup` 保留它们
    以便 `flutter symbolize`。

## 签名

签名由 `codesign_identity` 驱动，每次调用都是

```
codesign --force --timestamp --options runtime --sign <identity> \
    [--entitlements <file>] <target>
```

硬化运行时与时间戳是无条件的：没有它们的签名无法通过公证，而公证正是配置身份的唯一理由。

签名是对其包含的一切字节的封印，所以顺序不是装饰：

1. `Contents/Frameworks` 下的嵌套代码——`.framework`、`.app` 与 `.dylib`——**由深到浅**，
   让内层先于外层被封印；这些**不带** entitlements；
2. `.app` 本身，带 entitlements；
3. `codesign --verify --deep --strict --verbose=2` 校验 bundle。校验失败即构建失败：一个
   Gatekeeper 会拒绝的 app 交付出去没有意义；
4. `.dmg`，在镜像构建完成之后。只签镜像——本后端以前就是这么做的——会得到一个看起来正常、
   但内部载荷未签名的 DMG，`notarytool` 会以 "code object is not signed at all" 拒收。

entitlements 在签名内部，所以不带它重签会丢掉 app 构建时的沙箱与网络权限。
`codesign_entitlements` 优先且必须指向存在的文件；否则使用
`macos/Runner/Release.entitlements`，debug 与 profile 构建使用
`DebugProfile.entitlements`——也就是 Xcode 本来会用来签名的那份。

## 公证

`notarize_credentials` 是一个 `notarytool` 钥匙串配置名，先用下面的命令创建一次：

```
xcrun notarytool store-credentials AC_PASSWORD \
    --apple-id <id> --team-id <team> --password <app 专用密码>
```

随后构建会执行

```
xcrun notarytool submit <dmg> --keychain-profile <profile> --wait --output-format json
xcrun stapler staple <dmg>
```

`notarytool submit --wait` 无论 Apple 接受还是拒收都以 0 退出，所以退出码本身无法决定产物能否
发布：判定从 JSON 里读，被拒时会连同 submission id 一起报出——那是向 Apple 追问原因唯一需要的
东西：

```
Apple rejected the notarization submission <id> with status "Invalid"; run
`xcrun notarytool log <id> --keychain-profile <profile>` to see why
```

没有它，被拒只会在随后的 `stapler` 失败中体现，而那个失败对原因只字不提。pacor 命令行上的
`--verbose` 会给 `notarytool` 和 `stapler` 都加上 `--verbose`。

## 缓存布局

后端不需要下载的东西都在

```
~/Library/Application Support/pacor/macos/
  create-dmg.sh                    # 内嵌脚本，每次运行都重写
  support/template.applescript
  support/eula-resources-template.xml
```

`pacor cache clean` 会删除整个缓存目录（`--dry-run` 只列出将删除的内容），下次构建会重新写入。
除此之外没有别的缓存：Flutter 的构建输出在项目里，由 `clean_build_directories`（`build/macos`）
清理。

## 复用已有 Flutter 构建

`--use-existing-flutter-build` 打包 `build/macos/Build/Products/<mode>/` 里已有的 bundle，跳过
上面的第 2 步。其余步骤照常执行——安装打包工具、本地化并写入元数据、签名，并产出 DMG、更新归档
与校验值。缺少 `<mode>` 目录时会以 `Flutter build output directory does not exist` 失败。

## 已知限制

- **只能在 macOS 上构建。** 该 crate 在任何主机上都会编译并测试，但真正的构建需要 `xcodebuild`
  与 `hdiutil`。
- **嵌套代码签名只覆盖一层。** 签名计划遍历 `Contents/Frameworks` 的直接子项；更深层的代码由
  bundle 的签名覆盖，但不会被单独签名。
- **票据只装订到镜像。** 更新归档里的 `.app` 不做装订：它由更新器就地替换，而 Gatekeeper 检查的
  是磁盘上的那份副本。
- **`Info.plist` 必须是 XML。** 二进制 plist 无法就地编辑，读取失败会指出路径以及修复它的
  `plutil -convert xml1` 命令。
- **窗口布局需要 Finder 自动化。** 没有它就别设置 `dmg_background` 与 `dmg_window_size`，让 DMG
  走 sandbox-safe 路径。
- **`[app.i18n]` 本地化的是 bundle，不是镜像。** 卷名与产物名使用项目名。

## 故障排查

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `Platform tool not found: hdiutil (required for macOS)` | 主机不是 macOS，或 `PATH` 里没有该工具 | 在 macOS 上构建，用 `which hdiutil` 确认；`codesign` 只在签名时需要，`xcrun` 只在公证时需要 |
| `failed to read .../Info.plist (... `plutil -convert xml1` ...)` | 构建写出的是二进制 plist，它不是文本，无法就地编辑 | 先转换（`plutil -convert xml1 <路径>`）再重新构建 |
| `Apple rejected the notarization submission <id>` | 原因在 Apple 的日志里，而不在退出码里 | `xcrun notarytool log <id> --keychain-profile <profile>` |
| `notarytool` 报 `code object is not signed at all` | DMG 里的载荷没有签名 | 配置 `codesign_identity`：pacor 会依次签嵌套代码、bundle、镜像 |
| Gatekeeper 拒绝下载到的 DMG 或 app | 副本未签名，或已签名但未公证/未装订票据 | 同时配置 `codesign_identity` 与 `notarize_credentials`；用 `spctl -a -vvv -t install dist/<name>-<version>-<arch>.dmg` 检查结果 |
| 安装窗口忽略 `dmg_background` / `dmg_window_size` | 窗口由 Finder 的 AppleScript 布局，需要图形会话与 Finder 自动化权限 | 在已登录的会话里构建，或去掉这两项、接受默认布局 |
| `codesign_entitlements` 指向的路径不是文件 | 该路径相对项目根目录解析 | 修正路径，或去掉该项以使用 Flutter 生成的 entitlements |
| 带 `--use-existing-flutter-build` 时 `Flutter build output directory does not exist` | `build/macos/Build/Products/<mode>/` 不存在 | 先不带该开关构建一次 |
| `could not read the notarization result: ...` | Apple 改变了 `notarytool` 的 JSON 结构 | 原始输出就在错误信息里；提交本身不受影响，可用 `xcrun notarytool log` 查看 |
