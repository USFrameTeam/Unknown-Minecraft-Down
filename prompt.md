你是一名精通 Tauri v2、Rust 异步编程与现代化前端工程的全栈开发者，同时熟悉 DeepSeek Harness（dsh）插件与自动化测试体系。请为我设计并实现一个“Minecraft Java 版仅下载器”桌面应用。项目名暂定 MC-Downloader-GUI。

## 一、角色与核心硬约束（不可违反）

1. 下载器只做三件事：解析版本清单、下载文件、按标准目录放置并校验。绝不启动 Minecraft，绝不生成 Java 启动参数，绝不绕过正版验证。
2. 不修改 PCL、HMCL、BakaXL 的私有配置文件或数据库。兼容性完全依赖 .minecraft 标准目录结构，尤其是 versions/<version_id>/<version_id>.json 作为元数据入口。
3. 不执行任何下载得到的 jar/exe/脚本文件。不捆绑来源不明的“修复补丁”。
4. 下载源仅限 Mojang 官方或 BMCLAPI 镜像，不得内置盗版资源站。
5. 所有网络请求使用 HTTPS；下载先写临时文件，校验（SHA-1）通过后再原子移动到目标路径。
6. 不收集用户账号信息，不要求 Microsoft 登录。
7. **定位与免责声明**：在应用首次启动引导页及关于页面，必须显著展示以下免责与建议提示语：“⚠️ 本下载器仅作为备用方案，专注于游戏文件的纯净下载与目录构建。想要完善的启动器功能（如正版登录、模组管理、整合包安装、版本隔离等），强烈建议使用官方启动器或非中国大陆区的第三方启动器（如 Prism Launcher、MultiMC 等）。”

## 二、技术栈与项目结构

- 桌面框架：Tauri v2（Rust 后端 + WebView 前端）。
- 后端语言：Rust（stable，异步运行时 Tokio）。
- 前端：React 19 + TypeScript + Vite；UI 采用 shadcn/ui + Tailwind CSS（可选 DaisyUI 作为备选）。
- 状态管理：Zustand（轻量、适合下载队列这类客户端状态）。
- 下载引擎：优先评估 tauri-plugin-download（支持并行、断点续传、进度通知），或基于 reqwest + Tokio 自行实现 Range 下载。
- 测试框架：DeepSeek Harness（dsh）+ Vitest/Playwright（前端与 E2E）；Harness 用于编排自动化验证流程、生成测试用例、驱动 Tauri 命令通道的端到端校验。

项目目录结构建议：
src-tauri/src/
  main.rs
  commands/download.rs    // Tauri command 层
  commands/settings.rs    // 设置与路径探测
  engine/mod.rs           // 下载引擎抽象
  engine/manifest.rs      // version_manifest_v2 解析
  engine/version_json.rs  // version JSON 解析（rules/natives/assets）
  engine/verifier.rs      // SHA-1 校验
  engine/mirror.rs        // 官方/BMCLAPI 域名映射
src/
  components/DownloadQueue.tsx
  components/VersionPicker.tsx
  components/SettingsPanel.tsx
  components/Onboarding.tsx // 首次启动向导
  stores/downloadStore.ts
  stores/appStore.ts        // 全局状态（如路径配置）
  lib/tauri.ts            // invoke/listen 封装
tests/                    // Harness 与 Playwright 测试

## 三、Rust 后端与下载引擎

请实现或说明以下核心模块：

1. **Manifest 获取**：从 version_manifest_v2.json 拉取版本列表，支持官方源与 BMCLAPI 镜像切换。
2. **Version JSON 解析**：完整解析 libraries、assets、mainClass、jar 等字段；正确处理 rules（os.name / os.arch / features）与 natives 分类及解压目标目录。必须递归处理 inheritsFrom，确保 Forge/Fabric 等继承版本的兼容性。
3. **下载器**：
   - 使用 reqwest::Client 配合 Tokio 并发下载（建议并发数 10–20，可配置）。
   - 支持 HTTP Range 断点续传：中断后从 .part 文件的已下载偏移继续，而不是重新下载。
   - 流式写入时同步计算 SHA-1，下载完成后校验；校验失败删除临时文件并重试。
   - 防路径穿越：解压 natives 和写入文件时规范化路径，禁止写出 .minecraft 根目录。
4. **Tauri Command 设计**：
   - `list_versions(mirror) -> Vec<VersionSummary>`
   - `start_download(app, version_id, game_dir, mirror, concurrency) -> DownloadTaskId`
   - `cancel_download(task_id)`
   - `verify_installation(version_id, game_dir) -> VerificationReport`
   - `get_download_state(task_id) -> DownloadState`
   - `detect_game_dir(selected_path) -> Result<PathBuf, String>` // 用户选择文件夹后，自动探测 .minecraft 的准确位置
   所有 command 使用 Result<T, E> 返回结构化错误，E 实现 Serialize，前端可展示可读错误信息。
