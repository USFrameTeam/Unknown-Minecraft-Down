# Unknown Minecraft Down 设计文档

> Minecraft Java 版「仅下载器」—— 解析版本清单、下载文件、按标准目录放置并校验。
> v0.2：按产品决策新增 Forge 安装（执行官方安装器 processors，唯一授权例外）、
> 多目录管理、主页下载进度、Minecraft 风格 UI。

## 0. 硬约束（不可违反）

1. 绝不启动 Minecraft、绝不生成 Java 启动参数、绝不绕过正版验证。
2. 不修改 PCL / HMCL / BakaXL 的私有配置或数据库；兼容性完全依赖 `.minecraft` 标准目录结构，尤其是 `versions/<id>/<id>.json`。
3. 进程启动调用（`process::Command`）**仅允许**出现在 `engine/src/forge.rs`：执行 Forge 官方安装器的 processors（用户明确授权的 Forge 安装能力），不执行任何其他下载产物。CI 有 grep 检查强制此边界。
4. 下载源仅限 Mojang 官方、BMCLAPI 镜像、Fabric Meta（官方）、Forge 官方 maven。
5. 所有网络请求 HTTPS（非环回地址的 `http://` 一律强升）；下载先写 `<dest>.part`，SHA-1 通过后原子改名。
6. 不收集账号信息，不要求 Microsoft 登录。
7. 首次启动向导与关于页显著展示免责与建议提示语（含"国内主流启动器暂停 MC 下载启动功能，本工具作为替代方案"说明，见 `src/content/disclaimer.ts`）。

---

## 1. 总体架构与选型理由

