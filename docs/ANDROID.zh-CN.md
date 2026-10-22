<p align='center'>
<a href="ANDROID.md">English</a> | <span>中文</span>
</p>

# Pacor Android 平台说明

本文档说明 `pacor build --target android`（`pacor android` 是其别名，接受同样的
`--use-existing-flutter-build` 与 `--skip-cleanup` 开关）的产物、配置、签名流程、
已知限制与常见排查方法。配置项的完整注释模板见
[`crates/pacor-core/src/config/template.toml`](../crates/pacor-core/src/config/template.toml)，
GitHub 镜像相关环境变量见 [ENVIRONMENT_VARIABLES.md](ENVIRONMENT_VARIABLES.zh-CN.md)。

> 与其他平台不同，Android 后端**不下载任何工具或模板**，也不使用 `%APPDATA%\pacor` 缓存：
> 它只是调用本机的 `flutter`，工具链（构建与签名）都来自你已安装的 Flutter / Android SDK / JDK。

Android 能产出两种格式，**默认只产出 APK**：

| `formats` | 产物 | 用途 |
| --- | --- | --- |
| `["apk"]`（默认） | 每个 ABI 一个 `.apk` | 直接安装：官网下载、企业/MDM 分发、侧载、应用内自更新 |
| `["apk", "aab"]` | 上面的 APK + 一个 `.aab` | 额外产出一份用于上传 Google Play |
| `["aab"]` | 只产出 `.aab` | 只走 Play 的工程 |

AAB（Android App Bundle）是**分发格式而不是安装包**：它把所有 ABI、屏幕密度和语言打进一个文件，
Google Play 再用它按设备生成实际安装的 APK；它不能直接安装到设备上（本地测试需要
`bundletool build-apks` 或 Play 的测试轨道）。因此它是 APK 的补充，而不是替代，而且只在需要
上 Play 时才值得开启——有些工程只发布 APK，那就保持默认。

## 产物

一次成功的构建会在 `[build] output_dir`（默认 `dist/`）下写入所选格式的产物，每个都带一个
同名 `.sha1`：

| 文件 | 说明 |
| ---- | ---- |
| `<name>-<version>-<abi>.apk` | 按 ABI 拆分的 APK，`target_abi` 中每个 ABI 一个 |
| `<name>-<version>.aab` | Android App Bundle，无论多少个 ABI 都只有一个文件 |
| 上述文件的 `.sha1` | 与产物同名的 SHA1 校验文件 |

`<name>` 是 `pubspec.yaml` 的项目名（`name:`，不是 `[app.i18n]` 的 `display_name`），
`<version>` 是原始版本号（例如 `1.0.0+1`），`<abi>` 是 Android NDK 的 ABI 名
（`arm64-v8a` / `armeabi-v7a` / `x86_64`）。例如：

```
dist/probe_app-1.0.0+1-arm64-v8a.apk
dist/probe_app-1.0.0+1.aab
```

`.sha1` 在**签名之后**生成：签名会改写文件内容，先算校验和等于发布一个错误的值
（Windows 后端出于同一原因也是先签名后校验，见 [WINDOWS.md](WINDOWS.zh-CN.md)）。

pacor 只把产物复制出来并按上面的规则改名，其余内容（`versionCode`、`versionName`、
`applicationId`、图标、权限）完全由 Flutter / Gradle 决定。

## 配置

```toml
[platforms.android]
enabled = true
target_abi = ["arm64-v8a"]        # 默认 ["arm64-v8a"]；至少一项
formats = ["apk"]                 # 默认 ["apk"]；要上 Play 就写成 ["apk", "aab"]
# flavor = "prod"                 # 对应 flutter build … --flavor prod
# 签名相关（可选，配置 keystore_file 即对所选格式全部开启）
# keystore_file = "android/app/upload-keystore.jks"   # 相对项目根目录
# key_alias = "upload"
# signing_tool_path = "C:/Android/sdk/build-tools/35.0.0/apksigner.bat"  # 只给 apk 用
# jarsigner_path = "C:/Program Files/Android/Android Studio/jbr/bin/jarsigner.exe"  # 只给 aab 用
# keystore_password = "..."       # 建议改用 PACOR_ANDROID_KEYSTORE_PASSWORD
# key_password = "..."            # 建议改用 PACOR_ANDROID_KEY_PASSWORD
```

`enabled = false` 时该目标直接报错（`Platform not enabled: Android`），与其它平台一致。
`formats = []` 是配置错误：它会让构建什么都不产出，因此直接被拒绝。
`--format apk` / `--format aab` 可以在命令行上覆盖 `formats`（与 `--target linux` 的 `--format`
语义一致，多个值用重复的 `--format`）。

## 构建流程

