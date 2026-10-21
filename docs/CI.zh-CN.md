<p align='center'>
<a href="CI.md">English</a> | <span>中文</span>
</p>

# CI 与发布流程

`.github/workflows` 下有七个工作流。它们的共同目标是：**能在本地跑通的检查，在 PR 上
一定跑；不能本地跑的东西（六个平台的二进制）只在真需要的时候跑；任何"看起来绿了但什么
都没做"的状态都要被显式说出来。**

## 一览

| 文件 | 触发 | 作用 |
| ---- | ---- | ---- |
| `pr-check.yml` | PR → `main`（`crates/**`、`Cargo.toml`、`Cargo.lock`、`.cargo/**`、`.github/**`、`docs/**`、`*.md`） | 三平台的 fmt / clippy / test / release 构建，release profile 的 clippy，四个交叉目标的 `cargo check`，actionlint，以及打了标签的自动合并 |
| `build-binaries.yml` | 被 `publish.yml` 与 `nightly.yml` 以 `workflow_call` 调用 | 六个平台的 release 二进制：构建、可选 UPX、冒烟测试、打包、`.sha256`、上传 artifact |
| `publish.yml` | 手动 dispatch：版本提升方式与通道，或填 `tag` 重发已有版本 | 在临时 `release` 分支上提升版本 → 检查该分支 → 调 `build-binaries` → 在 `release` 分支 head 上建草稿 release、上传、公开 → 快进 `main` → 删除分支 |
| `nightly.yml` | 每天 04:00 UTC，或手动 dispatch | 构建 `main`，整体替换 `nightly` 预发布 |
| `warm-cache.yml` | `main` 上的 Cargo 文件变更、每周一 05:00 UTC、手动 dispatch | 提前把 cargo registry 与 target 缓存写好 |
| `cleanup-old-caches.yml` | 每周一 00:00 UTC，或手动 dispatch（可 `dry_run`） | 每个缓存族只保留最新一条 |
| `delete-merged-branch.yml` | PR 被合并后关闭 | 删除该分支；长期分支、来自 fork 的分支、以及仍被其它未合并 PR 使用的分支除外 |

## 发版

1. 手动 dispatch **Publish Release**，选择版本怎么走（`patch`、`minor`、`major`，或只递增
   预发布号的 `prerelease`）和发布通道（`stable`、`alpha`、`beta`）。填了 `tag` 则改为重发
   已有的 tag，不碰版本号。
2. `prepare` 按 npm 的规则算出新版本——`0.1.0-alpha` 上做 `patch` 得到 `0.1.0`，因为那个
   预发布本身就是这次补丁发布——然后改 `[workspace.package] version`、跑
   `cargo update --workspace`、提交 `chore(release): vX`，推到一个临时的 `release` 分支。
   听起来"为了一个版本号还得建分支再删"比直接推 `main` 更麻烦，其实不是：这个分支除了本
   工作流没人读，而 `main` 在发布公开之前一动不动。
3. `check` 会在**这个分支上**跑 `fmt`、`clippy` 与测试，因为没有 PR 评审过它。之后六个平台
   从同一个 ref 构建。
4. 每个平台产出一个压平的压缩包（解压就是二进制，不带 `target/<triple>/release/` 结构），
   并配一份同名 `.sha256`。**UPX 压缩可选、默认关闭**：勾上 `compress` 才会压（仅 Linux 与
   Windows；压不动只打 warning，不阻塞发布）。默认关闭是因为它改写可执行文件——崩溃回溯
   变难读、更容易触发杀软误报。macOS 从不压缩：UPX 产出的 Mach-O 不可靠。
5. release **先建草稿**，资产传完才公开。说明里的下载列表是**按真实存在的资产**渲染的，
   尺寸也是实测值，因此不可能与上传结果不一致。
6. 预发布通道还会删掉更早的预发布：更新器只看最新的一个，页面上留着的旧版本只会让人装错。
   它们的 tag 会保留，历史仍可从源码复现。
7. `merge-back` 把 `main` 快进到 bump 提交——**只在**发布已经公开、且构建期间 `main` 没动过
   时（快进失败就说明有人合并了）。它**不**创建 tag：草稿 release 已经在 `release` 分支的
   head 上建好了，而 `merge-back` 只有在 `vX` 与 `main` 将要落到的那个 commit 是同一个时
   才推送。重复打标签正是第一次真实发版失败的方式——快进做完了、`main` 没推上去，公开的
   tag 指向的还是 bump **之前**的树。
8. `cleanup` 在成功时删除 `release` 分支；失败的运行会留着它给排查的人看，下次运行会强制
   覆盖。正式版发布还会撤下 `nightly` 发布与标签：用 nightly 的人离正式版只差一条命令。

**重发同一个 tag**（填 `tag`）会用该 tag 的树重建、用 `--clobber` 覆盖资产并刷新说明——
不升版本、不回合并、不清理。这是资产坏掉之后的出路。

**为什么是 dispatch 而不是推 tag 触发**：现在 tag 是这条流水线**产出**的，`push: tags: v*`
的触发会让它在 merge-back 那一步重新进入自己。
## Nightly