```
┌────────────────────────── WebView（React 19 + TS + Vite）──────────────────────────┐
│  App                                                                                │
│   ├─ appStore(Zustand)      设置/引导状态（gameDir、mirror、concurrency、onboarded）│
│   ├─ downloadStore(Zustand) 任务队列实时状态（subscribe download://progress）       │
│   ├─ Onboarding（强制向导）  免责声明 → 路径探测/选择/新建 → 完成                   │
│   ├─ VersionPicker          搜索/类型筛选/镜像切换/Fabric loader 选择               │
│   ├─ DownloadQueue          进度条/速度/ETA/暂停/取消（二次确认）/批量操作         │
│   ├─ VerifyPanel            一键 verify → 缺失/损坏文件表格                         │
│   └─ SettingsPanel          换目录（二次确认）/并发数/镜像/关于+免责               │
│        │ lib/tauri.ts（invoke/listen 封装，统一错误规整）                           │
└────────────┼───────────────────────────────────────────────────────────────────────┘
             │ Tauri IPC（commands + event "download://progress"）
┌────────────▼───────────────────────── Rust（Tokio 异步）───────────────────────────┐
│  unknown-minecraft-down（应用层）                                                        │
│   ├─ commands/download.rs  list_versions / start_download / pause / resume /        │
│   │                         cancel / get_download_state / list_tasks / verify       │
│   ├─ commands/settings.rs  detect_game_dir / probe_default_dir / prepare_game_dir   │
│   ├─ state.rs              TaskEntry{cancel token, snapshot} + 事件转发 + 持久化    │
│   └─ persistence.rs        settings.json / downloads.json（tmp+rename 原子写）      │
│  mc-dl-engine（纯引擎 crate，不依赖 tauri，可独立 cargo test）                      │
│   ├─ manifest.rs     version_manifest_v2 解析                                       │
│   ├─ version_json.rs 版本 JSON / rules / natives / inheritsFrom 递归合并           │
│   ├─ plan.rs         下载计划（client jar + libraries + natives + assets 展开）     │
│   ├─ downloader.rs   Range 断点续传 + 流式 SHA-1 + 重试 + 取消                      │
│   ├─ runner.rs       任务编排（阶段/并发/取消/快照）、natives 解压（防穿越）        │
│   ├─ verifier.rs     安装校验报告                                                   │
│   ├─ game_dir.rs     .minecraft 探测 + safe_join/ensure_within 防穿越               │
│   └─ mirror.rs       Mojang ↔ BMCLAPI 域名映射 + HTTPS 强制                         │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

**选型理由**

- **Tauri v2**：体积小、Rust 后端天然适合高并发文件下载；IPC 为结构化 JSON，错误可序列化。
- **引擎独立成 crate（mc-dl-engine）**：不依赖 tauri/GTK，`cargo test` 无需桌面环境即可跑全部下载/解析/防穿越测试；应用层只做粘合（注册任务、转发事件、持久化）。
- **reqwest(rustls) + Tokio 自研下载器**：MC 下载是"海量小文件（几千个 assets）+ 少量大文件"混合负载，需要 per-file 的断点续传、流式哈希与精细取消，通用下载插件难以满足（见 §7）。rustls 避免 OpenSSL 系统依赖。
- **Zustand**：下载队列是典型客户端状态，store 订阅事件即可，无需 Redux 样板。
- **Tailwind v4 + shadcn 风格手写组件**：组件直接复制进仓库（shadcn 的本意），不引入 radix 运行时依赖，构建确定性强。
- **JSON 持久化（settings.json / downloads.json）**：数据量小、人工可读可修；SQLite 在此规模属于过度设计。

---

## 2. Rust 后端核心模块（关键代码摘要）

### 2.1 manifest 解析（`engine/src/manifest.rs`）

```rust
pub async fn fetch_manifest(client: &Client, mirror: Mirror) -> Result<Manifest> {
    let resp = client.get(mirror.manifest_url()).send().await?.error_for_status()?;
    Ok(resp.json::<Manifest>().await?)   // { latest, versions: [{ id, type, url, releaseTime, sha1 }] }
}
```

### 2.2 rules 判断（`engine/src/version_json.rs`）

```rust
/// 官方启动器语义：无 rules → 允许；有 rules → 默认拒绝，最后一条命中的规则决定结果
pub fn evaluate_rules(rules: Option<&[Rule]>, ctx: &OsContext) -> bool {
    let Some(rules) = rules else { return true };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, ctx) { allowed = rule.action == "allow"; }
    }
    allowed
}
// OsContext.detect(): os = windows/linux/osx; arch = x86/x64/arm64; features 恒为空
```

### 2.3 inheritsFrom 递归合并（`engine/src/version_json.rs::resolve_version`）

- 链解析：入口 JSON → `inheritsFrom` → 本地 `versions/<pid>/<pid>.json` 优先，缺失才走 manifest（`ManifestHandle` 惰性拉取，本地齐全时**完全离线**）；环检测 + 深度上限 8。
- 合并：libraries 父+子、同名 maven name 子版本覆盖；`mainClass`/`assetIndex` 子优先；客户端 jar 归属 = 链上最后的显式 `jar` 字段，缺省为根版本 id。
- 链上所有版本 JSON **原样落盘**到各自 `versions/<id>/<id>.json` —— PCL/HMCL 靠它们自己解析继承，这是"下载完即被识别"的关键。

### 2.4 Range 断点续传 + SHA-1（`engine/src/downloader.rs`）

```rust
// 单次尝试 try_once：
//   part 存在 → Range: bytes={part_len}- ; 206 → 用 part 前缀字节做 SHA-1 种子后追加写
//                              200 → 服务器忽略 Range，截断重写
//                              416 → part 恰好完整(尺寸吻合)则直接校验改名，否则删除重下
//   流式写入：hasher.update(chunk) 与 file.write 同步进行
// download_file 外层：最多 3 次重试；SHA-1 不匹配 → 删 part 重下；网络失败/取消 → 保留 part（断点）
//   校验通过 → fs::rename(part → dest) 原子落位
```

### 2.5 路径防穿越（`engine/src/game_dir.rs` + `plan.rs` + `runner.rs`）

- `safe_join(root, rel)`：拒绝 `..`、绝对路径、反斜杠、空串（components 逐项校验）。
- `PathGuard::ensure_within`：最深已存在祖先 canonicalize + 剩余尾部拼接后 `starts_with(root)`，可拦截**已存在符号链接**指向 root 外的情况；祖先结果带缓存（数千 assets 不产生重复 syscall）。
- natives 解压：仅 `.dll/.so/.dylib/.jnilib` 白名单 + `extract.exclude` + **只取 zip entry 的文件名部分**（天然丢弃目录成分与 `..`）+ ensure_within 兜底。
- 资产路径：hash 必须为 40 位十六进制；legacy 索引虚拟拷贝的资源名校验后才能进入 `assets/virtual/<id>/`。

### 2.6 路径探测（`engine/src/game_dir.rs::detect_game_dir`）

```text
选中目录本身像 .minecraft（有 versions/|assets/|libraries/）→ 直接用
否则 selected/.minecraft 像 → 用它
否则扫描一层子目录：子目录本身像，或 子目录/.minecraft 像
  → 恰好 1 个候选：用它；0 个：报错并提示可选空文件夹新建；多个：报歧义并列出
