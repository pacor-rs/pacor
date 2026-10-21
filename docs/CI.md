# CI 与发布流程

`.github/workflows` 下有七个工作流。它们的共同目标是：**能在本地跑通的检查，在 PR 上
一定跑；不能本地跑的东西（六个平台的二进制）只在真需要的时候跑；任何"看起来绿了但什么
都没做"的状态都要被显式说出来。**

## 一览

| 文件 | 触发 | 作用 |
| ---- | ---- | ---- |
| `pr-check.yml` | PR → `main`（`crates/**`、`Cargo.toml`、`Cargo.lock`、`.cargo/**`、`.github/**`、`docs/**`、`*.md`） | 三平台的 fmt / clippy / test / release 构建，release profile 的 clippy，三个交叉目标的 `cargo check`，actionlint，以及打了标签的自动合并 |
| `build-binaries.yml` | 被 `publish.yml` 与 `nightly.yml` 以 `workflow_call` 调用 | 六个平台的 release 二进制：构建、可选 UPX、冒烟测试、打包、`.sha256`、上传 artifact |
| `publish.yml` | 推送 `v*` tag，或手动 dispatch（填 `tag`） | 校验 tag 与 `Cargo.toml` 的版本一致 → 调 `build-binaries` → 建草稿 release → 上传 → 发布 |
| `nightly.yml` | 每天 04:00 UTC，或手动 dispatch | 构建 `main`，整体替换 `nightly` 预发布 |
| `warm-cache.yml` | `main` 上的 Cargo 文件变更、每周一 05:00 UTC、手动 dispatch | 提前把 cargo registry 与 target 缓存写好 |
| `cleanup-old-caches.yml` | 每周一 00:00 UTC，或手动 dispatch（可 `dry_run`） | 每个缓存族只保留最新一条 |
| `bump.yml` | 手动 dispatch（`version`，可选 `dry_run`） | 改版本号、同步 `Cargo.lock`、提交、打 tag，并触发发布 |

## 发版

1. 跑 **Bump Version**：`version` 填 `0.2.0` 或 `0.2.0-alpha.1`，先 `dry_run` 看 diff 也行。
   它做四件事：改 `[workspace.package] version`（其他 crate 都继承它）、
   `cargo update --workspace` 同步 `Cargo.lock`、提交 `chore(release): vX`、打 annotated tag。
   版本号必须显式给出——仓库里没有 `npm`/`cargo-release`，而"`patch` 对 `0.1.0-alpha`
   意味着什么"应该是人决定的。
2. 它随后触发 **Publish Release**。注意这里有个坑：**用 `GITHUB_TOKEN` 推的 tag 不会触发
   其它工作流**（GitHub 防递归），所以 `bump.yml` 在推完 tag 后显式调用
   `gh workflow run publish.yml -f tag=vX`，而不是指望 tag 自己唤醒发布流程。
   （如果想完全靠 tag 触发，就给这一步换一个有 `repo` 权限的 PAT。）
3. `verify` 先比对 `v$version` 与 tag，不一致 **5 秒内**失败，不会先烧掉六个 runner；
   `Cargo.lock` 与清单是否一致，由构建时的 `--locked` 保证。
4. 六个平台并行构建 → 每个产出一个压平的压缩包（解压就是二进制，不带
   `target/<triple>/release/` 目录结构）+ 同名 `.sha256`。
   **UPX 压缩是可选的，默认关闭**：需要更小的下载体积时，在手动 dispatch 时勾上
   `compress`（Linux 与 Windows 生效，压不动会打 warning 并继续，不阻塞发布）。
   默认关闭的原因是它会把可执行文件改写——崩溃报告里的回溯变难读，也更容易触发杀软误报；
   用于排查的 nightly 尤其不需要它。macOS 从不压缩：UPX 产出的 Mach-O 不可靠。
5. release **先建草稿、上传完再 publish**：中途失败不会留下一个被 "latest" 指向的半成品。
6. 版本号带 `-alpha`/`-beta`/任何预发布后缀时自动加 `--prerelease --latest=false`，
   不会占用 latest。
7. 资产：`pacor-<Platform>.zip|tar.gz`、同名 `.sha256`，以及汇总的 `SHA256SUMS`
   （`sha256sum -c SHA256SUMS` 可直接校验，也是 winget/Scoop/Homebrew 想要的格式）。

**重发同一个 tag**：dispatch 时填 `tag`，会复用该 tag 的树重新构建，并覆盖已有 release 的
说明与资产（`gh release upload --clobber`）。

## Nightly

