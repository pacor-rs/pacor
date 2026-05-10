<p align='center'>
<img src='docs/pacor.svg' width='64' />
<br/>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
<a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust"></a>
<br/>
<a href="README.md">English</a> | <span>中文</span>
</p>

# Pacor

用于 Flutter 应用程序的强大跨平台打包工具。

支持生成：

- macOS DMG 安装包
- Windows NSIS/MSI 安装包
- ~~Linux AppImage/DEB 安装包~~
- Android APK 安装包

## 使用须知

本项目尚处于前期开发阶段，诸多功能尚不完善，一些配置项尚未启用，同时一些功能和配置项在未来可能会删除或者改变，因此不建议用于生产环境。欢迎各界人士提出宝贵意见和建议，共同推动此项目的发展。

## 功能特性

- [x] **跨平台**：使用单一工具为所有主要平台构建安装包
- [x] **易于使用**：具有合理默认值的简单命令行界面
- [x] **快速**：采用 Rust 编写，性能卓越
- [x] **轻量级**：二进制文件小，依赖项少
- [ ] **可配置**：灵活的配置系统，可自定义构建过程
- [ ] **国际化**：一个配置文件完成各平台安装包的国际化定制

## 安装

### 从源码安装

```bash
git clone https://github.com/pacor-rs/pacor.git
cd pacor
cargo build --release
```

编译后的二进制文件位于 `target/release/pacor`。

## 使用方法

### 初始化配置

```bash
pacor init
```

此命令会在项目根目录创建一个 `pacor.toml` 配置文件。

### 为当前平台构建

```bash
pacor build
```

### 构建 Android 包

```bash
pacor android
```

### 其他命令

```bash
# 清理构建产物
pacor clean

# 检查构建环境
pacor check

# 显示当前配置
pacor config
```

## 配置

Pacor 使用项目根目录中的 TOML 配置文件 (`pacor.toml`)。以下是一个示例配置：

```toml
[app]
name = "My Flutter App"
publisher = "My Company"
url = "https://example.com"

[build]
flutter_build_mode = "release"
auto_flutter_build = true
cleanup_temp_files = true
output_dir = "dist"

[platforms.macos]
enabled = true
minimum_system_version = "10.14"

[platforms.windows]
enabled = true
installer_type = "nsis"

[platforms.android]
enabled = true
target_abi = ["arm64-v8a"]
```

## 支持的平台

- **macOS**：创建可自定义背景的 DMG 安装包
- **Windows**：创建 NSIS 或 MSI 安装包
- **Linux**：创建 AppImage 或 DEB 包
- **Android**：创建签名的 APK 文件

## 许可证

本项目采用 MIT 许可证 - 详情请见 [LICENSE](LICENSE) 文件。
