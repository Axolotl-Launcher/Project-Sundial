# Superlogical

Rust + [gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui) 桌面应用骨架，
pnpm + Turbo 负责任务编排。

## 结构

```
apps/core          superlogical_core          —— 领域逻辑，不依赖 UI 工具包
apps/ui            superlogical_ui            —— gpui 视图与窗口（library）
platforms/windows  superlogical_platform_windows —— 产出 superlogical.exe
platforms/linux    superlogical_platform_linux   —— 产出 Linux 可执行文件
```

`apps/*` 只产出 lib；可执行文件由 `platforms/*` 组装，便于按平台追加图标、清单、
打包与发布步骤，而不污染共享的 UI 代码。UI 只从 `superlogical_core` 读状态，不反向依赖。

## gpui 版本

依赖 `gpui = "0.2.2"`，即 Zed 仓库 `crates/gpui` 在 crates.io 的官方发布版本。该版本在
Windows (MSVC) 下已验证可编译运行，无需额外系统依赖。

Zed `main` 分支上的 gpui 已把平台层拆成 `gpui_platform` 等未发布 crate，只能以 git
依赖整仓拉取（约 558 MB），上游 API 随时可能破坏性变更，因此当前不采用。如需跟进，
把 `Cargo.toml` 中的 gpui 换成 git 依赖并同步调整 `apps/ui` 的导入即可。

## 常用命令

```bash
pnpm install                                   # 安装 turbo
pnpm dev:windows                               # 运行 Windows 壳（turbo -> cargo run）
pnpm build                                     # 构建当前平台（turbo 只调度本机平台壳）
pnpm test / pnpm lint                          # 各包 cargo test / clippy，按依赖顺序
cargo build -p superlogical_platform_windows   # 直接调用 cargo
cargo test --workspace
cargo fmt --all && cargo clippy --workspace --all-targets
```

`turbo` 本身不认识宿主平台，直接 `turbo run build` 会在 Windows 上连 Linux 壳一起编译。
根脚本经 `scripts/turbo.mjs` 注入 `--filter`，只调度 `apps/*` 加当前平台的壳。

## 平台依赖

- **Windows**：无额外依赖，gpui 走 Win32 + DirectWrite + DirectX。
- **Linux**：需要 fontconfig、X11/Wayland 开发库与可用的 Vulkan 驱动；用
  `pnpm build` 在 Linux 主机上构建即可，交叉编译不受支持。
