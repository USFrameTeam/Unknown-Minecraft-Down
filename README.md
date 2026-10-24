# MC-Downloader-GUI

Minecraft Java 版「**仅下载器**」桌面应用：解析版本清单 → 下载官方文件 → 按标准 `.minecraft` 目录放置并校验。仅此三件事。

> ⚠️ **免责与建议**：本下载器仅作为备用方案，专注于游戏文件的纯净下载与目录构建。想要完善的启动器功能（如正版登录、模组管理、整合包安装、版本隔离等），强烈建议使用官方启动器或非中国大陆区的第三方启动器（如 Prism Launcher、MultiMC 等）。

## 功能

- **版本清单**：Mojang 官方 / BMCLAPI 镜像一键切换；release / snapshot / old_beta / old_alpha 筛选与搜索；Fabric Loader 版本安装（自动解析 `inheritsFrom` 继承链）。
- **下载引擎**（Rust + Tokio，`src-tauri/engine`）：
  - Range 断点续传（`.part` 保留断点，重启/暂停后从偏移继续，进度不归零）；
  - 流式 SHA-1 校验，通过后才原子改名落位；失败自动重试；
  - 并发 1–32 可配置；任务级取消/暂停；
  - 完整处理 version JSON 的 `rules`（os/arch/features）、natives 分类与解压、`assets` 索引展开、legacy 虚拟资源；
  - 路径防穿越：`safe_join` + canonicalize 兜底 + natives 白名单提取。
- **兼容性**：版本 JSON / 客户端 jar / libraries / assets 全部按标准目录放置，继承链上每个版本 JSON 原样落盘——下载完成后用 **PCL / HMCL / BakaXL 的「刷新版本 / 导入」** 即可识别。
- **安装校验**：一键 verify，逐文件核对 SHA-1，表格展示缺失/损坏。
- **首次启动向导**：强制引导配置 `.minecraft` 路径（自动探测 / 手动选择 / 新建），第一页展示免责声明。

## 不做的事（硬约束）

启动游戏、登录/正版验证、模组与整合包管理、修改第三方启动器私有配置、内置官方与 BMCLAPI 之外的下载源、执行任何下载得到的可执行文件。详见 [docs/DESIGN.md §6](docs/DESIGN.md)。

## 开发

依赖：Node 20+、Rust stable、Linux 需 `libwebkit2gtk-4.1-dev` 等（Tauri 常规依赖）。

```bash
npm install

npm run tauri dev      # 开发运行
npm run tauri build    # 打包

# 测试
npm run test:rust      # 引擎 + 应用层 Rust 测试（39 例）
npm test               # 前端单元测试（Vitest，11 例）
npm run test:e2e       # Playwright E2E（Tauri IPC mock，4 个场景）
```

> 网络说明：若 crates.io 官方索引不可达，本仓库 `.gitignore` 排除的 `.cargo/config.toml` 可配置镜像（如 rsproxy.cn），模板见文末。

### .cargo/config.toml 模板（仅本机，不入库）

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
retry = 3
```

## 目录结构

```
src-tauri/
  src/            Tauri 应用层（commands / state / persistence / error）
  engine/         纯下载引擎 crate（不依赖 tauri，可独立测试）
  icons/          应用图标
src/              React 19 前端（components / stores / lib / content）
tests/
  unit/           Vitest 单测
  e2e/            Playwright + __TAURI_INTERNALS__ mock
docs/DESIGN.md    架构、关键代码、Command/Event 签名、测试方案、不实现清单
```

## 技术栈

Tauri v2 · Rust stable（Tokio / reqwest-rustls / sha1 / zip）· React 19 · TypeScript · Vite · Tailwind CSS v4 · shadcn 风格组件 · Zustand · Vitest · Playwright
