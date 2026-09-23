# xink

基于 [GPUI](https://gpui.rs) 的本地优先 Markdown 笔记桌面应用，提供 Obsidian 的基础能力：
**目录管理**、**Markdown 编辑**、**实时预览**。

笔记就是磁盘上普通的 `.md` 文件——不需要账号，不上传云端，不转换格式。

- 需求规格：[`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)
- 技术设计：[`docs/DESIGN.md`](docs/DESIGN.md)

## 技术栈

| | |
|---|---|
| 语言 | Rust 1.98+（edition 2024） |
| UI 框架 | [`gpui-kit`](https://github.com/longbridge/gpui-kit) 0.6（内含 GPUI） |
| 渲染 | GPU 加速（macOS: Metal） |
| 平台 | macOS 优先，架构上保持 Windows / Linux 可编译 |

## 构建前置条件

**macOS**（GPUI 使用 Metal，需要完整 Xcode）：

1. 从 App Store 或 [Apple Developer](https://developer.apple.com/download/all/) 安装 **Xcode**
2. 首次启动 Xcode，安装 macOS 组件（默认选项）
3. 安装命令行工具并指向新 Xcode：

```bash
xcode-select --install
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
```

> 当前环境只装了 Command Line Tools，**未安装完整 Xcode.app**，
> 这是 M0 环境验证阶段需要首先确认的风险项。

## 快速开始

```bash
cargo run                 # 运行
cargo test                # 运行领域层测试
cargo clippy --all-targets
```

首次编译会拉取并构建 blade-graphics / tree-sitter / resvg 等重依赖，耗时较长，属正常现象。

## 开发约定

项目级 AI 开发规则与技能位于 `.codebuddy/`：

```
.codebuddy/rules/    开发铁律（技术栈、分层、编码规范、领域规则）
.codebuddy/skills/   开发技能（GPUI 桌面开发、本应用开发 SOP）
```

开工前请先阅读 [`.codebuddy/rules/`](.codebuddy/rules/)，其中有 5 条会导致编译失败的硬约束。

## 当前进度

| 里程碑 | 状态 |
|---|---|
| M0 环境验证 | 完成 |
| M1 骨架布局 | 完成 |
| M2 目录管理 | 完成 |
| M3 编辑与保存 | 完成 |
| M4 预览 | 完成 |
| M5 持久化 | 完成 |

**P0 已全部完成**。

启动后点击左侧「打开库」或 `Cmd+O` 选择任意文件夹。可先用示例库 `~/ink-demo`
（演示数据，可随时 `rm -rf ~/ink-demo`）。

常用快捷键：

| 快捷键 | 作用 |
| --- | --- |
| `Cmd+O` | 打开文件夹作为库 |
| `Cmd+N` | 新建笔记 |
| `Cmd+S` | 立即保存当前笔记 |

示例库里的 `图片测试.md` 可用于快速验证：本地相对路径图片、代码块语法高亮、表格渲染。

## 许可

待定。
