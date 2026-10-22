<p align='center'>
<span>English</span> | <a href="ENVIRONMENT_VARIABLES.zh-CN.md">中文</a>
</p>

# Pacor environment variables

This document describes the environment variables the Pacor tool supports and how to use them.

## Build environment variables

### PACOR_OUTPUT_DIR

**Purpose**: override `[build] output_dir`, the directory that receives the artifacts.

**Format**: a path relative to the project root. It has to stay inside the project — an
absolute path, a `..` component and `.` are rejected, because `pacor clean` removes the
directory wholesale.

### PACOR_BUILD_MODE

**Purpose**: override `[build] flutter_build_mode`.

**Format**: `release`, `debug` or `profile`.

## Downloader environment variables

The downloader module reads GitHub mirrors from environment variables to improve download speed
and reliability, which matters most in networks with restricted access.

### PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE

**Purpose**: override the GitHub releases download URL with a template

**Format**: a string template with these placeholders:

- `<owner>`: the GitHub repository owner
- `<repo>`: the repository name
- `<version>`: the release version
- `<asset>`: the asset file name

**Example**:

```bash
# using the jsDelivr CDN mirror
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"

# using the FastGit mirror
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://download.fastgit.org/<owner>/<repo>/releases/download/<version>/<asset>"

# using a custom mirror service
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://your-mirror.com/<owner>/<repo>/releases/<version>/<asset>"
```

### PACOR_GITHUB_RAW_MIRROR_TEMPLATE

**Purpose**: override the GitHub raw file URL with a template

**Format**: a string template with these placeholders:

- `<owner>`: the GitHub repository owner
- `<repo>`: the repository name
- `<branch>`: the branch name
- `<path>`: the file path

**Example**:

```bash
# using the jsDelivr CDN mirror
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# using the FastGit mirror
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://raw.fastgit.org/<owner>/<repo>/<branch>/<path>"

# using a custom mirror service
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://your-mirror.com/<owner>/<repo>/raw/<branch>/<path>"
```

**How it works**:
with this variable set, an original GitHub releases URL:

```
https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

becomes:

```
https://cdn.jsdelivr.net/gh/flutter/flutter@3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

For a GitHub raw file URL:

```
https://raw.githubusercontent.com/flutter/flutter/main/README.md
```

becomes:

```
https://cdn.jsdelivr.net/gh/flutter/flutter@main/README.md
```

### PACOR_GITHUB_MIRROR_BASE_URL

**Purpose**: set the base URL of a GitHub mirror

**Format**: a valid base URL

**Example**:

```bash
# using a GitHub proxy mirror (one variable covers raw files and release assets)
export PACOR_GITHUB_MIRROR_BASE_URL="https://gh.my-website.ccwu.cc"

# using another mirror service
export PACOR_GITHUB_MIRROR_BASE_URL="https://your-mirror.example.com"
```

**How it works**:
with this variable set, the original GitHub URL is stripped of its scheme and appended to the
mirror base URL as a path.

Original URL:

```
https://github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

After the rewrite:

```
https://gh.my-website.ccwu.cc/github.com/flutter/flutter/releases/download/3.0.0/flutter_linux_3.0.0-stable.tar.xz
```

The same applies to raw URLs: `https://raw.githubusercontent.com/pacor-rs/tools/refs/tags/1.1/windows/wix/main.wxs`
becomes `https://gh.my-website.ccwu.cc/raw.githubusercontent.com/pacor-rs/tools/refs/tags/1.1/windows/wix/main.wxs`
(a mapping pinned by the unit tests of `utils::downloader`). Proxies such as ghproxy accept both
`https://<mirror>/<host>/<path>` and `https://<mirror>/https://<host>/<path>`, so the base URL form works
as it is.