- 每天 04:00 UTC 构建 `main`；`nightly` 是**移动 tag**，每次先删掉旧 release 与 tag
  再重建，所以它永远只代表"最近一次能编译通过的 `main`"。
- 什么时候跳过：`nightly` 已经指向同一个 commit；或者 HEAD 本身带着 `v*` 发布 tag
  （那次发布已经覆盖它了）。跳过时写一条 `GITHUB_STEP_SUMMARY` 说明原因，
  免得"什么都没构建"被误读成"什么都验过了"。
- 中间产物保留 1 天；正式版发布会删掉 `nightly` 发布与标签（`publish.yml` 的最后一个
  job）——用 nightly 的人离正式版只差一条命令，两个都留着等于让他们继续装昨天的构建。

## 分支清理

PR 被关闭**且已合并**时，`delete-merged-branch.yml` 会删除它的头分支——打 `automerge`
标签自动合并与人工点"Merge"是同一个事件，都走这里。以下情况保留分支：

- 分支名属于长期分支（`main`、`master`、`develop`、`release/*`、`release-*`）：
  某个 PR 恰好以它为头分支，不构成删除它的理由；
- PR 来自 fork：分支在别人的仓库里，这个 token 够不着；
- 还有其它未合并的 PR 以它为头分支——堆叠 PR 正是这种形状，删掉 ref 会让子 PR 未经决定
  就被关闭。

ref 本来就不存在算成功：如果仓库开启了"自动删除头分支"设置，这个工作流只是第二道保险而
已。每次运行都会把结果写进 job summary，被保留下来的分支旁边就写着原因。

## 缓存

缓存键的形状（`<cachekey>` 是 dtolnay/rust-toolchain 输出的 rustc 版本哈希）：

```
<os>-cargo-registry-<cachekey>-<hash(Cargo.lock)>
<os>-rust-debug-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<cachekey>-<hash(Cargo.lock)>
<os>-rust-release-<target>-<cachekey>-<hash(Cargo.lock)>
<os>-rust-cross-<target>-<cachekey>-<hash(Cargo.lock)>
```

前三族是宿主 triple，由 `warm-cache.yml` 写，这是 PR 保持快速的来源。后两族带 target，
由**读它们的那些作业自己写**——`build-binaries`（每次 nightly 与发版）和 `cross-check`。
之所以值得，是因为 nightly 每天都构建全部六个目标：`Cargo.lock` 变动后的第一次运行
付出上传成本，之后每次都是恢复。

几条规则，都是踩过才写下来的：

- **`restore-keys` 里保留 toolchain**。cargo 的 fingerprint 含 rustc 版本，换编译器后
  旧产物一个都用不上，恢复它只是白下载几百 MB；让 `stable` 更新后老实冷编译一次更好。
- **registry 与 target 分开缓存**。依赖没变时 target 缓存命中，`cargo fetch` 也就不必再
  走网络。
- **`CARGO_INCREMENTAL=0`**。增量数据不在缓存路径里，开着只会让恢复出来的目录更大，
  并在恢复后打印一条 "could not be used" 警告。
- **每个缓存步骤要么自己写，要么与别人共用一族**。`warm-cache.yml` 对宿主 triple 的
  三族用 `actions/cache`（读+写），`build-binaries` 与 `cross-check` 对各自的 per-target
  键同样用它。没有任何地方单独使用 `actions/cache/restore`：只读的步骤是一个永远不可能
  命中的缓存，而这两个工作流里曾经有两条这样的步骤，文档还把它们当成了有效配置。
- GitHub 对每个仓库的缓存有 10 GB 上限，超了就按 LRU **静默驱逐**，通常正好发生在需要它的
  那个 PR 之前。`cleanup-old-caches.yml` 按"键的最后一个 `-` 之前"分族（正好等于
  `restore-keys` 的前缀），每族只留最新一条。

## 漂移防护

- **`main` 只通过 PR 接收改动。** 所有改动先落在各自的分支上，`pr-check` 全绿后才合并；
  不直接向 `main` 推送。本仓库所在的 Plan 无法启用分支保护，所以这一条靠评审而不是靠
  GitHub 执行。唯一例外是 `bump.yml` 推的发布版本提升，它只改版本号与 lock 文件。
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
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked --target aarch64-unknown-linux-gnu
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
- **不接受手写的版本号**：版本由 `bump_type` × `release_channel` 算出，因此版本号所在的
  三处（清单、lock 文件、tag）不可能互相矛盾。
- **不测 Windows-arm64 交叉构建的工具链**：`aarch64-pc-windows-msvc` 依赖
  `windows-latest` 镜像自带的 ARM64 MSVC 工具链，这一点和改造前一致。
- **不在 `warm-cache.yml` 里预热 release 构建**，因为那些缓存是 per-target 的
  （`target/<triple>/release/`），由使用它们的构建自己写。`nightly` 每天构建全部六个
  目标，所以 nightly 本身就是预热器；发版会 bump `Cargo.lock`、精确键必然 miss，靠
  `restore-keys` 恢复后只重编工作区自己的 crate。`cross-check` 出于同样理由保留自己的
  debug 缓存。
