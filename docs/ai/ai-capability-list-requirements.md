# 模型能力清单：获取、展示与说明 — 需求规格

> 状态：**G1 已过**（2026-10-29 用户拍板，见 §6）
> 关联：[`multi-model-architecture-design.md`](./multi-model-architecture-design.md)（§G7 缺口：能力探测只有 tools）、
> [`ai-m1-completion-design.md`](./ai-m1-completion-design.md)
> 落地位置（预计）：`src/core/ai/capability.rs`（新）、`src/core/config.rs`、`src/ui/settings.rs`
> 分支：`feature/ai-capability-list`

---

## 1. 背景与目标

### 1.1 现状（全部附证据）

用户要在配置模型时看到它支持哪些能力（think / tools 等）以及每项是什么意思。
实测**两端的起点完全不对称**：

| | Ollama | OpenAI 兼容（DeepSeek 等） |
|---|---|---|
| 能力来源 | `/api/tags` 顶层 `capabilities` 数组（`models.rs:56-60`） | **无，恒为空** |
| 模型来源 | 自动发现 / 手填 | **只能手填** id + base_url（`settings.rs:2456-2461` 直接 `..Default::default()`） |
| 界面展示 | 原样拼接英文字符串（`settings.rs:1525-1549`） | 一行能力都看不到 |

具体问题：

| # | 问题 | 证据 |
|---|---|---|
| P1 | 能力项是**自由 `Vec<String>`**，没有枚举、没有注册表、没有说明文案，约定值只活在注释里 | `config.rs:632-633`（`/// completion / tools / thinking / vision`）、`models.rs:23` |
| P2 | UI **原样拼接英文原始字符串**：`completion / vision / tools / thinking`，除"工具调用"外连中文标签都没有 | `settings.rs:1525-1549` `capability_line()`；单测锁定了这个文案 `tests/ui/settings.rs:280-306` |
| P3 | OpenAI 兼容侧**完全没有能力来源**，也没有 `/v1/models` 拉取（各家路径不一，当初就排除了） | `ai-m1-completion-design.md:238`；全仓搜 `v1/models` 无结果 |
| P4 | 探针**只测 tools**，不测 thinking / vision；`tools_enabled` 才是权威判据，`capabilities` 只是厂商声明 | `probe.rs:1-10`（模块头）；`ui/ai.rs:861` 只看 `tools_enabled` |
| P5 | `supports_tools()` / `supports_thinking()` 是 `#[allow(dead_code)]` 死代码 | `config.rs:913-922` |
| P6 | 能力声明**不可信**：本机 `qwen3.5:0.8b` 在 `/api/tags` 里声明支持 tools，真发工具过去返回 500 | `probe.rs:1-4` 模块头实测记录 |
| P7 | `think` 只对 Ollama 生效（`ollama.rs:223` 有 `"think"` 字段），OpenAI 侧 `build_body` 根本没有；界面**没有思考开关** | `openai.rs:218-235`；`settings.rs:1594-1602` 的 `ModelParam` 只有三项 |

### 1.2 目标

| # | 目标 | 衡量 |
|---|---|---|
| G1 | 每类模型都能看到能力清单 | Ollama 走 `/api/tags`，OpenAI 兼容走静态表；两边都不再有"一片空白" |
| G2 | 每项能力有**中文标签 + 含义与用途说明** | 鼠标悬停或展开能看到这句能力是干什么的、开了会怎样 |
| G3 | 覆盖不到的模型**明确说"未知"**，而不是沉默 | 静态表没匹配上时显示"未知"，并提示可手动指定 |

---

## 2. 非目标

- **不改变任何运行时行为**（用户拍板 Q2：只展示说明）——`think` 仍只对 Ollama 生效且**不做开关**；agent loop 仍只看 `tools_enabled`
- **不扩展探针**（Q3）——thinking / vision 不做实测，`probe.rs` 保持只测 tools
- **不新增 `/v1/models` 拉取**（各家路径不一，且返回里也没有能力字段）
- **不清理 `supports_tools()` / `supports_thinking()` 死代码**（本次不碰行为层，留着无害）
- **不做 vision 相关功能**（`multi-model-architecture-design.md:141` 已定为非目标）
- **不新增依赖**

---

## 3. 功能模块

| # | 需求 | 说明 |
|---|---|---|
| R-1 | **能力字典** | 一张 `能力 id →（中文标签、含义、用途）` 的映射表，覆盖 Ollama 的 4 项（completion / tools / thinking / vision）+ OpenAI 侧会用到的项。未知 id 回落为"未知能力"，**不丢弃** |
| R-2 | **Ollama 侧展示** | 沿用 `/api/tags` 的 `capabilities`，但改展示中文标签，并能看到每项说明 |
| R-3 | **OpenAI 兼容侧静态表** | 按 **base_url 域名 + 模型名**匹配内置表，给默认能力（如 `deepseek-reasoner` → thinking，`deepseek-chat` → tools）；匹配不上显示"未知" |
| R-4 | **可手动增删** | 界面上能增删能力项，覆盖静态表的推断。手动改过之后**不再被静态表覆盖** |
| R-5 | **"未知"可见** | 覆盖不到时明确显示"未知"，并提示可以手动指定，而不是留白 |

