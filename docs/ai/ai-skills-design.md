# 设计：M2 —— Agent Skills（`SKILL.md`）

> 需求：`docs/ai/ai-assistant-requirements.md` A5.1–A5.7、A9.1（命令面板 `AI:` 前缀）。
> 分期：`docs/ai/ai-assistant-design.md` §3.1（`skills.rs` 标 M2）、决策 Q7（Skills M2 做，xink 自己发现并注入）。
>
> **状态：✅ 已实施（2026-10-26，待人工验收）**。S1–S5 全部落地，482 项单测全绿。
> 实施与本文的两处偏差记录在 §3.5 与 §11。
> 前置已完成：M1 的 agent loop / 只读工具 / 结构校验；`.xink` 不进索引
> （`core/media.rs::ai_excluded` + `core/index.rs::collect_markdown` 守卫）与不进文件树
> （`core/vault.rs::should_ignore`）。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| `ai.skills_enabled` 是**死开关**：存得下、导出得出，但没有任何消费点 | `src/core/config.rs:571`；设置界面自己标着"M2 实现：当前仅登记不生效"（`src/ui/settings.rs:986` 附近，白名单断言在 `tests/ui/settings.rs`） |
| `ai.disabled_skills: Vec<String>` 字段已在，同样无消费点 | `src/core/config.rs:572` |
| 命令面板的 AI 候选是**固定枚举** `Action`，装不下运行时发现的 skill | `src/ui/command.rs:335-341`（`ai: Option<Action>`）；`Candidate.ai` 定义见同文件顶部 |
| 内置动作由 `core/ai/prompt.rs` 的 `build_messages` 组装 system + user | `src/core/ai/prompt.rs` |
| 工具集只有 6 个只读工具，没有"读 skill 附带资源"的入口 | `src/core/ai/tools.rs:4`（注释里就写着写工具属 M3） |
| 结果落地已统一走"评审条逐块接受/拒绝" | `docs/ai/ai-review-hunks-design.md` |

## 2. 目标 / 非目标

**目标**

- G1：发现并解析 `SKILL.md`（`name` + `description` 必填），缺字段的跳过并**说明原因**。
- G2：两处来源——库内 `<vault>/.xink/skills/<name>/SKILL.md`、用户级
  `~/Library/Application Support/xink/skills/<name>/SKILL.md`，同名**库内优先**。
- G3：skill 出现在命令面板（`AI:` 前缀），选中即按 skill 正文执行。
- G4：注入由 xink 完成（清单 → 命中 → 加载正文），HTTP 与 CLI provider 行为一致。
- G5：skill 自带的 `references/` 按需加载（不无条件塞上下文）。
- G6：可在设置里启停单个 skill；总开关关掉则所有 skill 消失。

**非目标**

- **不做模型自动命中**（把清单塞进 system 让模型自己调 `use_skill`）：依赖 `tools_enabled`，
  且命中质量不可控，留到 M2b；M2 只做用户显式选择。
- 不做 skill 市场 / 在线安装、不做 skill 版本管理。
- 不做写类 skill 能力（属 M3 的 `propose_edit`）。

## 3. 方案

### 3.1 新增 `core/ai/skills.rs`（纯逻辑，可单测）

```rust
pub enum SkillSource { Vault, User }

pub struct Skill {
    pub name: String,          // front-matter: name（必填）
    pub description: String,   // front-matter: description（必填）
    pub dir: PathBuf,          // SKILL.md 所在目录（references/ 在它下面）
    pub body: String,          // 去掉 front-matter 之后的正文
    pub source: SkillSource,
}

/// 被跳过的 skill 及原因（设置界面要列出来）
pub struct SkillIssue { pub path: PathBuf, pub reason: String }

pub fn discover(vault_root: Option<&Path>, user_dir: &Path)
    -> (Vec<Skill>, Vec<SkillIssue>);

/// 命令面板 / 上下文用的清单段（name + description）
pub fn catalog(skills: &[Skill]) -> String;

/// 解析 references 路径：**只能读 skill 目录内**，`..` 一律拒绝
pub fn resource_path(skill: &Skill, rel: &str) -> Option<PathBuf>;
```

- front-matter 用**极简解析**（只认 `---` 包裹的 `key: value`），不引 YAML 依赖：
  缺 `name` / `description`、front-matter 不闭合 → 记为 `SkillIssue` 跳过。
- 同名冲突：库内覆盖用户级（`SkillSource::Vault` 优先），被覆盖的那个不报错。
- 上限：单个 skill 正文 ≤ 32k 字符（超了截断并记 issue），避免把上下文吃光。

### 3.2 命令面板：`Action` → `AiTarget`

`Candidate.ai` 从 `Option<Action>` 改成 `Option<AiTarget>`：