```

另提供 `probe_default_dir`（%APPDATA%/.minecraft 或 $HOME/.minecraft）与 `prepare_game_dir`（空文件夹初始化骨架）。

### 2.7 Forge 安装管线（`engine/src/forge.rs`，v0.2 新增）

按官方安装器（真实 1.20.1-47.3.0 与 1.12.2-14.23.5.2860 安装器实测逆向）复刻：

```text
BMCLAPI 构建列表（含安装器 SHA-1）
  → 下载官方安装器 jar（镜像可切，哈希校验）
  → 解析 install_profile.json（spec 0/1）+ 内嵌 version.json
  → version.json 落盘 versions/<id>/<id>.json，进入常规继承链流程
  → profile.libraries 并入下载计划（无 url 旧式库默认 Forge maven）
  → Forge 阶段：
      launcher_profiles.json 桩文件（官方安装器的启动器标记要求）
      提取安装器内嵌 maven/* 产物（≤1.12.2 的 universal 只内嵌、url 为空）
      [1.13+] 定位 Java（设置项 > JAVA_HOME > PATH > 常见目录）
        逐个执行 client 侧 processors（MCP_DATA / DOWNLOAD_MOJMAPS /
        MERGE_MAPPING / jarsplitter / FART / binarypatcher）
        - 参数解析：{TOKEN}（内置 MINECRAFT/ROOT/SIDE/INSTALLER/
          LIBRARY_DIR/MINECRAFT_VERSION/MINECRAFT_JAR + data 表）、
          [g:a:v[:classifier[@ext]]]（maven 路径）、'literal'、
          /安装器内路径（先解压为真实文件）
        - 每步以 outputs 的 SHA-1 模板校验产物
      [≤1.12.2] 无 processors，纯下载完成
```

**验证方式**：`tests/forge_pipeline.rs`（`#[ignore]`，需网络+Java）对真实的
1.20.1-47.3.0 完整执行上述管线，断言补丁客户端
`forge-1.20.1-47.3.0-client.jar` 的 SHA-1 与 install_profile 声明的
`PATCHED_SHA` 一致 —— 即与官方安装器产物逐字节一致。本仓库开发机上已实际通过。

**网络要求**：processors 中 `DOWNLOAD_MOJMAPS` 直接访问 Mojang 官方端点
（piston-meta / piston-data），Forge 1.13+ 安装期间需要二者可达（与镜像偏好无关）。

**安全边界**：Java 执行仅发生在 forge.rs、仅运行官方安装器声明/自带的
processor jar，取消令牌可随时终止（kill_on_drop）；CI 以 grep 强制
`process::Command` 不出现在其他任何源文件。

---

## 3. Tauri Command 与 Event 完整签名

```rust
// commands/download.rs
list_versions(mirror: Mirror) -> Result<Vec<VersionSummary>, AppError>
list_fabric_loaders(mc_version: String) -> Result<Vec<FabricLoaderSummary>, AppError>
list_forge_builds(mc_version: String) -> Result<Vec<ForgeBuildSummary>, AppError>
start_download(app, state, source: VersionSource, game_dir: String,
               mirror: Mirror, concurrency: u32,
               java_path: Option<String>) -> Result<String /*taskId*/, AppError>