---

## 4. 边界与约束

逐条过边界九项：

| # | 边界项 | 判定 | 处理 |
|---|---|---|---|
| 1 | 空输入 | **适用** | `capabilities` 为空（OpenAI 侧默认、静态表未命中）→ 显示"未知"而不是留白；空模型名 / 空 base_url 不匹配任何规则 |
| 2 | 超大输入 | **适用** | 手动加入很多能力项时列表要能换行或滚动，不把模型行撑爆 |
| 3 | 非法路径 | 不适用 | 能力不涉及路径 |
| 4 | 并发写 | 不适用 | 能力改写走既有 `update_config`，不新增写盘时机 |
| 5 | 外部程序改文件 | 不适用 | — |
| 6 | 崩溃后半截文件 | **适用** | `capabilities: Vec<String>` 有 `#[serde(default)]`，半截 JSON 时整个配置加载失败降级为空（既有行为），不 panic |
| 7 | 多库切换 | 不适用 | 模型配置是全局的，不随库变 |
| 8 | 隐藏与忽略文件 | 不适用 | — |
| 9 | 非 UTF-8 | **适用** | `/api/tags` 返回的非法字符串：JSON 解析层已保证是合法 `String`；**未知取值**不能让 UI panic，一律按"未知能力"渲染 |

补充约束：

| # | 约束 | 理由 |
|---|---|---|
| C1 | 能力项保持**自由字符串**，不做枚举 | 厂商会造新词（如 `embedding`）；枚举会让未知值在反序列化时丢失。字典查不到就回落"未知"，值本身原样保留 |
| C2 | 静态表判定必须是**纯函数**，可单测 | `fn infer(kind, base_url, model_id) -> Vec<String>`，不碰网络、不碰全局状态 |
| C3 | 手动改过的能力项要能**持久化并优先于静态表** | 静态表只是默认值（Q1）；用户改了就是权威。需要一个"用户是否改过"的标记字段 |
| C4 | `capability_line()` 的现有测试断言要同步更新 | `tests/ui/settings.rs:280-306` 锁定了 `"chat / tools · 上下文 128k · 工具调用待实测"` 这类文案 |
| C5 | 不在 `tests/` 根新增文件 | `Cargo.toml:13` `autotests = false`，靠 `#[path]` 挂载 |
| C6 | 说明文案与静态表放**同一处**，不要散在 UI 里 | 否则加一个能力要改两个地方，必然漂移 |

---

## 5. 验收标准

| # | 类型 | 标准 | 关联 |
|---|---|---|---|
| AC-1 | 自动 | `infer()`：DeepSeek 的 `deepseek-reasoner` → 含 `thinking`；`deepseek-chat` → 含 `tools`；未知域名 + 未知模型名 → 空 | R-3 |
| AC-2 | 自动 | 字典：已知 id 给中文标签与说明；未知 id 回落"未知能力"且不 panic；空字符串也安全 | R-1 |
| AC-3 | 自动 | 手动改过的项优先于静态表推断（有标记时不再被覆盖） | R-4 |
| AC-4 | 自动 | 既有测试全绿（尤其 `tests/ui/settings.rs` 的 `capability_line` 断言要按新文案更新） | C4 |
| AC-5 | 自动 | `cargo build` / `clippy --all-targets` 0 warning、`fmt --check` 干净 | G4 |
| AC-6 | 人工 | 配置 DeepSeek 模型后，模型行下能看到能力清单（不再是空白） | R-3 / G1 |
| AC-7 | 人工 | 每项能力有中文标签，能看到（悬停或展开）含义与用途说明 | R-2 |
| AC-8 | 人工 | Ollama 模型同样显示中文能力清单，不再出现 `completion / vision / tools` 这种原始串 | R-2 |
| AC-9 | 人工 | 手动增删能力后，重启应用仍在 | R-4 |
| AC-10 | 人工 | 匹配不到时显示"未知"并提示可手动指定，不是留白 | R-5 |
| AC-11 | 人工 | **行为未变**：agent loop 仍只看 `tools_enabled`；给 DeepSeek 标了 tools 但没跑连接测试，不会启用工具阶段 | §2 非目标 |

---

## 6. 决策记录

| # | 问题 | 结论 | 日期 |
|---|---|---|---|
| Q1 | OpenAI 兼容侧能力从哪来 | **静态表 + 可手动改**：内置主流厂商 / 模型的能力表给默认值，界面上允许增删；覆盖不到的显示"未知" | 2026-10-29 |
| Q2 | 是否要让能力"可选用"（开关） | **只展示说明，不改行为**：`think` 仍只对 Ollama 生效且不做开关。最小改动 | 2026-10-29 |
| Q3 | Ollama 侧是否扩展探针校正 | **不动**：tools 已有实测（`tools_enabled`），其余能力只用厂商声明 + 说明文案 | 2026-10-29 |

**静态表为什么可信度够**（回应 P6）：能力声明本来就不可信，所以静态表给的只是**默认值**，
用户可以手动改（R-4）；而运行时权威判据仍是探针实测的 `tools_enabled`，
标了 tools 却没实测通过的模型**不会**启用工具阶段（AC-11）。

待用户拍板清单：**已清空**。