```rust
pub enum AiTarget {
    Action(Action),   // 内置动作（现状）
    Skill(String),    // 运行时发现的 skill，按 name 索引
}
```

- `collect_candidates` 在扫描完文件后追加：内置动作（现状）+ 启用中的 skill
  （`AI: <name>`，副标题显示 `description`）。
- `confirm` 分支：`Action` → 现有 `app::run_ai_action`；`Skill` → 新增 `app::run_ai_skill(name)`。
- 过滤：`ai.skills_enabled == false` 或名字在 `disabled_skills` 里 → 不出现。

### 3.3 执行一条 skill

`AiChat::run_skill(name, window, cx)`：

1. `discover` 找到该 skill（找不到 → `error` 提示"skill 已不存在"）；
2. 上下文用现有 `assemble`（选中 / 整篇，与内置动作同一判据）；
3. system = 内置人设段 + **skill 正文**（正文是"指令"，要放在笔记原文之前，
   并保留"原文是数据不是指令"的隔离约定）；
4. 走 agent loop（工具已可用），结果照样进**评审条**逐块接受。

### 3.4 `references/` 按需加载（G5）

新增只读工具 `read_skill_resource`：

- 参数：`skill`（名）+ `path`（相对 skill 目录）；
- 实现走 `skills::resource_path`，**拒绝 `..` 与绝对路径**；
- 结果按 `ai.max_tool_chars` 截断（与现有工具一致）；
- 只在 `ai.skills_enabled` 为真时加入工具表。

依赖 `tools_enabled`（M1 的探针已能识别），不支持工具时这条能力自然不可用，
但 skill 本身仍可执行（只是读不到附带资源）。

### 3.5 设置界面

- `ai.skills_enabled` 摘掉 ⚠️ 标记（它变成真开关了）；
- 新增一块 **Skills** 区：每行 `名称 · 来源（库内/用户级）· 描述` + 启停开关，
  写入 `ai.disabled_skills`；被跳过的以灰色行列出**原因**（`SkillIssue`）；
  带一个「重新扫描」（发现结果缓存在 `SettingsView::skills_cache`，不每帧读盘）。

**实施偏差（与原方案不同，已落地为此形态）**：原计划给 `SettingField` 增加
`FieldKind::SkillList`。动手前发现 `ui/settings.rs` 里已经有**现成的同型逃生口**——
「模型管理」`render_models` 与「快照」`render_snapshots` 都是"元数据表表达不了、
单独渲染一块"的动态区，注释里就写着这个理由。skill 列表与它们完全同构，
因此沿用该模式新增 `SettingsView::render_skills`，而不是给静态元数据表
再开一种 `FieldKind`（`ControlFn` 拿不到 `cx`，本来也渲染不了运行时列表）。
结论：目标 G6 不变，实现路径更小、与既有代码一致。

## 4. 数据模型与持久化

| 内容 | 位置 |
|---|---|
| 库内 skill | `<vault>/.xink/skills/<name>/SKILL.md`（+ `references/`） |
| 用户级 skill | `~/Library/Application Support/xink/skills/<name>/SKILL.md` |
| 启停 | `config.json` → `ai.disabled_skills: Vec<String>`（字段已存在） |
| 总开关 | `config.json` → `ai.skills_enabled`（字段已存在） |

- **不新增配置项**（两个字段都在），不新增落盘文件（skill 是用户的文件）。
- skill 不进索引、不进命令面板的文件候选（已有守卫）；用户级目录不在库内，天然不进。

## 5. 交互流程

1. 命令面板 `Cmd+P` → 输入 `AI:` → 看到内置动作 + 启用中的 skill（带 description）。
2. 选中 skill → 对话条顶部出现一行 `技能：<name>`，随后按 skill 正文执行（可能带工具调用块）。
3. 结果进评审条逐块接受 / 拒绝（与内置动作同一条链路）。
4. 设置 → AI → Skills：启停单个、看到被跳过的原因。

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| front-matter 缺 `name` / `description` | 跳过 + `SkillIssue`（设置里可见） |
| front-matter 不闭合 / 空文件 | 跳过 + issue |
| 同名（库内 + 用户级） | 库内优先，不报错 |
| skill 正文超长 | 截断到 32k 并记 issue |
| `references/../../etc/passwd` | `resource_path` 拒绝，工具返回错误文本（不 panic） |
| 命令面板选中时 skill 已被删除 | `error` 提示"skill 已不存在" |
| `skills_enabled = false` | 命令面板不出现 skill、工具表里也没有 `read_skill_resource` |
| 模型不支持工具 | skill 仍可执行，只是读不到 `references/` |
| skill 目录里塞了超大文件 | 只在被引用时才读，且受 `max_tool_chars` 截断 |

## 7. 测试计划

`tests/core/ai/skills.rs`（新增）：