cancel_download(state, task_id: String) -> Result<(), AppError>     // 终止（保留 .part）
pause_download(state, task_id: String) -> Result<(), AppError>      // 引擎 Canceled → 映射为 Paused
resume_download(app, state, task_id: String) -> Result<(), AppError> // 断点续跑
get_download_state(state, task_id: String) -> Result<TaskSnapshot, AppError>
list_download_tasks(state) -> Result<Vec<TaskSnapshot>, AppError>
clear_finished_tasks(state) -> Result<(), AppError>
verify_installation(version_id: String, game_dir: String, mirror: Mirror)
    -> Result<VerificationReport, AppError>

// commands/settings.rs
detect_game_dir(selected_path: String) -> Result<String, AppError>
probe_default_dir() -> Result<Option<String>, AppError>
prepare_game_dir(path: String) -> Result<String, AppError>   // 空目录 → versions/assets/libraries
get_settings(state) -> Result<AppSettings, AppError>
save_settings(state, settings: AppSettings) -> Result<(), AppError>  // normalize + 逐目录存在性校验
detect_java() -> Result<Option<String>, AppError>            // 仅用于 Forge processors
```

```ts
// 关键 DTO（serde camelCase，与 src/types.ts 一一对应）
type VersionSource =
  | { id: string }                                    // 原版
  | { mcVersion: string; loaderVersion: string }      // Fabric
  | { mcVersion: string; forgeVersion: string; branch?: string | null };  // Forge
interface TaskSnapshot { taskId; versionId; gameDir; mirror; status: TaskStatus;
  stage?: "manifest"|"version_json"|"libraries"|"assets"|"forge"|"verifying"|"finished";
  downloadedBytes; totalBytes; speedBps; currentFile?; filesDone; filesTotal; error?; createdAt; finishedAt? }
interface AppSettings { onboarded; gameDirs: string[]; activeGameDir?: string|null;
  mirror; concurrency; javaPath?: string|null }   // 旧 gameDir 单目录字段读取时自动迁移
interface VerificationReport { versionId; gameDir; ok; checkedFiles; missing: ReportEntry[]; corrupted: ReportEntry[]; totalBytes }
```

**事件**：`app.emit("download://progress", TaskSnapshot)` —— 阶段切换与终态即时发送，进度每 500ms 一帧（速度做指数平滑）。前端 `listen("download://progress")`，组件卸载时 `unlisten()`。

**结构化错误**：`AppError { code, message }`，code ∈ network/io/json/version_not_found/invalid_url/sha1_mismatch/path_traversal/invalid_game_dir/canceled/task_not_found/internal，前端直接展示 message。

**持久化**：`app_data_dir/settings.json`（onboarded/gameDir/mirror/concurrency）与 `downloads.json`（任务记录：source/game_dir/mirror/status/进度，节流 1s 写盘 + 终态立即写）。启动时恢复：未结束任务重建为 Paused，原本运行/排队的**自动续跑**——引擎按磁盘现状推进（已校验文件秒过、`.part` 断点续传），进度不归零。

---

## 4. 前端组件树与 Zustand 状态形状

```
App
 ├─ ready? ── 否 → 加载页
 ├─ !onboarded ── Onboarding（全屏强制向导，未完成禁止进入主界面）
 │    step: "welcome"（免责声明 + 勾选确认）→ "path"（自动探测/选择文件夹/新建）
 │    → addGameDir + save({onboarded:true}) → 进入主界面
 └─ Layout(page: home|versions|queue|verify|settings)
      ├─ HomePanel      主页：聚合进度条+速度+ETA、每任务迷你进度（事件驱动）、
      │                  最近记录、免责声明；常驻底部状态栏（任何页面可见）
      ├─ VersionPicker  原版(搜索/类型筛选) / Fabric / Forge 选项卡
      │                  + DirSelector（多目录切换）+ MirrorSwitch
      ├─ DownloadQueue  TaskCard[]（进度/速度/ETA/暂停/取消二次确认/批量）
      ├─ VerifyPanel    版本输入 → 报告表格（缺失/损坏）
      └─ SettingsPanel  多目录管理（添加/移除/切换，均二次确认）、并发数、
                        Java 路径（Forge 用，自动探测）、镜像、关于+免责
