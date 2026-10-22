<p align='center'>
<a href="ENVIRONMENT_VARIABLES.md">English</a> | <span>中文</span>
</p>

# Pacor 环境变量配置文档

本文档详细说明了 Pacor 工具支持的环境变量及其用法。

## 构建环境变量

### PACOR_OUTPUT_DIR

**用途**: 覆盖 `[build] output_dir`，即产物写入的目录。

**格式**: 相对项目根目录的路径，且必须位于项目内——绝对路径、包含 `..` 的路径与 `.` 都会被拒绝，
因为 `pacor clean` 会整体删除该目录。

### PACOR_BUILD_MODE

**用途**: 覆盖 `[build] flutter_build_mode`。

**格式**: `release`、`debug` 或 `profile`。

## 下载器环境变量

Pacor 的下载器模块支持通过环境变量配置 GitHub 镜像，以提高下载速度和可靠性，特别适用于网络受限的环境。

### PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE

**用途**: 通过模板自定义 GitHub releases 下载链接

**格式**: 字符串模板，支持以下占位符：

- `<owner>`: GitHub 仓库所有者
- `<repo>`: 仓库名称
- `<version>`: 发布版本号
- `<asset>`: 资源文件名

**示例**:

```bash
# 使用 jsDelivr CDN 镜像
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"

# 使用 FastGit 镜像
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://download.fastgit.org/<owner>/<repo>/releases/download/<version>/<asset>"

# 使用自定义镜像服务
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://your-mirror.com/<owner>/<repo>/releases/<version>/<asset>"
```

### PACOR_GITHUB_RAW_MIRROR_TEMPLATE

**用途**: 通过模板自定义 GitHub raw 文件下载链接

**格式**: 字符串模板，支持以下占位符：

- `<owner>`: GitHub 仓库所有者
- `<repo>`: 仓库名称
- `<branch>`: 分支名称
- `<path>`: 文件路径

**示例**:

```bash
# 使用 jsDelivr CDN 镜像
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# 使用 FastGit 镜像
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://raw.fastgit.org/<owner>/<repo>/<branch>/<path>"

# 使用自定义镜像服务
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://your-mirror.com/<owner>/<repo>/raw/<branch>/<path>"
```

**工作原理**:
当设置此变量时，原始的 GitHub releases 下载链接：

```
https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

会被转换为：

```
https://cdn.jsdelivr.net/gh/flutter/flutter@3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

对于 GitHub raw 文件链接：

```
https://raw.githubusercontent.com/flutter/flutter/main/README.md
```

会被转换为：

```
https://cdn.jsdelivr.net/gh/flutter/flutter@main/README.md
```

### PACOR_GITHUB_MIRROR_BASE_URL

**用途**: 设置 GitHub 镜像的基础 URL

**格式**: 有效的 URL 基础路径

**示例**:

```bash
# 使用 GitHub 代理镜像（一个变量同时覆盖 raw 文件与 release 资源）
export PACOR_GITHUB_MIRROR_BASE_URL="https://gh.my-website.ccwu.cc"

# 使用其他镜像服务
export PACOR_GITHUB_MIRROR_BASE_URL="https://your-mirror.example.com"
```

**工作原理**:
当设置此变量时，原始 GitHub 链接去掉协议后作为路径拼接到镜像基础 URL 后面。

原始链接：

```
https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

转换后：

```
https://gh.my-website.ccwu.cc/github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

raw 链接同理：`https://raw.githubusercontent.com/pacor-rs/tools/refs/tags/1.1/windows/wix/main.wxs`
会变成 `https://gh.my-website.ccwu.cc/raw.githubusercontent.com/pacor-rs/tools/refs/tags/1.1/windows/wix/main.wxs`
（这条映射由 `utils::downloader` 的单元测试固定下来）。ghproxy 这类代理对
`https://<镜像>/<主机>/<路径>` 与 `https://<镜像>/https://<主机>/<路径>` 两种写法都接受，
因此 base URL 模式可以直接使用。

**注意**: 配置了镜像时，pacor 会**显式不使用** `HTTP_PROXY`/`HTTPS_PROXY`（这是为了避免代理与
镜像叠加）。如果镜像本身只有通过代理才能访问，请改用 `PACOR_GITHUB_RAW_MIRROR_TEMPLATE` /
`PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` 指向可达的地址。

