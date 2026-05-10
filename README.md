<p align='center'>
<img src='docs/pacor.svg' width='64' />
<br/>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
<a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust"></a>
<br/>
<span>English</span> | <a href="README.zh-CN.md">中文</a>
</p>

# Pacor

A powerful cross-platform packaging tool for Flutter applications.

Supports generating:

- macOS DMG installers
- Windows NSIS/MSI installers
- ~~Linux AppImage/DEB packages~~
- Android APK installers

## Important Notice

This project is still in the early stages of development. Many features are not yet complete, and it is not recommended for production environments. We welcome valuable feedback and suggestions from all walks of life to jointly promote the development of this project.

## Features

- [x] **Cross-platform**: Build installers for all major platforms with a single tool
- [x] **Easy to use**: Simple command-line interface with sensible defaults
- [x] **Fast**: Written in Rust for excellent performance
- [x] **Lightweight**: Small binary size with few dependencies
- [ ] **Configurable**: Flexible configuration system to customize the build process
- [ ] **Internationalization**: Customize internationalization of installers for all platforms with a single configuration file

## Installation

### Install from source

```bash
git clone https://github.com/pacor-rs/pacor.git
cd pacor
cargo build --release
```

The compiled binary is located at `target/release/pacor`.

## Usage

### Initialize configuration

```bash
pacor init
```

This command creates a `pacor.toml` configuration file in the project root directory.

### Build for the current platform

```bash
pacor build
```

### Build Android package

```bash
pacor android
```

### Other commands

```bash
# Clean build artifacts
pacor clean

# Check build environment
pacor check

# Display current configuration
pacor config
```

## Configuration

Pacor uses a TOML configuration file (`pacor.toml`) in the project root directory. Here is a sample configuration:

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

## Supported Platforms

- **macOS**: Create DMG installers with customizable backgrounds
- **Windows**: Create NSIS or MSI installers
- **Linux**: Create AppImage or DEB packages
- **Android**: Create signed APK files

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