```

```ts
// appStore.ts（多目录）
{ ready, onboarded, gameDirs: string[], activeGameDir: string|null,
  mirror, concurrency, javaPath: string|null,
  load()/save(patch)/setLocal(patch),
  addGameDir(dir)   // 去重；首个目录自动激活
  removeGameDir(dir)// 拒绝移除最后一个；移除激活目录时顺延
  setActiveGameDir(dir) }

// downloadStore.ts
{ tasks: Record<taskId, TaskSnapshot>, order: taskId[], lastError,
  refresh(): list_download_tasks（启动恢复）,
  applySnapshot(snap): 事件入口（新任务入 order，其余原位更新）,
  startTask(source, displayId, gameDir, mirror, concurrency, javaPath?) -> taskId,
  pauseTask/cancelTask/resumeTask/clearFinished }
```

**UI 风格（Minecraft 主题，v0.2）**：方角 + 2px 黑描边 + 内嵌斜面（bevel 阴影）的
石质面板（`.mc-panel`）、经典灰石按钮与绿色主按钮（`.mc-btn`）、XP 风格绿色条纹
进度条（`.mc-progress`）、像素字体 Press Start 2P（标题/数字，中文回退系统字体）、
黄字 splash 动画。全部以 CSS 实现（无图片资源），组件经语义类统一换肤。

向导流转状态机：`ready=false → 加载设置 → onboarded? 主界面(主页) : Onboarding(welcome → path → 完成)`。`welcome` 页必须勾选"已阅读免责声明"才能进入下一步（`onboarding-next` disabled）。

---

## 5. 测试方案（分层）

> 原方案中的 DeepSeek Harness（dsh）层已按需求删除；以下为等效覆盖的常规分层测试。

| 层级 | 工具 | 覆盖 | 运行 |
|---|---|---|---|
| 引擎单元/集成 | `cargo test -p mc-dl-engine`（48 例） | rules 语义（last-match-wins/os/arch/features）、natives 分类器 `${arch}`、maven 坐标（含 `@zip`/`classifier@ext`）、mirror 映射（含 Forge maven）与 HTTPS 强升、`safe_join`/`ensure_within`（含**符号链接逃逸**）、`detect_game_dir` 四种形态、`inheritsFrom` 离线合并（Fabric 风格夹具 + 环检测）、**Range 断点续传**（本地 HTTP 服务器模拟断流 → 断言 `Range: bytes=N-` 重发 + 整文件 SHA-1 一致）、SHA-1 失败重试与清理、取消、已存在文件跳过、natives 解压防穿越、`verify_installation` 缺失/损坏报告、Forge profile 解析/token 解析/URL 构造 | `npm run test:rust` 或 `cargo test --workspace` |
| Forge 真实管线 | `cargo test --test forge_pipeline -- --ignored`（需网络+Java） | 真实 1.20.1-47.3.0：安装器下载校验 → profile 库下载 → 内嵌提取 → processors 全程执行 → **补丁产物 SHA-1 与官方 PATCHED_SHA 一致** | 本地手动 |
| 前端单元 | Vitest（14 例） | 格式化工具、appStore 多目录读写/切换/移除顺延、downloadStore 队列归并/批量命令/javaPath 传参 | `npm test` |
| 前端 E2E | Playwright + `__TAURI_INTERNALS__` mock（7 场景） | 强制向导与免责声明（含替代方案文案）、向导完成 → 主页、**主页进度面板+底部状态栏（事件驱动）**、原版下载 → 队列 → 完成 → 兼容性提示、**Forge 选项卡发起（javaPath 透传）**、**多目录切换联动**、取消二次确认、校验页缺失表格 | `npm run test:e2e` |
| 真机端到端（人工/CI-可选） | `tauri-driver`（WebDriver） | 真实窗口中的完整下载（空目录 1.20.1 → 目录结构断言）、强杀进程 → 重启续传 | 见下 |

**tauri-driver 集成要点**（可选，CI 需 Linux + WebKitGTK）：
1. `cargo install tauri-driver`；
2. 以 WebDriver 模式启动应用（Tauri v2 内置 `--webdriver` 支持）；
3. 用例中直接执行 JS 调 `window.__TAURI_INTERNALS__.invoke("start_download", …)` 驱动真实后端，轮询 `get_download_state`，然后从测试侧断言 `versions/1.20.1/1.20.1.json`、`versions/1.20.1/1.20.1.jar`、`libraries/`、`assets/objects/…` 存在且 SHA-1 命中 —— 与 Rust 集成测试互为印证。

**验收标准对照**：

| 验收项 | 覆盖 |
|---|---|
| 未配置路径强制进入引导页且可见免责提示 | E2E `未完成向导时强制引导` |
| 空目录下载 1.20.1 后 `versions/1.20.1/1.20.1.json` 存在、启动器可识别 | 引擎测试（目录布局断言）+ 真机用例；PCL/HMCL 识别由用户手动验证 |
| 删除一个 asset 后 verify 准确报告缺失 | Rust `verify_reports_missing_and_corrupted_accurately` |
| 下载中途强杀 → 重启从断点继续而非归零 | Rust `resume_continues_from_part_and_sends_range`（保留 `.part` + Range 续传断言）+ 启动恢复逻辑 `bootstrap_from_disk` |
| 代码中不存在启动游戏/绕过验证逻辑 | 全仓无 `process::Command`（`grep` CI 检查） |

**CI 集成**：见 `.github/workflows/ci.yml` —— Rust fmt/clippy/test + 前端 tsc/vitest/build + Playwright（mock 层）+ `process::Command` 禁用扫描。

---

## 6. 明确不实现的功能

- ❌ 启动游戏 / 生成 Java 启动参数 / JVM 调优
- ❌ 正版登录（Microsoft OAuth）、离线登录、任何验证绕过
- ❌ 模组管理（mods 目录管理、mod 下载）
- ❌ 整合包管理（CurseForge/Modrinth 整合包解析安装）
- ❌ 版本隔离、内存/分辨率等启动配置
- ❌ NeoForge（结构同 Forge，可后续按同一管线扩展）
- ❌ 修改任何第三方启动器的私有配置/数据库
- ❌ 内置 Mojang 官方、BMCLAPI、Fabric Meta、Forge maven 之外的任何下载源

> Forge 安装原属此清单（提示词约束），v0.2 起按产品决策移出并实现：
> 以官方安装器 + 其 processors 完成，执行范围严格限定于 forge.rs（见 §2.7）。

---

## 7. 为什么不直接用 tauri-plugin-download

1. **海量小文件并发**：一个 1.20.1 资产索引约 3000–4000 个小文件。需要"计划 → 并发信号量 → 每文件独立重试/跳过 → 汇总进度"的编排层；插件的任务模型是"一个 URL 一个任务"，几千个任务的通知与恢复语义都不合适。
2. **断点续传语义**：插件支持断点续传，但 MC 需要 per-file 的 `.part` 保留策略（网络失败保留、SHA-1 失败删除重下）、续传时的**整文件哈希种子**（206 追加前先哈希 part 前缀，保证最终 SHA-1 覆盖全文件）——这些细节插件无法表达。
3. **两阶段计划**：assets 任务列表依赖先下载 asset index 才能展开；版本 JSON 需要递归 `inheritsFrom`。这是"解析驱动的动态计划"，不是静态 URL 列表。
4. **取消/暂停粒度**：需要任务级 CancellationToken 贯穿到每个 chunk 的 `select!`，且取消后状态要能映射为 Paused 供重启恢复。
5. **镜像映射与安全**：BMCLAPI 域名重写、HTTPS 强升、路径防穿越需要在每个请求前介入。

自研 Range 引擎（本文 §2.3/2.4）总量约 400 行核心代码，换来上述全部控制点，且以 39 个测试锁定行为——收益明显大于引入插件后的对抗成本。插件的进度通知（event）方案值得借鉴：我们采用同样的 `download://progress` 单事件广播模型。
