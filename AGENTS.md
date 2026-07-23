# Plot — AGENTS.md

## 项目定位
Minecraft 1.8 PotPvP 材质包修复工具：扫描 resourcepacks，按「MC 1.8 能否实际读取」五分类（正常/嵌套/文件夹/臃肿/非法），一键把可修复包重建为标准 zip。仅 Windows，Tauri 单文件 exe，Apache-2.0。

## 技术栈与结构
- Tauri 2 + React 19 + Vite 7 + TypeScript + Vitest；Rust workspace（edition 2021）
- `engine/` — 纯 Rust 域引擎（扫描/分类/处理），零 Tauri/UI 概念；集成测试 `engine/tests/`，fixture 构造器在 `tests/common/mod.rs`
- `src-tauri/` — 薄 command 层（scan/process/settings/open_plot_temp）+ WebView2 守卫
- `src/` — React 前端；`ipc.ts` 是唯一 invoke 边界（UI 测试 mock 它）；`i18n.ts` 三语目录
- `.docs/` — adr（生效中）+ archive（已完成 prd/issues）；`CONTEXT.md` — 领域词汇表

## 常用命令
- `cargo test` — 引擎+设置全部测试（主测试缝）
- `npm run test` — Vitest 前端测试（次缝，mock ipc）
- `npm run typecheck` / `npm run build` — tsc / 前端构建
- `npm run tauri dev` — 开发运行；`npm run tauri build` — 发布单 exe（target/release/plot.exe）
- `cargo run -p engine --example scan_real -- <目录>` — 真实文件夹只读扫描对账

## 常驻法则
- 域规则只进 engine：分类/修复行为一律在 `engine/` 实现并配套 fixture 测试；command 层与前端不得含判定逻辑
- 改分类/处理规则必同步三处：`CONTEXT.md` 术语、`engine/tests/` fixture、三语文案（若用户可见）
- UI 文案零硬编码，一律经 `t()`；改任一语言目录后跑 `npm run test`（键集一致性测试拦缺译）
- zip 条目名禁止直接用 `name()` 展示/落盘：走 `decode_entry_name`（UTF-8→GBK 回退）；产物文件名必过 `sanitize_windows_name`
- 处理顺序不可倒置：先移原件入 problematic_packs，再写产物（tmp → rename）；任何写入 resourcepacks 的路径必经 `unique_target`，绝不覆盖
- 扫描/处理永不触碰：顶层垃圾文件名单条目、plot_temp、exe 自身
- 新增行为先红后绿，测试只落在两条已定缝（engine 公共 API / mock ipc 组件测试）
- **改完自动编译**：功能/UI/引擎/command 等会影响可运行产物的改动，在测试通过后**主动**执行 `npm run tauri build`，不必等用户说「编译」；若 `plot.exe` 被占用先结束进程再编；纯文档/注释/仅测用例且不改运行逻辑时可跳过；用户明确说先别编则遵从

## 按需读取索引
- 涉及领域术语（五分类/拯救家族/死路径等）→ 读 `CONTEXT.md`
- 追溯许可证与 rar 路线前提 → 读 `.docs/adr/0001-apache-2-license.md`
- 追溯 v1 需求全貌 → 读 `.docs/archive/prd/0001-plot-v1.md`

## 优先级声明
用户指令 > 更近目录的 AGENTS.md > 本文件 > 被路由文档。本文件更改在新会话生效。
