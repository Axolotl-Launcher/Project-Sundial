<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Axolotl Launcher (AXLauncher)

Rust + [GPUI Kit](https://gpui-kit.com) 桌面应用骨架，pnpm + Turbo 负责任务编排。

修改仓库前先读 [AGENTS.md](AGENTS.md)，其中列出了代码归属、修改前的影响分析和交付前的检查。

## 结构

```
apps/core/spi         axlauncher_spi         —— 端口 trait（SPI）与跨界类型
apps/core/domain      axlauncher_domain      —— 领域标识与基础类型
apps/core/content     axlauncher_content     —— 内容模型与 provider
apps/core/install     axlauncher_install     —— 安装计划与执行
apps/core/launch      axlauncher_launch      —— 启动、Java 发现、进程
apps/core/loader      axlauncher_loader      —— 加载器版本查询与版本配置解析
apps/core/state       axlauncher_state       —— 应用状态与持久化
apps/core/storage     axlauncher_storage     —— 文件系统与缓存原语
apps/core/network     axlauncher_network     —— HTTP 与协议客户端
apps/core/telemetry   axlauncher_telemetry   —— 事件与 span
apps/core/scheduler   axlauncher_scheduler   —— 后台任务调度
apps/core/resources   axlauncher_resources   —— 收藏、历史、指纹
apps/core/mod_metadata axlauncher_mod_metadata   —— 加载器与 mod 元数据
apps/core/observability axlauncher_observability   —— 命令指标与诊断
apps/core             axlauncher_core        —— 聚合层，re-export 全部系统
apps/ui               axlauncher_ui          —— 调色板 + 允许使用的 gpui-kit 词表
apps/frontend         axlauncher_frontend    —— 应用状态、布局与窗口装配、provider 选择
platforms/*                                  —— 各平台可执行文件
tests                                        —— 仓库级策略测试与 SPI 契约测试
```

### 分层与 SPI

每个系统是独立 crate，因此**依赖方向由 Cargo 强制**，而不是目录约定：

```
apps/core/*  ->  spi  ->  domain
apps/frontend  ->  ui, core（选择具体 provider）
platforms/*    ->  frontend
```

`spi` 定义端口（`Network`、`Storage`、`ContentProvider`、`Installer`、`Launcher`、
`StateStore`、`Telemetry`、`ModLoader`），各系统提供实现，`frontend` 在启动时决定装配哪一个。
服务调用方依赖 `spi`。原子写入、下载校验和游戏清单模型有明确的跨 crate 复用边界，
具体归属和允许的依赖见 [crate 职责说明](.agents/docs/crates.md)。

异步端口用返回位置 `impl Trait` 而非 `async_trait`，不产生每次调用的装箱。
`tests/tests/spi_contract.rs` 验证：一个只针对端口编写的调用方，能与任意
provider 配合工作。

## gpui-kit 版本

依赖 `gpui-kit = "0.10.0"`。它一揽子锁定 GPUI 家族：

- `gpui-pre 0.3.12` —— Zed `crates/gpui` 的长期跟进快照，以 `=0.3.12` 精确锁定
- `gpui-base 0.10.0` —— 无样式行为层
- `gpui-component 0.10.0` —— 75+ 样式组件
- `gpui-kit-assets 0.10.0` —— Lucide 图标资源

已在 Windows (MSVC) 下验证可编译并启动窗口。升级时整体升级 `gpui-kit`，
不要单独改 `gpui-pre`：gpui-kit 的发布说明指出，只在 `gpui-pre` 上做 caret
依赖会让应用跳到组件未适配的新快照（上游 issue #3156）。

## 常用命令

```bash
pnpm install                                   # 安装 turbo
pnpm dev:windows                               # 运行 Windows 壳（turbo -> cargo run）
pnpm build                                     # 构建当前平台（turbo 只调度本机平台壳）
pnpm test / pnpm lint                          # 各包 cargo test / clippy，按依赖顺序
pnpm check                                     # 本机包的 lint/test、格式、脚本和钩子检查
pnpm deny                                      # cargo deny check
pnpm shear                                     # cargo shear（未使用依赖）
pnpm nextest                                   # cargo nextest run --workspace
cargo build -p axlauncher_platform_windows   # 直接调用 cargo
```

默认数据目录为系统应用数据目录下的 `AXLauncher`。如需继续使用已有的设置和实例，可将
`AXLAUNCHER_DATA_DIR` 指向原数据目录。

## Git 钩子

用 [prek](https://github.com/j178/prek) 管理本地钩子，配置在 `prek.toml`。
`prek install` 写入 `pre-commit`、`pre-push`、`commit-msg` 三个钩子：

```bash
prek install          # 安装钩子（每个克隆执行一次）
prek run --all-files  # 手动对全仓跑一遍 pre-commit 检查
prek run --stage pre-push
```

钩子只做提交前后值得快速修复的检查：

| 阶段 | 内容 |
|---|---|
| `commit-msg` | DCO 签名、提交信息为英文 |
| `pre-commit` | 空白/编码、结构化文件解析、大文件、私钥泄漏、cargo fmt、tombi format、代码文件 500 行上限 |
| `pre-push` | 待推送提交的 DCO trailer 与 GPG 签名 |

clippy、测试、cargo deny/shear、typos、REUSE、tsc、ESLint 等完整门禁由 CI
承担，本地可用 `pnpm check` 执行其中大部分。CI 的 `Hooks` job 对全仓跑
pre-commit 阶段，既验证钩子配置可用，也补上本地因只检查暂存文件而跳过的部分。

## 脚本语言

`scripts/` 下的文件一律用 TypeScript，由 Node 24 直接剥离类型运行，不引入
构建步骤或转译器：

```bash
node scripts/turbo.ts test
pnpm tsc      # 类型检查；同时拒绝 Node 无法剥离的语法
pnpm eslint
pnpm nojs     # 拒绝任何被跟踪的 .js/.mjs/.cjs/.jsx 文件
pnpm file-length  # 拒绝超过 500 行的代码文件
```

唯一例外是 `eslint.config.js`：ESLint 在 TypeScript 工具链就绪前就要加载它，
因此在 `scripts/check-language.ts` 中按精确路径放行。

## 工程约束

- **工具链单一来源**：`rust-toolchain.toml` 的 channel、`Cargo.toml` 的
  `rust-version`、`clippy.toml` 的 `msrv` 三处都是 `1.100.0`，改一处必须同步改另两处。
- **Lint 会被真正继承**：根 `[workspace.lints]` 要求每个 crate 声明
  `[lints] workspace = true`；缺了这条，lint 一条都不会跑。
- **依赖方向**：只有 `apps/ui` 可以出现 `gpui_kit` 路径，其余代码走
  `axlauncher_ui` 的重导出。
- **许可证**：全仓 `GPL-3.0-only`；`deny.toml` 只放行宽松许可证 + MPL-2.0
  + 本仓自身。
- **供应链**：pnpm `minimumReleaseAge: 10080`（7 天冷却）；Dependabot 覆盖
  cargo / npm / github-actions 三个生态；GitHub Actions 全部固定到 commit SHA。
- **依赖需有用**：`cargo shear` 在 CI 中失败于任何"声明但未使用"的依赖。
- **代码文件不超过 500 行**：`pnpm file-length` 在 pre-commit 和 CI 中检查。

## CI

`ci.yml` 在 Ubuntu、Windows 和 macOS 上运行 Rust 格式检查、Clippy、Nextest 和依赖检查。
Ubuntu 上的 Hygiene 和 Hooks 任务负责脚本、仓库规则和 Git 钩子检查。
分支保护使用汇总检查 `CI result`。

## turbo 的平台过滤

`turbo` 本身不认识宿主平台，直接 `turbo run build` 会在 Windows 上连 Linux 壳一起编译。
根脚本经 `scripts/turbo.ts` 注入 `--filter`，只调度 `apps/*` 加当前平台的壳。

## 网络

若 crates.io 直连受阻（企业代理、镜像环境等），可在 `~/.cargo/config.toml` 配置镜像，
或临时用：

```bash
cargo build --config 'source.crates-io.replace-with="mirror"' \
            --config 'source.mirror.registry="sparse+<镜像地址>/index/"'
```

`Cargo.lock` 已提交，日常构建不需要重新解析索引。

## 许可与 REUSE

全仓 GPL-3.0-only，许可证全文在 `LICENSES/GPL-3.0-only.txt`。每个文件都带有
SPDX 标签，或（对 JSON、锁文件等无法写注释的格式）在 `REUSE.toml` 中声明。

```bash
reuse lint
```

## 平台渠道

| 渠道 | 构建主机 | 产物 | 验证状态 |
|---|---|---|---|
| `platforms/windows` | Windows | `axlauncher.exe` | 本机实测编译与运行 |
| `platforms/linux` | Linux | ELF 可执行文件 | CI（ubuntu-24.04）实测 |
| `platforms/macos` | macOS | Mach-O 可执行文件，可打包为 `.app` | 仅 Windows 上 `cargo check` 通过；首次 macOS CI 运行为实际编译验证 |

`apps/*` 与三个平台壳在同一 workspace 内，`cargo check --workspace` 可在任一主机
对全部壳做类型检查（macOS 壳无平台专属代码，因此跨主机可检查；但链接与运行需要
对应主机）。

## 平台依赖

- **Windows**：无额外依赖；gpui-pre 走 Win32 + DirectWrite + DirectX。
- **Linux**：fontconfig、Wayland/X11 开发库与 Vulkan；`libdbus-1-dev`（oo7
  secret service 依赖）。CI 已列出完整 apt 清单。
- **macOS**：需要 Xcode 命令行工具；Metal 渲染。打包、签名与公证见
  `packaging/macos/README.md`。
