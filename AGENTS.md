# AGENTS.md

本文件写给在本仓库里改代码的人，以及代替人改代码的自动化代理。它只记录**这个仓库
实际在执行的**规则：每一条都能在 CI、脚本或既有提交里找到对应物，而不是一份通用建议。

## 这是什么

`pacor` 是一个 Flutter 应用的跨平台打包工具（CLI），输入一个 Flutter 工程和一份
`pacor.toml`，输出 Linux（AppImage/deb/rpm）、macOS（app/dmg）、Windows（NSIS/MSI）
与 Android（APK/AAB）的可发布产物。

工作区是 7 个 crate（edition 2024，`rust-version = 1.99`，`resolver = 3`）：

| crate | 职责 |
| --- | --- |
| `pacor-cli` | 参数解析、命令分发、用户可见输出 |
| `pacor-core` | 配置 schema、领域模型、错误类型、工具链下载、共享工具 |
| `pacor-packaging` | 注册表：把 `--target` 映射到一个 `BundleBuilder`，并处理 `--format` |
| `pacor-packaging-{linux,macos,windows,android}` | 各平台的构建与打包 |

依赖方向是单向的：后端 → `core`，`pacor-cli` → `pacor-packaging` → 各后端。`core` 刻意
不含任何具体打包格式，因此**所有平台的配置在任意宿主上都能解析**，这也是 CI 能在 Linux 上
交叉检查 Windows/macOS 后端的前提。

## 环境与必须全绿的检查

改动前至少 `cargo build --workspace` 能过；提交前的清单与 CI 完全一致：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings

RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-apple-darwin
```

几条不是形式主义的规则：

- **release 那一遍不是重复劳动**。`#[cfg(debug_assertions)]` 会把整段代码藏起来
  （`pacor-cli` 的 `trace` 日志级别就是这样），只在 release 下才会出现的 `dead_code` /
  `unused` 警告，只有这一遍能抓到。
- **交叉检查不需要交叉工具链**，因为仓库没有 C 依赖（压缩用的是纯 Rust 的 `zlib-rs`）。
  引入任何需要 C 工具链或系统库的依赖都会让这条失效，属于需要单独讨论的改动。
- 没有 `rustfmt.toml`，用默认配置（100 列）。不要为了"更好看"手工调换行。
- 本地不要习惯性跑完整的 `cargo build --release`：release profile 是
  `opt-level = "z"` + LTO + `codegen-units = 1`，慢是设计的一部分，CI 用缓存补偿。

## 代码规范

- `unsafe`：生产代码目前 0 处，不要引入。测试里唯一允许的用法是 edition 2024 下必须
  `unsafe` 的 `env::set_var`/`remove_var`（`tests/android_smoke.rs`），且必须串行化——
  见下文"测试"。
- 错误：crate 内用 `thiserror`，按领域拆到 `src/error/` 的子模块并在 `src/error.rs` 里
  `pub use`；跨 crate 的边界与 CLI 用 `anyhow`。错误信息要自己解释清楚**哪个路径、哪个
  工具、期望什么**，不要把 `系统找不到指定的文件。 (os error 2)` 直接甩给用户。
- 非测试代码不 `unwrap()`/`expect()`（全仓现在只剩 2 处，都是可证明不会失败的场景）。
  失败要能定位：优先带上下文 `Err`，而不是 `unwrap`。
- 每个文件以 `//!` 模块文档开头，说明"这个模块负责什么、为什么这样切"；公开项写 `///`。
- 魔法字符串变常量（如 `CONFIG_FILE_NAME`、`TEMPLATE_REF`、`ENV_GITHUB_MIRROR_BASE_URL`），
  平台相关的字符串集中在后端 crate 内，不要散落到 CLI。
- 注释与标识符用英文，注释解释**为什么**而不是复述代码。
- 平台相关的行为放在对应后端 crate；`pacor-cli` 不出现任何平台分支。
- 新增平台时，除了实现 `BundleBuilder`，还要改 `crates/pacor-packaging/src/lib.rs` 的
  `packager_for`、`apply_format_overrides` 和它的测试，以及 `core` 里的 `Platform`。

## 测试

- 单元测试放文件底部 `#[cfg(test)] mod tests`；跨模块与端到端放 `crates/<crate>/tests/*.rs`
  （目前只有 `pacor-packaging-android/tests/android_smoke.rs`）。
- 测试名读起来就是断言：`the_windows_version_is_always_four_numeric_components`、
  `sizes_are_rendered_in_binary_units`。不需要 `test_` 前缀（历史遗留的保留即可）。
- **端到端测试不依赖真实工具链**：用 fixture 工程 + 桩可执行文件（`flutter`、`jarsigner`、
  `apksigner`），把传给它们的**完整参数向量**断言下来。这样 `cargo test --workspace` 在任意
  CI 主机上都能跑，也是"参数被改动"这类回归唯一的防线。
- 需要真实工具链的测试标 `#[ignore]`，并在文档里写清前置条件（例：
  `cargo test -p pacor-packaging-windows -- --ignored` 需要预置的 NSIS/WiX 目录，见
  `docs/WINDOWS.md`）。
- 修改行为环境变量的测试要串行执行并恢复现场：用 `temp-env` 的 `with_vars`，并像
  `android_smoke.rs` 的 `serialised()` 那样把并行测试串起来——`set_var` 在 edition 2024 里是
  `unsafe`，并发调用未定义行为。
