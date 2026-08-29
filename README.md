# Plot

**Minecraft 1.8 PotPvP 材质包修复工具**

[English](README_en.md)

## 这是什么

Minecraft 1.8 只认「zip **根层**直接就是 `assets` + `pack.mcmeta`」的材质包。社区分享的包经常：

- 外面又套了一层文件夹或 zip  
- 扩展名写错（`.ZIP`、假 `.rar`、无后缀）  
- 核心文件大小写 / 手滑写错  
- `pack.mcmeta` 里带了 **Lunar Client 读不了的非法转义**

游戏往往**静默忽略**，玩家只觉得「装了没效果」。Plot 扫描 `resourcepacks`，按能否被 MC 1.8 实际读取分类，并一键把可修的包重建成标准 zip。

- **仅 Windows** · 单文件 `plot.exe` · [Apache-2.0](LICENSE)  
- 依赖 WebView2（Win10/11 通常自带；缺失时会提示安装）

## 功能

| 分组 | 含义（简述） |
|------|----------------|
| **非法**（红） | 无法识别 / 打不开 / 真 RAR·7z 等；隔离到 `plot_temp/illegal_packs` |
| **嵌套**（橙） | 套壳、扩展名错、`pack.mcmeta` 大小写/typo 等；可自动解包重建 |
| **Lunar非法**（冰青） | `pack.mcmeta` 含 Lunar 非法 JSON 转义；最小修补，保留颜色与原文 |
| **文件夹**（黄） | 文件夹形态的合法包；压成标准 zip |
| **臃肿**（紫） | MC 能读，但有多余根层条目 / 死路径等；瘦身重建 |
| **正常**（绿） | 已符合目标形态；处理时不动 |

其它要点：

- **合集包**可拆成多个独立包；嵌套产物优先用**内层作者原名**  
- **原件不删**：处理前移入 exe 旁 `plot_temp/problematic_packs`  
- **绝不覆盖**：产物 / 隔离区重名时自动加 ` (1)`、` (2)`…  
- **锁定预检**：确认前探测被 MC 占用的包，跳过而非强改  
- **检查更新**：启动时静默探测 GitHub Release；页脚可手动检测（有更新 / 已最新 / 连接失败）  
- **三语界面**：简体中文 / 繁體中文 / English  

## 使用

1. 运行 `plot.exe`（默认扫描 `%APPDATA%\.minecraft\resourcepacks`，可改路径）  
2. 看六色概览与列表；可按分组筛选  
3. 点「处理」→ 核对清单（含被占用警告）→ 确认  
4. 完成后看结果；确认无误后再自行清理 `plot_temp`  

## v1 限制

- 只解析 **zip**（魔数 `PK`）。**RAR / 7z 不支持**，会标为非法并提示手动解压后重扫  
- 仅 Windows  

## 开发

```bash
npm install
npm run tauri dev      # 开发运行
npm run test           # 前端测试（Vitest，mock IPC）
cargo test             # 引擎 + 设置测试
npm run tauri build    # → .target/cargo/release/plot.exe
```

| 目录 | 职责 |
|------|------|
| `engine/` | 纯 Rust：扫描 / 分类 / 处理（无 UI） |
| `src-tauri/` | 薄 Tauri command + WebView2 守卫 |
| `src/` | React 前端；`ipc.ts` 为唯一 invoke 边界 |

领域术语见 [`CONTEXT.md`](CONTEXT.md)。

## 许可证

[Apache License 2.0](LICENSE)