**Note**: with a mirror configured, pacor **deliberately does not use** `HTTP_PROXY`/`HTTPS_PROXY`
(to keep a proxy and a mirror from stacking). If the mirror itself is only reachable through a
proxy, point `PACOR_GITHUB_RAW_MIRROR_TEMPLATE` / `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` at an
address that is reachable instead.

## Environment variable precedence

When several of them are set at once, the order of precedence is:

1. `PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE` / `PACOR_GITHUB_RAW_MIRROR_TEMPLATE` (highest priority, picked by URL type)
2. `PACOR_GITHUB_MIRROR_BASE_URL`
3. the original GitHub URL (default)

## HTTP_PROXY / HTTPS_PROXY (standard proxy variables)

**Purpose**: reach GitHub through a proxy on this machine or the local network. The download
client of pacor reads these standard variables (`ALL_PROXY` works too), with no extra
configuration key.

**Important: a browser reaching GitHub does not mean pacor can.** Browsers use the
**proxy auto-config (PAC) script** from the Windows "Internet Options", while pacor (like `curl`)
**reads environment variables only** — it looks at neither the PAC file nor the system proxy in
the registry. If this machine gets online through a PAC file, the proxy address has to be exported
explicitly in the terminal that runs pacor:

```powershell
# example: the local proxy listens on 1086 (hysteria's mixed http/socks5 port)
$env:HTTPS_PROXY = "http://127.0.0.1:1086"
$env:HTTP_PROXY  = "http://127.0.0.1:1086"
```

The PAC address is visible in the Windows Internet settings, and the PAC file names the actual
proxy; measured on this machine, that proxy accepts both HTTP CONNECT and SOCKS5, so the HTTP form
is enough for pacor.

**Relation to mirrors**: with a mirror configured pacor does **not** use a proxy; the two do not
stack. If the proxy itself can reach GitHub, use the proxy alone and set no mirror; measured on
this machine:

| Method | Result |
| --- | --- |
| Proxy (`HTTPS_PROXY=http://127.0.0.1:1086`) | with a cold cache, templates in 2 s and the 39 MB WiX toolchain in 26 s; both MSIs (NSIS excluded) finished in 48 s |
| Mirror `ghproxy.net` | templates fine, the 39 MB toolchain transfer drops (`Peer disconnected`), the build fails |

## Use cases

### 1. Users in mainland China

For users in mainland China these settings are recommended:

```bash
# option 1: a proxy mirror (one variable covers raw files and release assets; recommended)
export PACOR_GITHUB_MIRROR_BASE_URL="https://gh.my-website.ccwu.cc"

# option 2: two templates, one per kind
#   raw files (a template tag path is refs/tags/<branch>/)
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://gh.my-website.ccwu.cc/https://raw.githubusercontent.com/<owner>/<repo>/refs/tags/<branch>/<path>"
#   release assets
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://gh.my-website.ccwu.cc/https://github.com/<owner>/<repo>/releases/download/<version>/<asset>"

# option 3: jsDelivr only proxies files inside the repository, never release assets
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"
```

### Two things to watch when choosing a mirror (measured)

Measured on Windows after clearing the `%APPDATA%\pacor\windows` cache (2026-10):

- **The template files are small enough for any mirror**: the 5 files under `windows/nsis/**` and
  the 2 under `windows/wix/**` arrive through a mirror with exactly the sizes of the blobs in the
  tag; a cold NSIS run (download the distribution, the templates, compile, package) finished
  within 60 seconds.
- **A large file (the WiX toolchain, about 39 MB) is where a mirror shows its limits**: measured,
  `ghproxy.net` dropped that zip at about 75 KB/s (`Peer disconnected`). pacor **keeps the bytes it
  already received and resumes** (up to 4 attempts, asking for `Range: bytes=<bytes already received>-`), and
  prints progress every 8 MB; measured with the transfer cut at 3 MB, the second attempt continued
  from 3 MB and finished the whole MSI build in 38 s. A mirror that does not even support `Range`
  degrades to a restart, which at least does not fail on a single dropped connection.
  `https://gh.my-website.ccwu.cc` is only reachable through the proxy on this machine (a direct
  request resets the TLS handshake); through the proxy everything works, but in that case **using
  the proxy alone and no mirror** is both simpler and faster (see the previous section).

