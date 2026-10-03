<p align='center'>
<img src='docs/pacor.svg' width='64' />
<br/>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
<a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust"></a>
<br/>
<a href="README.md">English</a> | <span>中文</span>
</p>

# Pacor

用于 Flutter 应用程序的跨平台打包工具。

Pacor 读取**一份 `pacor.toml`**（描述所有支持的平台），并把你选择的平台的
Flutter 构建产物打包成安装包：

| 目标平台  | 产物                                                         | 状态        |
| --------- | ------------------------------------------------------------ | ----------- |
| macOS     | `.dmg`（可选签名与公证）+ 自动更新 `.tar.gz`                  | 已实现      |
| Windows   | NSIS 安装包 + 便携版 `.zip`                                   | 已实现      |
| Linux     | AppImage 和/或 `.deb`                                         | 已实现      |
| Android   | 按 ABI 拆分的 `.apk`                                          | 已实现      |
| —         | MSI、RPM                                                      | 尚未实现    |

> 项目处于前期阶段，功能和配置仍可能变化。

## 安装

```bash
git clone https://github.com/pacor-rs/pacor.git
cd pacor
cargo build --release
```

编译后的二进制文件位于 `target/release/pacor`。

## 快速开始

```bash
# 1. 在 Flutter 项目根目录生成带注释的 pacor.toml
pacor init

# 2. 编辑 pacor.toml（应用信息、输出目录、各平台选项）

# 3. 为指定平台构建
pacor build --target linux      # 格式取自 pacor.toml 中的 formats（appimage、deb 等）
pacor build --target windows
pacor build --target macos
pacor build --target android

# 省略 --target 时构建宿主平台
pacor build
```

## 命令

| 命令                                                        | 说明                                   |
| ----------------------------------------------------------- | -------------------------------------- |
| `pacor init [--force]`                                      | 写入带注释的配置模板                   |
| `pacor build [--target OS] [--format FMT]...`               | 构建安装包                             |
| `pacor android`                                             | `pacor build --target android` 的别名  |
| `pacor clean [--target OS]`                                 | 删除输出目录和 Flutter 构建产物        |
| `pacor cache clean [--dry-run]`                             | 清除缓存中已下载的工具与模板           |
| `pacor check [--target OS]`                                 | 校验项目、配置与构建工具               |
| `pacor config [--full]`                                     | 打印生效的配置                         |

`pacor clean` 作用于项目；`pacor cache clean` 清空用户配置目录下的共享缓存
（默认图标集、构建模板、`linuxdeploy`、NSIS）。被清除的内容会在下次需要时
重新下载，`--dry-run` 可先列出将要删除的内容。

全局参数：`--config <path>`（默认项目根目录下的 `pacor.toml`）、
`--command-output <none|normal|verbose>`（外部命令输出）、
`-l/--log-level <error|warn|info|debug>`。

所有平台配置段在任意宿主上都会解析，因此同一份 `pacor.toml` 可在 macOS、
Linux、Windows 通用；只有 `--target` 选中的段会被使用。各平台后端始终参与编译，
因此不支持的目标会在运行时给出清晰的「缺少工具 X」错误，而不是被静默忽略。

## 配置

`pacor.toml` 位于 Flutter 项目根目录。执行 `pacor init` 可生成完整注释模板，
或查看参考模板
[`crates/pacor-core/src/config/template.toml`](crates/pacor-core/src/config/template.toml)。
精简示例：

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

密钥优先从环境变量读取，且不会被写回配置文件：
`PACOR_WINDOWS_CERT_PASSWORD`、`PACOR_ANDROID_KEYSTORE_PASSWORD`、
`PACOR_ANDROID_KEY_PASSWORD`。GitHub 镜像相关变量见
[docs/ENVIRONMENT_VARIABLES.md](docs/ENVIRONMENT_VARIABLES.md)。
`PACOR_OUTPUT_DIR` 与 `PACOR_BUILD_MODE` 会覆盖 `[build]` 中的对应项。

## 项目结构

Pacor 采用 Cargo workspace，每个 crate 职责单一：

```
crates/
├── pacor-core/                 # 配置 schema、领域模型、共享工具
├── pacor-packaging/            # 目标注册表（`packager_for`）
├── pacor-packaging-linux/      # AppImage、DEB、图标、desktop 入口
├── pacor-packaging-macos/      # DMG、plist、本地化、签名、公证
├── pacor-packaging-windows/    # NSIS、便携 ZIP、签名
├── pacor-packaging-android/    # APK 组装、ABI、签名
└── pacor-cli/                  # clap 参数、分发、输出、二进制 `pacor`
```

依赖方向单向：`pacor-core` ← `pacor-packaging-*` ← `pacor-packaging` ← `pacor-cli`。

## 开发

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

由于所有后端在任意宿主上都会编译，在 Linux 上执行上述检查即可同时类型检查
macOS 与 Windows 后端。项目不含 C 依赖（压缩使用纯 Rust 的 `zlib-rs` 后端），
因此无需任何交叉编译工具链即可检查其它目标平台：

```bash
rustup target add x86_64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin
cargo check --workspace --target x86_64-pc-windows-msvc
cargo check --workspace --target x86_64-apple-darwin
cargo check --workspace --target aarch64-apple-darwin
```

## 许可证

本项目采用 MIT 许可证 - 详情请见 [LICENSE](LICENSE) 文件。