## 环境变量优先级

当同时设置多个环境变量时，优先级顺序为：

1. `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` / `PACOR_GITHUB_RAW_MIRROR_TEMPLATE` (最高优先级，根据 URL 类型选择)
2. `PACOR_GITHUB_MIRROR_BASE_URL`
3. 原始 GitHub URL (默认)

## HTTP_PROXY / HTTPS_PROXY（标准代理变量）

**用途**: 通过本机或局域网代理访问 GitHub。pacor 的下载客户端读取这些标准变量
（`ALL_PROXY` 亦可），不需要额外配置项。

**重要: 浏览器能访问 GitHub 并不代表 pacor 能访问。** 浏览器会使用 Windows 的
「Internet 选项」里的**自动配置脚本（PAC）**，而 pacor（与 `curl` 一样）**只读环境变量**，
既不看 PAC，也不看注册表里的系统代理设置。如果这台机器靠 PAC 上网，必须在运行 pacor 的终端里
显式导出代理地址：

```powershell
# 例：本机代理监听 1086（hysteria 的 http/socks5 混合端口）
$env:HTTPS_PROXY = "http://127.0.0.1:1086"
$env:HTTP_PROXY  = "http://127.0.0.1:1086"
```

从 Windows 的 Internet 设置里可以看到 PAC 地址，PAC 里会写实际代理；本机实测该代理同时接受
HTTP CONNECT 与 SOCKS5，pacor 用 HTTP 形式即可。

**与镜像的关系**: 配置了镜像时 pacor **不会**使用代理，两者不能叠加。如果代理本身能直连
GitHub，建议只用代理、不设镜像；实测对比：

| 方式 | 结果 |
| --- | --- |
| 代理（`HTTPS_PROXY=http://127.0.0.1:1086`） | 冷缓存下模板 2 s、WiX 工具链（39 MB）26 s，两个 MSI 与排除 NSIS 全流程 48 s 完成 |
| 镜像 `ghproxy.net` | 模板正常，39 MB 工具链传输中断（`Peer disconnected`），构建失败 |

## 使用场景

### 1. 中国大陆用户

对于中国大陆的用户，推荐使用以下配置：

```bash
# 方案 1: 使用代理镜像（一个变量同时覆盖 raw 文件与 release 资源，推荐）
export PACOR_GITHUB_MIRROR_BASE_URL="https://gh.my-website.ccwu.cc"

# 方案 2: 用模板分别指定
#   raw 文件（模板 tag 的路径是 refs/tags/<branch>/）
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://gh.my-website.ccwu.cc/https://raw.githubusercontent.com/<owner>/<repo>/refs/tags/<branch>/<path>"
#   release 资源
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://gh.my-website.ccwu.cc/https://github.com/<owner>/<repo>/releases/download/<version>/<asset>"

# 方案 3: jsDelivr 只能代理仓库内的文件，不能代理 release 资源
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"
```

### 镜像选型要注意的两件事（实测）

在 Windows 上清空 `%APPDATA%\pacor\windows` 缓存后实测（2026-10）：

- **模板文件很小，镜像都能胜任**：`windows/nsis/**` 的 5 个文件与 `windows/wix/**` 的 2 个文件
  经镜像下载后大小与 tag 中的 blob 完全一致；NSIS 冷启动（下载发行版 + 模板 + 编译 + 打包）
  60 秒内完成。
- **大文件（WiX 工具链约 39 MB）是对镜像的考验**：实测 `ghproxy.net` 在约 75 KB/s 的速度下
  传输该 zip 时会被断开（`Peer disconnected`）。pacor 会**保留已收到的字节并断点续传**（最多 4 次
  尝试，续传时发 `Range: bytes=<已收字节>-`），并每 8 MB 打印一次进度；实测被砍在 3 MB 后，
  第二次尝试从 3 MB 继续，38 秒内完成整个 MSI 构建。镜像若连 `Range` 都不支持，重试会退化为
  重新下载，至少不会因为一次断流而失败。
  `https://gh.my-website.ccwu.cc` 在本机需要经代理才能访问（直接访问时 TLS 握手即被重置），
  经代理后一切正常，但此时**直接用代理而不设镜像**更简单也更快（见上一节）。