1. 校验环境：`flutter` 在 `PATH` 中、项目根目录有 `pubspec.yaml` 与 `android/` 目录、输出目录
   可写（不存在时创建）；还会校验 `formats`/`target_abi` 非空，若开启了签名，则按所选格式分别
   校验 `keystore` 与所需的签名工具（见下）。
2. 执行（等价命令）：

   ```bash
   # formats 含 apk 时
   flutter build apk --release --split-per-abi --split-debug-info=tmp/ \
       [--flavor <flavor>] [--target-platform android-arm64,android-x64,…]
   # formats 含 aab 时（在 APK 之后执行，先跑便宜的那一步）
   flutter build appbundle --release --split-debug-info=tmp/ \
       [--flavor <flavor>] [--target-platform android-arm64,android-x64,…]
   ```

   `--target-platform` 的值来自 `target_abi`，顺序固定（`armeabi-v7a` → `arm64-v8a` →
   `x86_64`），因此同一份配置每次生成完全相同的命令行。对 APK 它决定产出哪些文件；对 AAB 它
   决定**这唯一一个文件里包含哪些 ABI**——所以只列 `arm64-v8a` 时，Play 上的包只支持 64 位设备
   （Play 本身还会按设备和语言再拆分，所以通常把 `armeabi-v7a` 也列上更稳妥）。
3. 定位每个产物（APK 见“APK 产物定位”，AAB 见“AAB 产物定位”）。
4. 复制到 `dist/` 并按配置签名，然后生成 `.sha1`。
5. 除非 `--skip-cleanup`，构建结束后执行 `flutter clean` 并删除 `tmp/` 符号目录。

### 调试符号与 `tmp/`

`--split-debug-info=tmp/` 让 Dart AOT 符号写入**项目根目录下的 `tmp/`**（与 Windows、macOS、
Linux 后端一致），APK 因此更小。这些 `.symbols` 文件是 `flutter symbolize` 还原线上堆栈所必需的：

- 默认构建结束后它们会被删除；需要保留时用 `--skip-cleanup`，或从 `tmp/` 里先把它们拷走。
  注意 `pacor clean` 删除的是 `dist/` 与 `build/`，**不含** `tmp/`；
- Android 后端**不做 Dart 混淆**（不传 `--obfuscate`）：符号用于还原方法名与行号；
  混淆需要 `--obfuscate`，目前不是 pacor 的配置项。

### 构建模式

Android 后端始终构建 **release**（APK 与 AAB 都是），`[build] flutter_build_mode` 与
`PACOR_BUILD_MODE` 对它无效——APK 的构建模式体现在文件名里而不是目录里，因此
“Android 始终构建 release”是刻意的行为，但这也意味着这两个配置项在 Android 上被静默忽略。

### APK 产物定位

Flutter 的 Gradle 插件把拆分 APK 复制到 `build/app/outputs/flutter-apk/` 并命名为
`app<-abi>?<-flavor-name>?-<build-mode>.apk`。文件名里带着本次构建所用的 flavor，所以
pacor 只找这次构建可能写出来的文件：

1. `flavor` 已配置时找 `app-<abi>-<flavor>-release.apk`，否则找 `app-<abi>-release.apk`；
2. 目录中唯一匹配 `app-<abi>-*-release.apk` 的文件。配置了 `flavor` 时，该候选必须带同一个
   flavor（比较时忽略大小写，因为 AGP 某些版本会把 flavor 转成小写）；没有配置 `flavor` 时，
   唯一候选直接采用——这正是 `--use-existing-flutter-build` 的情形：flavor 由构建该 APK 的
   那一方决定。

带**另一个** flavor 的文件、或有 flavor 构建旁边的无 flavor 文件名，都不可能是本次构建的
产物：`flutter build apk --flavor prod` 只会写出带 flavor 的名字，这类文件来自上一次构建，
打包它就会把错误的 APK 签名后当作本次发布件交付。匹配到多个候选时构建失败并列出候选，
不会随便挑一个；一个都没匹配到时会列出该目录实际包含的文件——这样配置了 `flavor` 却忘了
写进 `pacor.toml` 的工程也会得到可读的报错，而不是一句 `系统找不到指定的文件。 (os error 2)`。

### AAB 产物定位

AAB 的路径规则和 APK 不同：它只有一个文件，而且放在**带构建模式（和 flavor）的目录**里：

```
build/app/outputs/bundle/release/app-release.aab                  # 无 flavor
build/app/outputs/bundle/<flavor>Release/app-<flavor>-release.aab # 有 flavor
```

（这与 Flutter 自己在 `findBundleFile` 中使用的规则一致；AGP 各版本对目录大小写的处理不同，
因此 pacor 先按 `release/`、`<flavor>Release/`、`<flavor>release/` 精确尝试，再做一次
大小写不敏感的扫描。）