- 每天 04:00 UTC 构建 `main`；`nightly` 是**移动 tag**，每次先删掉旧 release 与 tag
  再重建，所以它永远只代表"最近一次能编译通过的 `main`"。
- 什么时候跳过：`nightly` 已经指向同一个 commit；或者 HEAD 本身带着 `v*` 发布 tag
  （那次发布已经覆盖它了）。跳过时写一条 `GITHUB_STEP_SUMMARY` 说明原因，
  免得"什么都没构建"被误读成"什么都验过了"。
- 中间产物保留 1 天。

## 缓存

缓存键的形状（`<cachekey>` 是 dtolnay/rust-toolchain 输出的 rustc 版本哈希）：

```
<os>-cargo-registry-<cachekey>-<hash(Cargo.lock)>
<os>-rust-debug-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<target>-<cachekey>-<hash(Cargo.lock)>
<os>-rust-cross-<target>-<cachekey>-<hash(Cargo.lock)>
```

几条规则，都是踩过才写下来的：

- **`restore-keys` 里保留 toolchain**。cargo 的 fingerprint 含 rustc 版本，换编译器后
  旧产物一个都用不上，恢复它只是白下载几百 MB；让 `stable` 更新后老实冷编译一次更好。
- **registry 与 target 分开缓存**。依赖没变时 target 缓存命中，`cargo fetch` 也就不必再
  走网络。
- **`CARGO_INCREMENTAL=0`**。增量数据不在缓存路径里，开着只会让恢复出来的目录更大，
  并在恢复后打印一条 "could not be used" 警告。
- **`warm-cache.yml` 用 `actions/cache`（读+写），PR 用 `actions/cache/restore`（只读），
  两边键完全一致**——否则预热出来的是一套没人读的缓存。
- GitHub 对每个仓库的缓存有 10 GB 上限，超了就按 LRU **静默驱逐**，通常正好发生在需要它的
  那个 PR 之前。`cleanup-old-caches.yml` 按"键的最后一个 `-` 之前"分族（正好等于
  `restore-keys` 的前缀），每族只留最新一条。

## 漂移防护

- **所有 action 用版本号引用**（`@v7`、`@v5.1.0`），不用 commit SHA：评审时能直接看出
  跑的是哪个版本。浮动分支（`@main`）不叫版本号，一律不用。
- **每个 job 都有 `timeout-minutes`**：挂死的 job 不会占着 runner 跑 6 小时。
- **concurrency**：PR 上取消旧运行（新 push 让旧结果失效）；发布、清理、提版本有并发组
  但不取消——一个上传或删除循环被腰斩，留下的半成品比慢一点更糟。
- **workflow-lint 跑 actionlint**（有 shellcheck 时一并检查 `run:` 块，`-S warning` 才失败）。
  它能抓住的典型问题：`if:` 里写错 job 名（钩子静默不生效，看起来和通过一模一样）、
  引用了不存在的 `needs`、表达式拼错。安装用官方发布包并按官方校验和验证，
  不依赖镜像 tag。
- **自动合并**只在同时满足这些条件时发生：PR 来自本仓库分支（fork 的 token 只读）、
  打了 `automerge` 标签、且没有失败或取消的检查（跳过的检查可以接受）。

## 本地等价命令

CI 跑的就是这些：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo clippy --workspace --all-targets --release --locked -- -D warnings   # release-only 的代码

RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-pc-windows-msvc
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target x86_64-apple-darwin
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-apple-darwin
```

workflow 本身的改动可以用 actionlint 本地校验：

```bash
docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:latest -color
```

## 刻意不做的事

- **不在 CI 里跑真实打包**。`pacor build` 需要 Flutter SDK，Android 还要 SDK + JDK，
  Windows 要 NSIS/WiX 工具链——这些放进 CI 会让每个 PR 都变成几十分钟。因此
  `cargo test -p pacor-packaging-windows -- --ignored`（需要预置 `makensis` 的
  NSIS 端到端测试）仍只在本地按需运行，见 `docs/WINDOWS.md`。
- **不给 pacor 自身做 macOS 签名/公证**。发布的是 CLI 二进制；`codesign`/`notarytool`
  是 pacor 给用户产物做的事（`--target macos`），与它自己的构建无关。
- **不猜版本号**：`bump.yml` 要求显式 `version`。
- **不测 Windows-arm64 交叉构建的工具链**：`aarch64-pc-windows-msvc` 依赖
  `windows-latest` 镜像自带的 ARM64 MSVC 工具链，这一点和改造前一致。