5. **状态持久化**：下载任务状态（URL、目标路径、已下载字节、总字节、SHA-1、临时文件路径）写入 Tauri app_data_dir 下的 downloads.json 或 SQLite；应用重启后可恢复未完成任务。同时持久化用户的 `.minecraft` 路径配置。

## 四、Tauri IPC 与前端 UI

1. **进度事件**：Rust 侧使用 `app.emit("download://progress", payload)` 向前端广播进度；payload 包含 taskId、stage（manifest/version_json/libraries/assets/finished）、downloadedBytes、totalBytes、currentFile、speedBps。
2. **前端订阅**：在 useEffect 中 `listen("download://progress", handler)`，组件卸载时务必取消监听，防止内存泄漏。Zustand store 维护下载队列与每个任务的实时进度。
3. **首次启动向导（Onboarding）**：
   - **强制引导**：应用启动时检查本地是否已保存有效的 `.minecraft` 路径。若没有，必须显示全屏的首次启动向导，未完成配置前禁止使用下载功能。
   - **免责声明展示**：向导第一页为欢迎页，必须醒目展示上述的免责与建议提示语。
   - **路径选择逻辑**：
     - 提供“自动探测”按钮（扫描默认路径如 `%APPDATA%/.minecraft`）。
     - 提供“选择文件夹”按钮，调用 Tauri 的文件夹选择对话框。
     - 用户可以选择 `.minecraft` 文件夹本身，也可以选择启动器所在的父文件夹（如 PCL 或 HMCL 的根目录）。后端 `detect_game_dir` 命令需要智能识别：如果是父目录，尝试向下寻找 `.minecraft`；如果是直接的 `.minecraft` 目录，直接验证其结构（如是否存在 versions 或 assets 文件夹）。
     - 成功识别后，展示路径并允许用户点击“完成”进入主界面。
4. **主界面 UI 要求**（现代化、深色模式优先）：
   - 版本选择页：搜索、类型筛选（release/snapshot/old_beta）、镜像切换。
   - 下载队列页：每个任务的进度条、速度、剩余时间、暂停/取消按钮；支持批量操作。
   - 安装校验页：一键 verify，以表格展示缺失/损坏文件。
   - 设置页：重新选择 `.minecraft` 路径（需二次确认）、并发数、镜像偏好。
   - 所有破坏性操作（取消下载、覆盖安装、更改路径）需二次确认。
5. **兼容性提示**：下载完成后，UI 明确提示用户“请使用 PCL/HMCL/BakaXL 的‘刷新版本/导入’功能识别该版本”，不要声称下载器替用户完成了启动配置。

## 五、Harness 测试方案

请设计基于 DeepSeek Harness（dsh）的测试策略，覆盖以下层级：

1. **Rust 单元测试**：rules 判断、natives 解压防穿越、SHA-1 校验失败处理、mirror 域名映射、`detect_game_dir` 路径识别逻辑。
2. **Harness 插件式集成测试**：编写一个 dsh 工具插件，调用 Tauri command 通道（`__TAURI_INTERNALS__`）或 HTTP 自动化端点，驱动 start_download → 轮询状态 → 断言 versions/libraries/assets 目录生成正确。
3. **Playwright E2E**：在真实 Tauri 窗口中，模拟首次启动（引导页流程）、选择版本、点击下载、观察进度条、取消任务、重启应用后恢复队列。
4. **Harness 测试用例生成**：利用 Harness 解析 version JSON 示例，自动生成针对不同版本（含 inheritsFrom 的 Forge/Fabric 版本）的回归用例，减少手写用例的重复劳动。
5. **验收标准**：
   - 首次打开应用，若未配置路径，强制进入引导页，且能看到免责提示。
   - 空目录下载 1.20.1 后，`.minecraft/versions/1.20.1/1.20.1.json` 存在，且 PCL/HMCL 可识别（由用户手动验证）。
   - 人为删除一个 assets 文件，verify 能准确报告缺失。
   - 下载中途强制关闭应用，重启后任务从断点继续而非归零。
   - 代码中不存在 `std::process::Command` 启动游戏、不存在绕过验证的逻辑。

## 六、输出要求

请按顺序输出：
1. 总体架构图（文字描述即可）与关键技术选型理由。
2. Rust 后端核心模块的伪代码或关键代码片段（manifest 解析、rules 判断、Range 下载循环、SHA-1 校验、路径探测）。
3. Tauri command 与 event 的完整签名设计。
4. 前端组件树与 Zustand store 核心状态形状（包括引导页的流转状态）。
5. Harness 测试方案的具体实施步骤（插件结构、测试命令、CI 集成方式）。
6. 明确列出“不实现的功能”：启动游戏、登录、正版验证、模组管理、整合包管理。
7. 如果你认为 tauri-plugin-download 不适合 MC 下载场景（例如对大量小文件的并发支持不足），请说明理由并给出自行实现 Range 下载引擎的方案。

不要省略安全校验、断点续传和兼容性说明。优先保证“下载完成后启动器能识别”以及“首次使用的路径配置体验”这两个核心验收标准。
