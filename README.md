# Superlogical

Rust + [GPUI Kit](https://gpui-kit.com) 桌面应用骨架，pnpm + Turbo 负责任务编排。

## 结构

```
apps/core          superlogical_core          —— 领域逻辑，不依赖 UI 工具包
apps/ui            superlogical_ui            —— 调色板 + 允许使用的 gpui-kit 组件词表
apps/frontend      superlogical_frontend      —— 应用状态、布局与窗口装配
platforms/windows  superlogical_platform_windows —— 产出 superlogical.exe
platforms/linux    superlogical_platform_linux   —— 产出 Linux 可执行文件
```

依赖方向严格单向：

```
core  <-  ui        （ui 只依赖 gpui-kit，不认识 core）
core  <-  frontend  ->  ui
platforms/*  ->  frontend
```

- `apps/ui` 不自己画控件，只把 gpui-kit 的组件按单一导入路径重新导出，并持有
  应用调色板（`Palette`）。换组件体系时只改这一个 crate。
- `apps/frontend` 是唯一同时组合 `core` 与 `ui` 的 crate，持有 `RootView` 等实体与事件处理。
- `platforms/*` 只调用 `superlogical_frontend::run()`，后续按平台追加图标、清单、打包与发布。

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
pnpm check                                     # 本地全套校验（不含 clippy/deny）
pnpm deny                                      # cargo deny check
pnpm shear                                     # cargo shear（未使用依赖）
pnpm nextest                                   # cargo nextest run --workspace
cargo build -p superlogical_platform_windows   # 直接调用 cargo
```

## 工程约束

- **工具链单一来源**：`rust-toolchain.toml` 的 channel、`Cargo.toml` 的
  `rust-version`、`clippy.toml` 的 `msrv` 三处都是 `1.100.0`，改一处必须同步改另两处。
- **Lint 会被真正继承**：根 `[workspace.lints]` 要求每个 crate 声明
  `[lints] workspace = true`；缺了这条，lint 一条都不会跑。
- **依赖方向**：只有 `apps/ui` 可以出现 `gpui_kit` 路径，其余代码走
  `superlogical_ui` 的重导出。
- **许可证**：全仓 `GPL-3.0-only`；`deny.toml` 只放行宽松许可证 + MPL-2.0
  + 本仓自身。
- **供应链**：pnpm `minimumReleaseAge: 10080`（7 天冷却）；Dependabot 覆盖
  cargo / npm / github-actions 三个生态；GitHub Actions 全部固定到 commit SHA。
- **依赖需有用**：`cargo shear` 在 CI 中失败于任何"声明但未使用"的依赖。

## CI

`.github/workflows/ci.yml` 在 Ubuntu 与 Windows 上跑 fmt、clippy（PR 上硬失败）、
nextest、cargo-shear、cargo-deny；`hygiene` job 跑 typos 与 tombi。分支保护只需
一个 required check：`CI result`。

## turbo 的平台过滤

`turbo` 本身不认识宿主平台，直接 `turbo run build` 会在 Windows 上连 Linux 壳一起编译。
根脚本经 `scripts/turbo.mjs` 注入 `--filter`，只调度 `apps/*` 加当前平台的壳。

## 网络

若 crates.io 直连受阻（企业代理、镜像环境等），可在 `~/.cargo/config.toml` 配置镜像，
或临时用：

```bash
cargo build --config 'source.crates-io.replace-with="mirror"' \
            --config 'source.mirror.registry="sparse+<镜像地址>/index/"'
```

`Cargo.lock` 已提交，日常构建不需要重新解析索引。

## 平台依赖

- **Windows**：无额外依赖；gpui-pre 走 Win32 + DirectWrite + DirectX。
- **Linux**：需要 fontconfig、X11/Wayland 开发库与可用的 Vulkan 驱动；在 Linux 主机上
  构建即可，交叉编译不受支持。
