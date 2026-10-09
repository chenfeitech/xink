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

## 打包（macOS）

打包由 `scripts/build_macos.sh` 完成：**编译 → 生成 `.icns` 图标 → 组装 `.app` → 签名 → 打包 `.dmg` / `.pkg`**。

```bash
./scripts/build_macos.sh               # 编译 release，产出 .app + .dmg（最常用）
./scripts/build_macos.sh --pkg         # 额外产出 .pkg 安装器
./scripts/build_macos.sh --no-dmg      # 只产出 .app
./scripts/build_macos.sh --skip-build  # 跳过编译，用已有 target 产物重新打包
./scripts/build_macos.sh -h            # 查看用法
```

产物统一输出到 `build/`（版本号取自 `Cargo.toml`，无需另外维护）：

| 产物 | 说明 |
|---|---|
| `build/xink.app` | 应用 Bundle |
| `build/xink-<version>.dmg` | 拖拽安装盘（默认产出） |
| `build/xink-<version>.pkg` | 安装器（加 `--pkg`） |

可用环境变量覆盖默认值：

```bash
PROFILE=debug ./scripts/build_macos.sh --skip-build    # 用 debug 产物试跑打包流程
ICON_SRC=resources/other.png ./scripts/build_macos.sh  # 指定其它图标源
VERSION=0.2.0 ./scripts/build_macos.sh                 # 覆盖版本号
```

| 变量 | 默认值 |
|---|---|
| `APP_NAME` | `xink` |
| `BUNDLE_ID` | `com.helight.xink` |
| `VERSION` | 取自 `Cargo.toml` |
| `ICON_SRC` | `resources/AppIcon.png` |
| `PROFILE` | `release` |
| `MIN_MACOS` | `11.0` |
| `BUILD_DIR` | `./build` |

注意事项：

- 依赖 Xcode 命令行工具提供的 `iconutil` / `sips` / `hdiutil`；加 `--pkg` 时还需要 `pkgbuild`
  （见上文"构建前置条件"）。缺失时脚本会直接报错退出。
- 图标由 `sips` 生成多尺寸 `iconset` 再转 `.icns`，源图默认是 `resources/AppIcon.png`。
- 脚本会**校验图标资源确实内嵌进二进制**（在产物里检索 SVG），未内嵌则直接失败——
  否则打出的包在本机正常、拷到别的机器就图标全丢。
- 签名默认是 ad-hoc（`codesign --sign -`），仅保证本地可运行；
  分发前需用 Developer ID 重新签名（脚本结束时会打印对应命令）。

## 应用数据目录

应用自身的状态（配置、布局、标签、库列表）统一放在**应用数据目录**：

| 平台 | 路径 |
|---|---|
| macOS | `~/Library/Application Support/xink/` |
| Linux | `~/.local/share/xink/` |
| Windows | `%APPDATA%\xink\` |

目录位置由 `dirs::data_dir()` 决定（见 `src/state/workspace.rs` 的 `app_data_dir()`）。

```
~/Library/Application Support/xink/
├── config.json        # 用户配置（基本 / 编辑器 / 预览三类设置）
├── layout.json        # Dock 布局（面板位置与大小）
├── open_notes.json    # 已打开的标签（顺序 + 当前活动标签）
├── vault.json         # 上次打开的库路径（旧的单库记录）
├── vaults.json        # 库列表与当前选择（多库功能，见 docs/multi-vault-design.md）
└── xink.pid           # 单实例锁（防止重复启动）
```

各文件的作用与"删了会怎样"：

| 文件 | 用途 | 删除后 |
|---|---|---|
| `config.json` | 外观、编辑器、预览三类偏好设置 | 恢复默认设置 |
| `layout.json` | 三栏布局与面板尺寸 | 恢复默认布局 |
| `open_notes.json` | 重启后恢复标签顺序与活动标签 | 启动时不恢复历史标签 |
| `vault.json` | 上次打开的库（单库时代的记录） | 启动时不自动恢复上次库 |
| `vaults.json` | 已注册的库与当前选择（开发中） | 库列表清空 |
| `xink.pid` | 单实例锁文件 | 无影响，下次启动重建 |

要点：

- **笔记正文不在这里**。笔记始终是库目录里普通的 `.md` 文件，这里的都是可重建的派生状态。
- 删除整个目录只会丢失偏好与界面状态，**不会丢失任何笔记**。
- 文件缺失或 JSON 损坏时，应用一律回落到默认值继续启动，不会崩在启动阶段。
- 配置支持设置界面里的"导出 / 导入"；其余文件不参与导入导出。

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