- 修 bug 先写一个能复现的失败测试（最小 fixture 也行），再改代码。

## 文档

- 行为变了就在**同一个提交**里改文档。文档与实现不一致按缺陷处理——本仓库有过
  "README 写着已实现、实现里是 `TODO`"的记录，别再来一次。
- 语言：`docs/*.md` 用中文；`README.md` 英文、`README.zh-CN.md` 中文，两份内容要同步。
  代码注释和提交信息用英文。
- 新增平台/格式时同步四处：`docs/<PLATFORM>.md`、README（两份）的对应小节、
  `crates/pacor-core/src/config/template.toml` 的注释、以及后端代码里指向文档的链接。
- 排障内容写"现象 → 原因 → 做法"，给出可直接复制的命令。

## 提交规范

格式：`<type>(<scope>): <summary>`

- `type` 用 `feat` `fix` `docs` `refactor` `test` `ci` `chore` `perf`（当前仓库出现过
  `feat` `fix` `docs` `ci` `refactor`）。
- `scope` 是受影响的区域或 crate：`windows` `android` `core` `tools` `downloads` `build`
  `ci` 等。影响面很广时可以省略。
- `summary` 全小写、祈使句、不加句号、不超过 72 字符。
- **正文说明为什么**，不要复述 diff。仓库既有提交是几段英文散文，通常包含：
  1. 当时错在哪（最好带上实测到的输出或数字）；
  2. 为什么这样改（而不是别的方案）；
  3. 怎么验证的（跑过什么真实工具链、什么命令、什么结果）。
  需要并列时可以插 `-` 列表，但别把正文写成变更清单。
- 一个提交只做一件事。格式化、顺手修的注释、无关的重命名单独提交。
- 不写 `Co-authored-by` 或生成器署名；不用 `--no-verify` 跳过检查。
- 不要提交 `dist/`、`target/`、`tmp/`、探针工程或临时脚本（`.gitignore` 已覆盖
  `target/`、`tmp/`、`/test_*`）。实验性工程放 `target/` 下。
- 可对照的真实例子：

  ```text
  feat(android): package App Bundles and sign what is shipped

  The Android backend collected Flutter's APKs and got almost everything after that
  wrong. Signing was a `TODO` that logged a warning and returned `Ok`, ...

  Signing is now real and validated before Gradle runs: a configured `keystore_file`
  requires the alias and both passwords, ...

  Verified end to end against a real Flutter project (`flutter create
  --platforms=android`) here: 5.7 MB and 6.3 MB split APKs plus a 23.8 MB bundle, ...
  ```

## 分支、发布与 CI

- 默认分支 `main`。改动走 PR，`pr-check` 必须绿；打了 `automerge` 标签且没有失败/取消的
  检查时会自动合并（来自 fork 的 PR 不会，token 只读）。
- **版本号只改 `Cargo.toml` 的 `[workspace.package]`**（各 crate 都是 `version.workspace = true`），
  改完必须跑 `cargo update --workspace` 让 `Cargo.lock` 跟上，否则 `--locked` 构建会失败。
- 发布由 `v*` tag 触发，tag 必须与 `[workspace.package] version` 完全一致（`publish.yml` 的
  `verify` 会先比对）。推荐用 **Bump Version** 那个 workflow 完成"改版本 + 同步 lock + 打 tag"，
  不要手工分几步做。
- 不要移动已发布的 tag，也不要手改 release 资产——重发就重新跑 `publish` 的 dispatch。
- workflow 里的 action 用**版本号**引用（`@v7`、`@v5`、`@v4`），不要 `@main`/`@latest`，也不要
  commit SHA。
- 改 workflow 后用 actionlint 校验（CI 的 `workflow-lint` 也会跑）：
  ```bash
  docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:latest -color
  ```
- 工作流全景见 `docs/CI.md`：七个文件各自何时触发、缓存键怎么算、发布与 nightly 的细节。

## 已知坑

- **CRLF**：`Cargo.toml` 在部分 Windows 检出里是 CRLF，`sed '/^\[workspace\.package\]$/ ...'`
  会完全匹配不到（报错看起来像"键不存在"）。读它之前先 `tr -d '\r'`，写它时保留原行尾——
  `publish.yml` / `bump.yml` 就是这么处理的。
- **符号文件**：Android 的 `--split-debug-info` 落在项目内 `tmp/`，由 `pacor clean` 删除，
  `--skip-cleanup` 时保留。`tmp/` 已在 `.gitignore` 里，别提交。
- **GitHub 直连**在某些网络下会被重置（raw、API、release 资产都可能）。代理与镜像的做法写在
  `docs/ENVIRONMENT_VARIABLES.md`，遇到下载超时先看那里，不要直接改代码绕过。
- **平台状态表**只能写已经真实跑通的东西。README 的状态表是发布承诺，不是路线图。

## 提交前清单

1. `cargo fmt --all -- --check`、`clippy`（debug + release）、`test --workspace` 全绿。
2. 行为变了 → 同一个提交里更新了文档（`docs/*.md`、两个 README、`template.toml` 注释）。
3. 新增/修改的参数、环境变量、路径都在文档里有出处。
4. 提交信息说明了为什么与怎么验证，而不是列举改了什么。
5. `git status` 里没有 `target/`、`tmp/`、探针工程或临时脚本。