包在 flavor 目录下、但配置里没写 `flavor` 时，pacor **不会**猜着打包：它报错并列出找到的
`.aab`，同时提示设置 `platforms.android.flavor`——这种文件通常是别的 flavor 或上次构建的
残留，静默打包等于发布错误的东西。无 flavor 的文件名同样是精确匹配：`release/` 下只接受
`app-release.aab`，因此被改名的残留（如 `app-release-unsigned.aab`）只会被报出来，不会被
打包。多个候选同理，会列出候选名。

## ABI

| `target_abi` | `--target-platform` | Flutter 产物文件名 |
| --- | --- | --- |
| `"armeabi-v7a"`（别名 `"arm"`） | `android-arm` | `app-armeabi-v7a-release.apk` |
| `"arm64-v8a"`（别名 `"arm64"`） | `android-arm64` | `app-arm64-v8a-release.apk` |
| `"x86_64"`（别名 `"x64"`） | `android-x64` | `app-x86_64-release.apk` |

`target_abi` 是**集合**，重复项会被去重；`[]` 是配置错误（旧的实现会静默退回“只打包
arm64-v8a”，同时让 Flutter 构建全部 ABI，导致产物比配置少）。32 位 `x86` 无法构建 release，
因此不在支持列表中（见 [Flutter 文档](https://flutter.dev/to/android-supported-architectures)）。

## 签名

**配置 `keystore_file` 即开启签名**，对 `formats` 里的每个格式都生效；下列各项都是必填项，
缺失直接报错而不是静默跳过：

| 项 | 来源 |
| --- | --- |
| keystore 文件 | `platforms.android.keystore_file`（相对路径按项目根目录解析，必须存在） |
| 别名 | `platforms.android.key_alias` |
| keystore 口令 | `PACOR_ANDROID_KEYSTORE_PASSWORD` 优先，其次 `platforms.android.keystore_password` |
| key 口令 | `PACOR_ANDROID_KEY_PASSWORD` 优先，其次 `platforms.android.key_password` |
| APK 签名工具 | `signing_tool_path` → `PATH` 中的 `apksigner` → Android SDK 的 build-tools |
| AAB 签名工具 | `jarsigner_path` → `PATH` 中的 `jarsigner` → `JAVA_HOME` 或常见 JDK 位置 |

> 只配置工具路径不会开启签名（这一点与 Windows 的
> `certificate_path`/`signing_tool_path` 都算开启不同）：Android 的签名主体是 keystore，
> 两个工具路径只是查找提示。工具是按格式**按需**校验的：`formats = ["aab"]` 时不会要求
> `apksigner`，`formats = ["apk"]` 时不会要求 `jarsigner`。

`pacor check --target android` 会连同 keystore 是否存在、口令是否提供、所选格式需要的签名
工具是否可定位一起校验，因此配置问题在构建前就能发现（与 [WINDOWS.md](WINDOWS.zh-CN.md) 的
`signtool` 校验一致）。

### `apksigner` 的查找顺序（APK）

1. `platforms.android.signing_tool_path`（必须存在）；
2. `PATH` 中的 `apksigner`（Windows 上同时尝试 `apksigner.bat`）；
3. Android SDK 的 build-tools，取版本号最高的一个：

   ```
   <sdk>/build-tools/<version>/apksigner[.bat]
   ```

   `<sdk>` 依次尝试 `ANDROID_HOME`、`ANDROID_SDK_ROOT`、`<项目根>/android/local.properties`
   里的 `sdk.dir`。

签名命令（`apksigner` 的参数风格）：

```
apksigner sign --ks <keystore> --ks-key-alias <alias> \
    --ks-pass <口令来源> --key-pass <口令来源> <apk>
```

口令来源有两种写法：口令来自环境变量时用 `env:PACOR_ANDROID_KEYSTORE_PASSWORD`
（不出现在进程命令行里，这是推荐路径），来自配置文件时用 `pass:<明文>`
（与 Windows 后端把口令作为 `signtool /p` 参数的做法相同，会出现在进程列表中，因此不建议）。

签名方案（v1/v2/v3）交给 `apksigner` 按 APK 自身的 `minSdkVersion` / `targetSdkVersion`
决定，pacor 不传 `--v*-signing-enabled`。APK 的 zipalign 对齐由 AGP 的 release 构建完成，
`apksigner` 重签会保留对齐，因此 pacor 不需要额外的 `zipalign` 步骤。

两处例外，都是为了让 `dist/` 里只有 pacor 报告并校验过的文件：

- **v4 签名被显式关闭**（`--v4-signing-enabled false`）。它是 `apksigner` 的默认行为之一，会在
  APK 旁边再写一个 `<apk>.idsig`（用于 `adb install --incremental`）。该文件既不在 pacor 的
  产物列表里，也没有 `.sha1`，留着会让输出目录比 pacor 声明的多出文件。需要它的话手动执行
  `apksigner` 即可。
- **`JAVA_HOME` 由 pacor 补齐**：`apksigner.bat` 只是个调用 `java` 的包装脚本，没有 `JAVA_HOME`
  也不在 `PATH` 上时它会直接以
  `ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH` 退出。
  Flutter 给自己跑 Gradle 时会用 Android Studio 的 JBR，但只运行 pacor 的终端通常什么都没配，
  所以 pacor 会自己找一个 JDK（顺序同上）并通过 `JAVA_HOME` 传给 `apksigner`；
  环境里已经有 `JAVA_HOME` 时不动它。

### `jarsigner` 的查找顺序与 AAB 签名（AAB）

AAB 是 JAR，用的是 **JAR 签名**，`apksigner` 的签名方案对它不适用，所以走 `jarsigner`：

1. `platforms.android.jarsigner_path`（必须存在）；
2. `PATH` 中的 `jarsigner`；
3. `JAVA_HOME/bin/jarsigner[.exe]`，然后是常见 JDK 位置：Windows 上
   `%ProgramFiles%\Android\Android Studio\jbr\bin\jarsigner.exe`（以及 `%LOCALAPPDATA%` 下的
   用户级安装），macOS 上 `/Applications/Android Studio.app/Contents/jbr/Contents/Home`，
   Linux 上 `/opt/android-studio/jbr` 与 `/usr/lib/jvm/*`。

签名命令：

```
jarsigner -keystore <keystore> -storepass[:env] <口令> -keypass[:env] <口令> \
    -digestalg SHA-256 <aab> <alias>
```

口令来自环境变量时用 `-storepass:env PACOR_ANDROID_KEYSTORE_PASSWORD` /
`-keypass:env PACOR_ANDROID_KEY_PASSWORD`（已用 JDK 21 的 `jarsigner` 实测可用），来自配置
文件时按字面量传入。`-digestalg` 固定为 SHA-256，避免老 JDK 默认的 SHA-1 让 Play 拒收。
**签名算法不再固定**：`jarsigner` 会按私钥类型自行选择（RSA 用 SHA256withRSA，EC 用
SHA256withECDSA），写死 SHA256withRSA 会让所有 EC 密钥库报 "signature algorithm
SHA256withRSA not compatible with key"。

**签名前会先剥离 Gradle 已有的 JAR 签名。** `flutter build appbundle` 会用工程的 Gradle
签名配置给 AAB 签名（没有配置时就是 **debug** keystore），而 `jarsigner` 是**追加**签名而不是
替换，因此不处理的话发布的 AAB 会同时带着 debug 证书和 upload key 两份签名——已实测：直接追加
后包内同时存在 `META-INF/ANDROIDD.*` 与 `META-INF/UPLOAD.*`。所以 pacor 在签名前删掉
`META-INF/MANIFEST.MF` 与 `META-INF/*.SF|RSA|DSA|EC|SIG`，再由 `jarsigner` 重建清单：
发布出去的 AAB 只有一个签名者，就是配置的那把 upload key（`META-INF/` 下的其他内容，例如
`META-INF/services/…`，属于有效负载，会被保留）。APK 不需要这一步：`apksigner` 会重写 APK
的签名块。

> Play App Signing：上传用的是 **upload key**，Google 会用它再以应用签名密钥重签分发给用户的
> APK。因此 pacor 只需要保证"用 upload key 正确签好"，不需要也不能代替最终签名。

### 签名者校验与 debug 签名告警

**没有**配置 `keystore_file` 时，pacor 不修改产物，其签名完全取决于工程的
`android/app/build.gradle(.kts)`。Flutter 生成的模板在 release 构建里用的是 **debug keystore**
（模板原注释：*Signing with the debug keys for now*），所以未配置签名的 APK 与 AAB 默认都是
debug 签名的：Google Play 会直接拒收，且以后换成正式密钥也无法覆盖升级（APK）。

因此未配置签名时，pacor 会分别检查：

- APK：`apksigner verify --print-certs`；
- AAB：`jarsigner -verify -verbose -certs`（Gradle 写进包里的签名保持原样，pacor 只读不动）。

签名者主题是 `CN=Android Debug` 时给出明确告警，提示配置 `platforms.android.keystore_file`；
AAB 还会额外提示"Play 要求 upload key 签名"。找不到对应工具时跳过该校验（只记 debug 日志），
不会因此让构建失败。

## 已知限制

| 限制 | 说明 |
| ---- | ---- |
| AAB 不能直接安装 | 它是 Play 的分发格式，本地验证要 `bundletool build-apks` 或 Play 测试轨道 |
| `aab` 需要显式开启 | 默认 `formats = ["apk"]`：只发布 APK 的工程不必为一个用不上的文件买单 |
| 不做 Dart 混淆 | 不传 `--obfuscate`；符号仍写入 `tmp/`，默认在构建结束时删除（`--skip-cleanup` 可保留） |
| 忽略构建模式 | 始终 release，`flutter_build_mode` / `PACOR_BUILD_MODE` 无效 |
| 不使用 `[app.i18n]` | 两种产物都没有安装界面，本地化由应用自身与 `AndroidManifest.xml` 决定 |
| 单 flavor | `flavor` 是单个值，一次构建只产出一个 flavor 的 APK/AAB |
| 不校验产物内容 | 不改 `versionCode`/`applicationId`，也不校验 `dist/` 里已有文件是否来自本次构建 |
| 不产出 `.idsig` | v4 签名被关闭，`dist/` 只放 pacor 报告并校验过的产物（见“签名”） |
| 不代管 Play 上传 | pacor 只产出并签名 AAB；上传、Play App Signing、内测轨道都要你自己做 |

## 常见失败与排查

### `Platform tool not found: flutter (required for Android)`

`flutter` 不在 `PATH` 中。Android 打包只需要 `flutter`——**不需要 `adb`**（旧实现把 `adb` 列
为必需工具，导致装了 Android SDK 但没把 `platform-tools` 加进 `PATH` 的机器无法打包）。

### `Configuration validation failed: platforms.android.formats`

配置成了 `formats = []`。删掉这一行回到默认的 `["apk"]`，或写成 `["apk", "aab"]`。

### `Configuration validation failed: platforms.android.target_abi`

配置成了 `target_abi = []`。删掉这一行即可回到默认的 `arm64-v8a`。

### `Flutter did not produce an APK for <abi>`

Flutter 没有产出该 ABI 的 APK，或产出的文件不带配置的 `flavor`——最常见的原因是工程的
Gradle flavor 与 `platforms.android.flavor` 不一致，或者目录里还留着上一次构建的文件。
报错会给出期望的文件名并列出目录内容；确认 `platforms.android.flavor` 后重试。
`several APKs match <abi>` 则表示同目录下有多个候选（例如一次构建了多个 flavor），
此时必须显式指定 `flavor`。

### `Flutter did not produce an Android App Bundle`

同上，但对象是 `build/app/outputs/bundle/`。报错会给出期望路径（带 flavor 时连同
`<flavor>Release/app-<flavor>-release.aab`）与目录里实际存在的 `.aab`；如果那些 `.aab` 位于
某个 flavor 目录下，报错会提示设置 `platforms.android.flavor`。
`several App Bundles match` 表示有多个候选，同样需要显式指定 `flavor`。

### Gradle 下载超时（`Connection timed out`）

真正的 Android 构建由 Gradle 完成，Gradle 发行版与依赖（AGP、Kotlin、AndroidX）来自
`services.gradle.org`、`google()`、`mavenCentral()`。**pacor 不代理也不配置这些仓库**，所以在
网络受限的环境里看到的是 Gradle 自己的报错：

```
Exception in thread "main" java.net.ConnectException: Connection timed out: connect
    at org.gradle.wrapper.Download.downloadInternal(Download.java:58)
```

这条来自 Gradle **wrapper**，说明连 Gradle 发行版都没下下来；如果报错是
`Could not resolve com.android.tools.build:gradle`，则是依赖仓库的问题。两处都要配镜像，
都改在工程的 `android/` 下（pacor 不参与，也不需要重新配置）：

1. Gradle 发行版，`android/gradle/wrapper/gradle-wrapper.properties`：

   ```properties
   # 腾讯云/华为云各版本齐全；阿里云镜像不是每个版本都有 -all
   distributionUrl=https\://mirrors.cloud.tencent.com/gradle/gradle-8.12-all.zip
   ```

2. 依赖仓库，`android/settings.gradle.kts` 的 `pluginManagement.repositories` **和**
   `android/build.gradle.kts` 的 `allprojects.repositories`（AGP/Kotlin 插件走前者，两处都要加）：

   ```kotlin
   maven { url = uri("https://maven.aliyun.com/repository/google") }
   maven { url = uri("https://maven.aliyun.com/repository/public") }
   maven { url = uri("https://maven.aliyun.com/repository/gradle-plugin") }
   // 保留官方仓库作为兜底
   google()
   mavenCentral()
   ```

   也可以写进用户级 `~/.gradle/init.gradle`，一次覆盖所有工程。Flutter 自己的引擎产物走
   `FLUTTER_STORAGE_BASE_URL`（pacor 同样不参与）。实测这套配置能把一个全新 Flutter 工程的首次
   release 构建从"卡在下载"变成可完成（见文末审计记录）。

### `apksigner could not verify the APK: ERROR: JAVA_HOME is not set ...`

`apksigner` 需要 JDK。pacor 会在环境里没有 `JAVA_HOME` 时自动补一个（见“签名”），所以这条通常
只在 pacor **找不到**任何 JDK 时出现：装一个 JDK 或 Android Studio，或设置 `JAVA_HOME`。

### `platforms.android.keystore_file` 相关报错

`... does not exist` 检查路径（相对项目根目录）；`set PACOR_ANDROID_KEYSTORE_PASSWORD ...` 表示
口令缺失（`... to sign APKs` / `... to sign App Bundles` 会告诉你是哪种格式需要它）。
四个必填项中的任何一个缺失都会在构建开始前失败。

### `no signing tool found: install the Android SDK build-tools ...`

`apksigner` 不在 `PATH`、也不在 SDK build-tools 中。用 Android SDK Manager 安装 build-tools，
或设置 `platforms.android.signing_tool_path`。

### `no App Bundle signing tool found: install a JDK ...`

`jarsigner` 既不在 `PATH`、也不在 `JAVA_HOME` 或常见 JDK 位置。装一个 JDK（或 Android Studio，
它自带 JBR），或设置 `platforms.android.jarsigner_path`。

### `... is not a readable Android App Bundle (expected a zip)`

`build/app/outputs/bundle/...` 里的文件不是有效的 zip/AAB（例如被别的东西覆盖了）。重新执行
`flutter build appbundle`，或去掉 `--use-existing-flutter-build`。

### APK 或 AAB 是 debug 签名（`CN=Android Debug`）

工程没有配置 release 签名，或没有在 `pacor.toml` 里配置 `keystore_file`。按 Flutter 官方流程
生成 upload keystore，然后设置 `platforms.android.keystore_file` / `key_alias` 与两个口令环境变量。
AAB 尤其要注意：Play 只接受 upload key 签名的包。

## 开发验证

```bash
cargo test -p pacor-packaging-android
```

两套测试都**不需要真实的 Flutter 安装、Android SDK 或 JDK**（端到端测试自己放 `flutter` 与
`jarsigner` 桩），因此它们随 `cargo test --workspace` 在 CI 的三个平台上运行：

- 单元测试（`src/`）：ABI 与文件名映射、APK 产物定位（精确名、flavor、唯一候选、多候选失败、
  目录为空、目录不存在）、AAB 产物定位（`release/`、大小写不同的 flavor 目录、未被配置的
  flavor、多候选、缺失）、JAR 签名剥离（条目判定、负载保留、失败时不留临时文件）、
  `apksigner` SDK 候选排序、`local.properties` 转义解码、`--print-certs` 与
  `jarsigner -verify` 输出解析、debug 证书识别、两种口令来源写法、JDK 位置的 `jarsigner`
  查找、配置层默认值/去重/相对路径解析，以及“空 `target_abi`/空 `formats` 是配置错误”。
- 端到端测试（`tests/android_smoke.rs`）：用一个手写的 fixture 工程和三个只记录参数的桩
  （`flutter`、`jarsigner`、`apksigner`），跑通 `validate` → `build` → 产物收集与命名 → 签名 →
  `.sha1` → `clean`，并断言 `flutter build apk`/`appbundle` 的完整参数向量（只有 APK 那一条带
  `--split-per-abi`）、`apksigner` 的完整签名参数（含 `--v4-signing-enabled false`）、两种格式都被
  打包、bundle-only 配置不产 APK、AAB 签名会先剥离 Gradle 签名且保留负载、未由 pacor 签名的
  APK/AAB 保持原样、校验和覆盖发布文件、`clean` 删除 `tmp/`，以及空 `formats`、空 `target_abi`、
  缺失 keystore、缺失口令、ABI 产物缺失、AAB 缺失这几条错误路径。

仍然没有自动化测试的部分是**真实链路**（`flutter build apk`/`appbundle` 真的出包、两个签名工具
真的签名）：它需要 runner 上装有 Flutter + Android SDK + JDK，`.github/workflows` 目前不带这些
工具链。这条链路已经在本机手工跑通并被记录在文末（含真实 Gradle 构建、真实 `apksigner`/
`jarsigner` 签名与独立校验）。

快速回归时可以用已有的构建产物，跳过耗时的 Gradle 步骤：

```powershell
pacor check --target android
pacor build --target android --use-existing-flutter-build --skip-cleanup
```

### AAB 支持（同日追加）

上面的审计只覆盖了"只产出 APK"的实现。AAB 支持是随后按同一份清单加的，过程中又实测出两个
只有真实工具链才会暴露的问题：

| 发现 | 表现（真实 `jarsigner` 实测） | 处理 |
| ---- | ---- | ---- |
| `jarsigner` 是**追加**签名而非替换 | 用 upload key 给一个已由 Gradle（debug key）签名的 AAB 签名后，包内同时存在 `META-INF/ANDROIDD.*` 与 `META-INF/UPLOAD.*`——发布出去等于把 debug 证书一起带上 | 签名前剥离 `META-INF/MANIFEST.MF` 与 `META-INF/*.SF\|RSA\|DSA\|EC\|SIG`（`zip` 重写），再由 `jarsigner` 重建清单；实测发布包只剩 `META-INF/UPLOAD.*`，`jarsigner -verify` 只报 `CN=My Upload Key` |
| AAB 的 ABI 语义与 APK 不同 | AAB 只有一个文件，`--target-platform` 变成"包内含哪些 ABI" | `target_abi` 同时作为 APK 的产出维度与 AAB 的内容限制，并在本文档与模板注释里写明"只留 arm64 会让 Play 上的包只支持 64 位设备" |

真实链路验证方式（本机，Windows + Android Studio JBR 的 JDK 21）：

```powershell
# 1) 造两把 keystore：一把冒充 Android debug key，一把当 upload key
keytool -genkeypair -keystore debug.keystore -storepass android -keypass android `
  -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" -keyalg RSA -validity 10000
keytool -genkeypair -keystore upload.jks -storepass <商店口令> -alias upload `
  -dname "CN=My Upload Key,OU=Mobile,O=Example Ltd,C=CN" -keyalg RSA -validity 10000
# 2) 用 jarsigner 手工签一个替身 AAB（zip）并放进 build/app/outputs/bundle/release/
# 3) 不配 keystore 跑 pacor：应告警 CN=Android Debug，且不改动包内签名
# 4) 配 keystore_file 再跑：应剥离旧签名并用 upload key 重签，.sha1 覆盖签名后的字节
jarsigner -verify -verbose -certs dist/<name>-<version>.aab   # 应只剩 CN=My Upload Key
```

### 真实打包实测（同日，真实 Flutter 工程）

上面几步用的是替身产物。之后又用 `flutter create --platforms=android` 建的真实工程
（`pacor_probe`，`formats = ["apk", "aab"]`、`target_abi = ["arm64-v8a", "armeabi-v7a"]`）
跑了一遍完整链路：

| 步骤 | 结果 |
| ---- | ---- |
| `flutter build apk`（经 pacor） | `app-armeabi-v7a-release.apk` 5.7 MB、`app-arm64-v8a-release.apk` 6.3 MB |
| `flutter build appbundle`（经 pacor） | `app-release.aab` 23.8 MB |
| `dist/` 产物 | 2 个 APK + 1 个 AAB，各带 `.sha1`；三个校验和都与发布文件一致 |
| 未配 keystore | 三个产物都被正确识别为 debug 签名（`C=US, O=Android, CN=Android Debug`）并告警 |
| 配 keystore | 用真实 `apksigner` 给 2 个 APK、真实 `jarsigner` 给 AAB 重签；独立校验：APK 的 `Signer #1` 与 AAB 的 `X.509` 都只剩 `CN=Pacor Upload Key`，AAB 的 `META-INF/` 只有 `MANIFEST.MF`/`UPLOAD.SF`/`UPLOAD.RSA`（Gradle 的 `ANDROIDD.*` 已被剥离） |
| 产物内容（`aapt2 dump badging`） | `com.example.pacor_probe`、`versionCode=1`、`versionName=1.0.0`、`minSdkVersion=21`、每个 APK 的 `native-code` 只含自己那个 ABI |
| `tmp/` 符号 | `app.android-arm64.symbols`（1.39 MB）、`app.android-arm.symbols`（1.17 MB），`--skip-cleanup` 下保留 |

这一轮又暴露了两个只有真实工具链才会出现的问题，都已修掉：

| 发现 | 表现 | 处理 |
| ---- | ---- | ---- |
| `apksigner.bat` 需要 `JAVA_HOME` | 终端里既没有 `java` 也没有 `JAVA_HOME`（Flutter 给自己跑 Gradle 时会用 Android Studio 的 JBR，pacor 不会）→ `apksigner` 退出并打印 `ERROR: JAVA_HOME is not set...`，pacor 只能报一句无用的 `apksigner verify failed`（该错误在 **stdout**，而当时的兜底只读 stderr） | pacor 自己解析 JDK 位置并通过 `JAVA_HOME` 传给 `apksigner`（环境里已有则不动）；失败原因改为 stderr→stdout 依次取第一行 |
| `apksigner` 默认写 `.idsig` | v4 签名默认开启，`dist/` 里多出 `<apk>.idsig`（58 KB / 54 KB），既不在产物列表也没有 `.sha1` | 签名时传 `--v4-signing-enabled false`，`dist/` 只留 pacor 报告并校验过的文件 |
| 真实签名者 DN 的顺序 | 真实输出是 `C=US, O=Android, CN=Android Debug`，与替身里假设的 `CN=Android Debug, O=…` 不同 | debug 判定用子串匹配而非等值比较（原本就是这样，现已被真实输出与测试固定下来） |

> 这台机器上没有连接 Android 设备或模拟器（`adb devices` 为空），因此没有做 `adb install`
> 的安装验证；APK 的可安装性只通过 `apksigner verify` 与 `aapt2 dump badging` 间接确认。
> Gradle 首次构建还卡在下载（`services.gradle.org` 超时、Maven 依赖慢），按
> “Gradle 下载超时”一节换成镜像后 275 s 完成首次构建。

## 附：2026-10-17 审计发现的缺陷与处理

审计基于当时的 `README.md`、`README.zh-CN.md`、`docs/MSI.md`、`docs/WINDOWS.md`、
`docs/ENVIRONMENT_VARIABLES.md` 与 `crates/pacor-packaging-android`（单文件 250 行、0 个测试）。
实测环境：Windows + Rust 1.99 + Flutter 3.32.8，本机装有 Android SDK（`platform-tools/adb.exe`
存在但不在 `PATH`）；为绕开 `flutter build` 的耗时，用 `--use-existing-flutter-build` 直接驱动
“收集 → 复制 → 签名”阶段，并在 `build/app/outputs/flutter-apk/` 放置替身 APK。

| 缺陷 | 当时的表现（实测） | 现在的处理 |
| ---- | ---- | ---- |
| 签名是空实现（`TODO` + `warn!` + `Ok(())`） | 配置了 keystore（路径甚至不存在）与两个口令，退出码 0、打印 `Build Completed Successfully`，`dist/` 里的 APK 与 Flutter 输出的 SHA1 完全相同——一个字节都没改 | 实现 `apksigner` 签名；签名配置不完整或工具不可用时**构建失败**；未被签名者校验覆盖的情形给出 debug 签名告警 |
| 签名门禁逻辑相反 | `keystore_file` 有、口令缺 → 硬失败 `keystore_password not configured`；口令齐 → 静默跳过签名 | 四个必填项统一在 `validate()` 中校验；`sign()` 失败即构建失败 |
| `pacor init` 模板教用户写无效 TOML 键 | `[platforms.android]` 下的 `PACOR_ANDROID_KEYSTORE_PASSWORD="..."` 被 serde 静默忽略（无 `deny_unknown_fields`），`pacor config --full` 正常且不显示——用户以为签名已配置 | 模板改写为 `keystore_password` / `key_password` 注释并说明环境变量优先级 |
| 硬性要求 `adb` | 本机 `platform-tools\adb.exe` 存在但不在 `PATH`，`pacor check --target android` 与 `pacor build --target android` 均在 `validate` 阶段以 `Platform tool not found: adb (required for Android)` 退出；把 `platform-tools` 加入 `PATH` 后立刻成功 | 必需工具只剩 `flutter` |
| 符号文件写入 `TempDir` 后立即删除 | `--split-debug-info <OS 临时目录>`，`build_apk` 返回时目录被析构；`--skip-cleanup` 也留不下，线上堆栈无法还原 | 改为项目内 `tmp/`，`clean()` 负责删除，`--skip-cleanup` 可保留 |
| 忽略 `flavor`，产物定位靠硬编码文件名 | 有 product flavor 的工程必然失败于无上下文的 `系统找不到指定的文件。 (os error 2)`，且此前已复制的 ABI 会留在 `dist/` | 新增 `flavor` 配置并透传 `--flavor`；产物定位三级回退 + 候选/目录清单报错 |
| `target_abi = []` 时静默只打包 arm64 | 目录里有 3 个 ABI 的 APK、`pacor config` 显示 `target_abi = []`、`dist/` 只有 `arm64-v8a`，退出码 0 | 空列表报配置错误 |
| `target_abi` 用 `HashSet`，顺序不确定 | 同一份配置连续 4 次 `pacor config --full`，`["arm64-v8a","armeabi-v7a"]` 与反向交替出现；`--target-platform` 参数顺序、产物顺序、失败时先报哪个 ABI 都随机 | 改用 `BTreeSet`，顺序固定 |
| Android 路径多余的 `Arch::current()` | `AndroidBuilder::new` 调用它，但 Android 的构建目录与宿主架构无关；在不属于 `{x86_64, aarch64}` 的宿主上会以 `Unsupported architecture` 直接失败 | 不再查询宿主架构（新增 `FlutterProject::android_build_path`） |
| 该后端 0 测试，CI 无 Android 作业 | `cargo test -p pacor-packaging-android -- --list` → `0 tests, 0 benchmarks`；`.github` 中 grep 不到 `android`/`apk`/`adb` | 新增 51 个单元测试与 12 个端到端测试，随 `cargo test --workspace` 在三个平台的 CI 中运行；不带真实 Flutter/SDK 的工具链作业仍未添加 |
| 文档与实现不一致 | README 状态表写“已实现”，项目结构注释写“签名”，而签名是 `TODO`；README 把 `pacor android` 列为公开命令，它却是 `hide = true`；`--sign-apk` 只打一条 WARN | README / README.zh-CN 增补 Android 小节并链接本文档；移除 `--sign-apk`，并把 `pacor android` 从其帮助中解隐藏（README 一直把它当作公开命令）；限制集中记录在本文档 |