So mirror bandwidth and stability still matter (every drop costs the segment again), but a single
drop no longer fails the build; if a mirror is unusable or too slow, seed the WiX toolchain
offline the way [WINDOWS.md](WINDOWS.md) describes.

### 2. Corporate environments

Corporate users may need an internal mirror service:

```bash
# an internal mirror
export PACOR_GITHUB_MIRROR_BASE_URL="https://internal-mirror.company.com"

# or a template (releases)
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://internal-cdn.company.com/<owner>/<repo>/<version>/<asset>"

# or a template (raw files)
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://internal-cdn.company.com/<owner>/<repo>/<branch>/<path>"
```

### 3. CI/CD environments

In a continuous integration environment:

```yaml
# GitHub Actions example
env:
  PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE: "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
  PACOR_GITHUB_RAW_MIRROR_TEMPLATE: "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# GitLab CI example
variables:
  PACOR_GITHUB_MIRROR_BASE_URL: "https://ghproxy.com"
```

## How to set them

### Windows

#### PowerShell

```powershell
# for the current session only
$env:PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE = "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
$env:PACOR_GITHUB_RAW_MIRROR_TEMPLATE = "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# permanently, system-wide
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>", "Machine")
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RAW_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>", "Machine")

# permanently, per user
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>", "User")
[Environment]::SetEnvironmentVariable("PACOR_GITHUB_RAW_MIRROR_TEMPLATE", "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>", "User")
```

#### Command Prompt

```cmd
# for the current session only
set PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE=https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>
set PACOR_GITHUB_RAW_MIRROR_TEMPLATE=https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>

# permanently
setx PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
setx PACOR_GITHUB_RAW_MIRROR_TEMPLATE "https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"
```

### Linux/macOS

#### Bash/Zsh

```bash
# for the current session only
export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"
export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"

# permanently (append to ~/.bashrc or ~/.zshrc)
echo 'export PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<version>/<asset>"' >> ~/.bashrc
echo 'export PACOR_GITHUB_RAW_MIRROR_TEMPLATE="https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>"' >> ~/.bashrc
source ~/.bashrc
```

## Verifying the configuration

After setting the variables, check them with:

```bash
# check that the variable is set
echo $PACOR_GITHUB_RELEASE_MIRROR_TEMPLATE
echo $PACOR_GITHUB_RAW_MIRROR_TEMPLATE
echo $PACOR_GITHUB_MIRROR_BASE_URL

# run a pacor command and read the log
pacor build --verbose
```

In verbose mode Pacor prints the download URL it actually uses, which is how you tell whether the
mirror took effect.

## Caveats

1. **URL format**: make sure the mirror URL is well formed and starts with `http://` or `https://`
2. **Network test**: try the mirror service before you rely on it
3. **Security**: use a mirror you trust, and avoid insecure HTTP links
4. **Performance**: mirrors differ, so pick the one that suits your network

## Troubleshooting

### Q: How do I unset the variables?

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

### Q: What if the mirror URL rewrite fails?

**A**: if the mirror URL is malformed or the service is unavailable, Pacor falls back to the original GitHub URL. Check the log output to confirm which download URL was actually used.

### Q: How do I test whether a mirror works?

**A**: try the mirror URL with curl or a browser:

```bash
curl -I "https://cdn.jsdelivr.net/gh/flutter/flutter@3.0.0/README.md"
```

### Q: Does it support proxy servers?

**A**: the downloader reads the proxy from **environment variables** only (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`), the same as `curl`. It does not read the Windows "Internet Options" proxy or a PAC script — see the PAC section above. Mirrors are configured through environment variables as well.