因此：镜像的带宽和稳定性仍然重要（每次断流都要重来一段），但单次断流已经不再让构建失败；
如果镜像完全不可用或速度无法接受，按 [WINDOWS.md](WINDOWS.zh-CN.md) 的方式离线预置 WiX 工具链。

### 2. 企业环境

企业用户可能需要通过内部镜像服务：

```bash
# 使用企业内部镜像
export PACOR_GITHUB_MIRROR_BASE_URL="https://internal-mirror.company.com"

# 或使用模板配置 (releases)
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://internal-cdn.company.com/<owner>/<repo>/<version>/<asset>"

# 或使用模板配置 (raw 文件)
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://internal-cdn.company.com/<owner>/<repo>/<branch>/<path>"
```

### 3. CI/CD 环境

在持续集成环境中配置：

```yaml
# GitHub Actions 示例
env:
  PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE: "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
  PACOR_GITHUB_RAW_MIRROR_TEMPLATE: "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# GitLab CI 示例
variables:
  PACOR_GITHUB_MIRROR_BASE_URL: "https://ghproxy.com"
```

## 配置方法

### Windows

#### PowerShell

```powershell
# 临时设置（当前会话）
$env:PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE = "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
$env:PACOR_GITHUB_RAW_MIRROR_TEMPLATE = "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# 永久设置（系统级）
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>", "Machine")
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RAW_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>", "Machine")

# 永久设置（用户级）
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>", "User")
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RAW_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>", "User")
```

#### 命令提示符

```cmd
# 临时设置
set PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE=https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>
set PACOR_GITHUB_RAW_MIRROR_TEMPLATE=https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>

# 永久设置
setx PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
setx PACOR_GITHUB_RAW_MIRROR_TEMPLATE "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"
```

### Linux/macOS

#### Bash/Zsh

```bash
# 临时设置（当前会话）
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# 永久设置（添加到 ~/.bashrc 或 ~/.zshrc）
echo 'export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"' >> ~/.bashrc
echo 'export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"' >> ~/.bashrc
source ~/.bashrc
```

## 验证配置

设置环境变量后，可以通过以下方式验证：

```bash
# 检查环境变量是否设置
echo $PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE
echo $PACOR_GITHUB_RAW_MIRROR_TEMPLATE
echo $PACOR_GITHUB_MIRROR_BASE_URL

# 运行 pacor 命令查看日志
pacor build --verbose
```

在详细模式下，Pacor 会显示实际使用的下载链接，您可以确认镜像配置是否生效。

## 注意事项

1. **URL 格式**: 确保提供的镜像 URL 格式正确，以 `http://` 或 `https://` 开头
2. **网络测试**: 在设置镜像之前，建议先测试镜像服务的可用性
3. **安全性**: 使用受信任的镜像服务，避免使用不安全的 HTTP 链接
4. **性能**: 不同镜像服务的性能可能有差异，建议选择最适合您网络环境的服务

## 常见问题

### Q: 如何清除环境变量设置？

**A**:

```bash
# Linux/macOS
unset PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE
unset PACOR_GITHUB_RAW_MIRROR_TEMPLATE
unset PACOR_GITHUB_MIRROR_BASE_URL

# Windows PowerShell
Remove-Item Env:PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE
Remove-Item Env:PACOR_GITHUB_RAW_MIRROR_TEMPLATE
Remove-Item Env:PACOR_GITHUB_MIRROR_BASE_URL
```

### Q: 镜像链接转换失败怎么办？

**A**: 如果镜像 URL 格式不正确或服务不可用，Pacor 会自动回退到原始 GitHub URL。检查日志输出确认实际使用的下载链接。

### Q: 如何测试镜像服务是否可用？

**A**: 可以使用 curl 或浏览器测试镜像链接：

```bash
curl -I "https://cdn.jsdelivr.net/gh/flutter/flutter@3.0.0/README.md"
```

### Q: 是否支持代理服务器？

**A**: 下载器**只读环境变量**（`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY`），与 `curl` 相同；它不会读取 Windows「Internet 选项」里的代理或 PAC 脚本——见上面的 PAC 一节。镜像同样通过环境变量配置。