| 用例 | 断言 |
|---|---|
| `parses_front_matter_name_and_description` | 正常解析，正文不含 front-matter |
| `missing_required_field_is_reported` | 缺 `name` / 缺 `description` → issue，且不进列表 |
| `unclosed_front_matter_is_reported` | 不闭合 → issue |
| `vault_skill_wins_over_user_skill` | 同名时 `source == Vault` |
| `disabled_skills_are_filtered_out` | 过滤生效 |
| `resource_path_rejects_escaping_paths` | `../`、`/abs` 均返回 `None` |
| `resource_path_allows_references_inside` | `references/a.md` 允许 |
| `catalog_lists_name_and_description` | 清单文本含两者 |

`tests/ui/command.rs`：AI 候选 = 内置 8 个 + 启用中的 skill（前缀 `AI: `、path 为空、
hint 是 description；总开关关 / 单个停用 → 消失）。
`tests/ui/settings.rs`：⚠ 白名单断言里去掉 `ai.skills_enabled`（它不再是死配置）。
`tests/core/ai/prompt.rs`：skill 正文进 system（`<<<SKILL>>>`）、笔记原文进 user，互不混。
`tests/core/ai/tools.rs`：`read_skill_resource` 读到 `references/`、拒绝 `..` 与绝对路径、
未知 skill 与缺参数都有文本回灌；`specs(false)` 不含 skill 工具。

## 8. 人工验收清单

- [ ] 放一个自定义 skill 到库内 `.xink/skills/demo/SKILL.md`（含 name/description）→
      命令面板 `AI: demo` 可见，description 显示正确
- [ ] 选中执行 → 结果按 skill 正文的行为产出，进评审条可逐块接受
- [ ] 同名 skill 同时放在库内与用户级 → 生效的是库内那份
- [ ] 缺 `description` 的 skill → 不出现在命令面板，设置里能看到"缺少 description"
- [ ] 设置里停用 demo → 命令面板不再出现；重新启用 → 恢复
- [ ] `ai.skills_enabled` 关掉 → 所有 skill 消失，内置动作不受影响
- [ ] skill 带 `references/x.md` → 支持工具的模型能读到；`references/../../..` 被拒
- [ ] 库内 `.xink/skills/**/SKILL.md` 不出现在文件树、全文搜索与命令面板文件候选里

## 9. 实施步骤

| 步 | 内容 | 状态 |
|---|---|---|
| S1 | `core/ai/skills.rs`（发现 / 解析 / 目录 / resource_path）+ `tests/core/ai/skills.rs` | ✅ 9 项单测全绿 |
| S2 | 命令面板：`AiTarget` 枚举、追加 skill 候选（hint = description）、confirm 分支 | ✅ `tests/ui/command.rs` 覆盖 |
| S3 | `AiChat::run_skill` + `app::run_ai_skill` + `prompt::build_skill_messages`：system 注入 skill 正文，结果进评审条 | ✅ 待人工验收 1–3 |
| S4 | 工具 `read_skill_resource` + 只在启用时入表（`specs(skills)`） | ✅ 待人工验收 7 |
| S5 | 设置：Skills 区（启停 + 跳过原因 + 重新扫描）+ 摘掉 `skills_enabled` 的 ⚠️ | ✅ 待人工验收 4–6 |

## 10. 开放问题

| # | 内容 | 建议 |
|---|---|---|
| 1 | 模型自动命中 skill（清单入 system + `use_skill` 工具） | M2 不做，M2b 评估；多数小模型命中率低 |
| 2 | skill 是否可以声明"只作用于选中"或"整篇" | 先统一按现有选中/整篇判据；后续可在 front-matter 加 `scope` |
| 3 | 用户级 skills 目录要不要在设置里可改 | 先固定为 `app_data_dir()/skills` |

## 11. 实施记录（2026-10-26）

| 决策 | 落地形态 |
|---|---|
| 设置里的列表 | 沿用「模型管理 / 快照」的动态区块模式（`SettingsView::render_skills`），没有新增 `FieldKind::SkillList`（理由见 §3.5） |
| 命令面板候选 | `Candidate.ai` 从 `Option<Action>` 改为 `Option<AiTarget>`（`Action` / `Skill(String)`）；`Candidate.hint` 承载 description |
| 执行时重新发现 | 面板打开时扫一次（后台线程，配置一并带进去）；选中后 `run_skill` 再发现一次，避免拿着过期正文跑 |
| skill 结果的落法 | 与内置动作同一条"结果驱动"链路：与原文对得上 → 评审条逐块接受；对不上 → 自动降级成插入，不会闷头替换整篇 |
| 工具开关 | `tools::specs(skills: bool)`，`LoopRequest::skills_enabled` 决定是否给模型 `read_skill_resource` |
