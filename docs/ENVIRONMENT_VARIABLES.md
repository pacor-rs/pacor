# Pacor 环境变量配置文档

本文档详细说明了 Pacor 工具支持的环境变量及其用法。

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
# 使用 GitHub 代理镜像
export PACOR_GITHUB_MIRROR_BASE_URL="https://ghproxy.com"

# 使用其他镜像服务
export PACOR_GITHUB_MIRROR_BASE_URL="https://mirror.github.com"
```

**工作原理**:
当设置此变量时，原始的 GitHub 下载链接会作为路径拼接到镜像基础 URL 后面。

原始链接：

```
https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

转换后：

```
https://ghproxy.com/https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

## 环境变量优先级

当同时设置多个环境变量时，优先级顺序为：

1. `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` / `PACOR_GITHUB_RAW_MIRROR_TEMPLATE` (最高优先级，根据 URL 类型选择)
2. `PACOR_GITHUB_MIRROR_BASE_URL`
3. 原始 GitHub URL (默认)

## 使用场景

### 1. 中国大陆用户

对于中国大陆的用户，推荐使用以下配置：

```bash
# 方案 1: 使用 jsDelivr (releases 文件)
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"

# 方案 1: 使用 jsDelivr (raw 文件)
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# 方案 2: 使用 ghproxy
export PACOR_GITHUB_MIRROR_BASE_URL="https://ghproxy.com"
```

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

**A**: Pacor 的下载器会自动检测系统代理设置，同时也支持通过环境变量配置镜像服务。
