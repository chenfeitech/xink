//! 库级长任务：任务模型、批量标签改写、计划生成与落盘
//!
//! 设计见 `docs/ai/ai-m3-library-tasks-design.md`。
//! M3-P1 只做**规则可判定**的批量标签动作（改 / 加 / 删），不经过模型：
//! 结果由字符串替换规则决定，因此这一层是纯函数 + 一次读盘，可以单测。
//!
//! 三条硬约定：
//! 1. **计划只读**：`plan` 只读盘算预览，写盘由执行器（UI 层）在用户确认后做；
//! 2. **一处都不猜**：整标签 token 才命中（`#AI` 命中、`#AI助手` 不命中），
//!    代码块与 front-matter 分别处理，命中不了就跳过并**说明原因**；
//! 3. **可恢复**：`Task.cursor` 与每条的状态一起落盘，中断后不重跑已完成的。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::media::AI_DIR_NAME;
use crate::state::workspace::{read_json, write_json_atomic};

/// `.xink` 下放长任务的子目录（`.xink` 已被文件树 / 索引 / 命令面板恒跳）
const TASKS_DIR: &str = "agent/tasks";
/// 任务文件名
const TASK_FILE: &str = "task.json";
/// 人可读的进度文件（与任务同目录，出问题时直接打开看）
const PROGRESS_FILE: &str = "progress.md";

/// 一批最多处理多少篇：到数就停下报告（设计 Q9）
pub const BATCH_SIZE: usize = 20;

/// 批量动作的种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    /// 全库把 `#target` 改成 `#new_tag`
    Retag,
    /// 给候选集合统一加一个标签
    AddTag,
    /// 从候选集合里删掉一个标签
    RemoveTag,
}

impl TaskKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Retag => "retag",
            Self::AddTag => "add_tag",
            Self::RemoveTag => "remove_tag",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Retag => "改标签",
            Self::AddTag => "加标签",
            Self::RemoveTag => "删标签",
        }
    }
}

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 计划已生成，还没跑
    Ready,
    /// 正在跑（落盘时不会是它：中途退出一律记为 Paused）
    Running,
    /// 到批上限或用户停止
    Paused,
    /// 全部处理完
    Done,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "待执行",
            Self::Running => "执行中",
            Self::Paused => "已暂停",
            Self::Done => "已完成",
        }
    }
}

/// 一条（一篇笔记）的状态：跳过与失败都要带原因，静默丢失等于骗用户
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemState {
    Pending,
    Done,
    Skipped(String),
    Failed(String),
}

impl ItemState {
    /// 已终结：执行器不再处理它（中断恢复靠这个判断）
    pub fn is_finished(&self) -> bool {
        !matches!(self, Self::Pending)
    }

    pub fn label(&self) -> String {
        match self {
            Self::Pending => "待处理".into(),
            Self::Done => "已改".into(),
            Self::Skipped(reason) => format!("跳过（{reason}）"),
            Self::Failed(reason) => format!("失败（{reason}）"),
        }
    }
}

/// 一条待处理的笔记
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    pub path: PathBuf,
    /// 计划阶段算好的预览：改前 → 改后，只含变化的那些行
    pub preview: Vec<(String, String)>,
    pub state: ItemState,
}

/// 一个库级任务
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub kind: TaskKind,
    /// 要改 / 加 / 删的标签（不带 `#`）
    pub target: String,
    /// 改后的标签（仅 [`TaskKind::Retag`] 用，其余为空）
    pub new_tag: String,
    pub status: TaskStatus,
    pub items: Vec<TaskItem>,
    /// 下一个要处理的下标：中断恢复靠它
    pub cursor: usize,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Task {
    /// 建一个新任务（不落盘：`save` 是单独一步，计划阶段可以先不写）
    pub fn new(id: String, kind: TaskKind, target: &str, new_tag: &str) -> Self {
        let now = now_secs();
        Self {
            id,
            kind,
            target: target.to_string(),
            new_tag: new_tag.to_string(),
            status: TaskStatus::Ready,
            items: Vec::new(),
            cursor: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// 面板标题用的一句话
    pub fn title(&self) -> String {
        match self.kind {
            TaskKind::Retag => format!(
                "{}：#{} → #{}",
                self.kind.label(),
                self.target,
                self.new_tag
            ),
            TaskKind::AddTag => format!("{}：#{}", self.kind.label(), self.target),
            TaskKind::RemoveTag => format!("{}：#{}", self.kind.label(), self.target),
        }
    }

    /// 还有多少条待处理
    pub fn pending(&self) -> usize {
        self.items
            .iter()
            .filter(|item| !item.state.is_finished())
            .count()
    }

    /// （已完成，跳过，失败）
    pub fn stats(&self) -> (usize, usize, usize) {
        let mut out = (0, 0, 0);
        for item in &self.items {
            match item.state {
                ItemState::Done => out.0 += 1,
                ItemState::Skipped(_) => out.1 += 1,
                ItemState::Failed(_) => out.2 += 1,
                ItemState::Pending => {}
            }
        }
        out
    }

    /// 从 `cursor` 往后取一批待处理的下标（已完成的不重复给）
    pub fn next_batch(&self, size: usize) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .skip(self.cursor)
            .filter(|(_, item)| !item.state.is_finished())
            .take(size)
            .map(|(ix, _)| ix)
            .collect()
    }

    /// 没有待处理的了
    pub fn is_finished(&self) -> bool {
        self.pending() == 0
    }
}

// ── 文本改写（纯函数） ────────────────────────────────────────

/// 单篇的改写结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagOutcome {
    /// 改后的整篇文本
    Changed(String),
    /// 这篇没有目标标签（"加标签"不会出现）
    Unchanged,
    /// 跳过并说明原因（如已含目标标签）
    Skipped(String),
}

impl TagOutcome {
    /// 计划阶段：没有目标标签的一律变成"跳过 + 原因"，不放进待处理
    fn or_skip(self, reason: &str) -> Self {
        match self {
            Self::Unchanged => Self::Skipped(reason.to_string()),
            other => other,
        }
    }
}

/// 按动作分派（执行器用）
pub fn apply_text(kind: TaskKind, text: &str, target: &str, new_tag: &str) -> TagOutcome {
    match kind {
        TaskKind::Retag => retag_text(text, target, new_tag),
        TaskKind::AddTag => add_tag_text(text, target),
        TaskKind::RemoveTag => remove_tag_text(text, target),
    }
}

/// 把 `#target` 改成 `#new_tag`
pub fn retag_text(text: &str, target: &str, new_tag: &str) -> TagOutcome {
    if target.is_empty() || new_tag.is_empty() {
        return TagOutcome::Unchanged;
    }
    // 已经有目标标签就别动：否则改完会出现两个重复标签
    if has_tag(text, new_tag) {
        return TagOutcome::Skipped(format!("已含 #{new_tag}"));
    }
    let (out, _, found) = rewrite(text, target, Some(new_tag));
    if !found {
        return TagOutcome::Unchanged;
    }
    TagOutcome::Changed(out)
}

/// 给整篇加一个标签
///
/// 有 front-matter `tags:` 就加进列表，没有就在**文末**追加一行 `#tag`
/// （正文里已经有的话跳过）。
pub fn add_tag_text(text: &str, tag: &str) -> TagOutcome {
    if tag.is_empty() {
        return TagOutcome::Unchanged;
    }
    if has_tag(text, tag) {
        return TagOutcome::Skipped(format!("已含 #{tag}"));
    }
    let (out, _, _) = rewrite(text, tag, None);
    TagOutcome::Changed(out)
}

/// 删掉整篇里的 `#tag`
pub fn remove_tag_text(text: &str, tag: &str) -> TagOutcome {
    if tag.is_empty() {
        return TagOutcome::Unchanged;
    }
    let (out, _, found) = rewrite(text, tag, Some(""));
    if !found {
        return TagOutcome::Unchanged;
    }
    TagOutcome::Changed(out)
}

/// 这篇里有没有这个标签（正文 token 或 front-matter 列表）
pub fn has_tag(text: &str, tag: &str) -> bool {
    let (_, _, found) = rewrite(text, tag, None);
    found
}

/// 一次扫描同时做三件事：改写、收集预览、报告是否命中
///
/// `replace`：`Some(新标签)` 改名（`Some("")` 删除）、`None` 只探测不改写
/// （`add_tag` 走"追加"分支，也用它先探一遍）。
fn rewrite(
    text: &str,
    target: &str,
    replace: Option<&str>,
) -> (String, Vec<(String, String)>, bool) {
    let adding = replace.is_none();
    let mut out: Vec<String> = Vec::new();
    let mut preview: Vec<(String, String)> = Vec::new();
    let mut found = false;
    // 只有"加标签"且探测不到时，才需要在 front-matter / 文末动手脚
    let mut need_add = adding;
    let mut in_fm = false;
    let mut fm_closed = false;
    let mut in_fence = false;
    let mut in_tags_list = false;
    let mut inserted = false;
    // 删掉的行在开头时，紧跟着的空行也要一起去掉
    let mut skip_next_blank = false;

    let lines: Vec<&str> = text.lines().collect();
    for (ix, line) in lines.iter().enumerate() {
        let line = *line;

        if skip_next_blank {
            skip_next_blank = false;
            if line.trim().is_empty() {
                continue;
            }
        }

        // front-matter 的开启 / 闭合：只在文件开头那一块
        if !fm_closed && ix == 0 && line.trim() == "---" {
            in_fm = true;
            out.push(line.to_string());
            continue;
        }
        if in_fm && line.trim() == "---" {
            // 闭合前还没加过，就在 front-matter 末尾补一个 `tags:`
            if need_add && !inserted {
                out.push(format!("tags: [{target}]"));
                preview.push((String::new(), format!("tags: [{target}]")));
                inserted = true;
            }
            in_fm = false;
            fm_closed = true;
            in_tags_list = false;
            out.push(line.to_string());
            continue;
        }

        // 代码块里的 `#xxx` 不是标签（`#define` 之类会被误伤）
        if is_fence(line) {
            in_fence = !in_fence;
            in_tags_list = false;
            out.push(line.to_string());
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            continue;
        }

        if in_fm {
            if let Some(rest) = tags_value(line) {
                // `tags: [a, b]`
                let (value, hit) = edit_list(&rest, target, replace);
                found |= hit;
                match value {
                    None => {
                        // 删空了：整行去掉
                        preview.push((line.to_string(), String::new()));
                        need_add = false;
                        continue;
                    }
                    Some(new_value) => {
                        let new_line = replace_value(line, &new_value);
                        if new_line != line {
                            preview.push((line.to_string(), new_line.clone()));
                        }
                        need_add = false;
                        out.push(new_line);
                        continue;
                    }
                }
            }
            if let Some(item) = list_item(line) {
                // `tags:` 下面 `- a` 这种写法
                if in_tags_list && item == target {
                    found = true;
                    match replace {
                        Some("") => {
                            preview.push((line.to_string(), String::new()));
                            continue;
                        }
                        Some(new_tag) => {
                            let new_line = format!("{}- {}", indent_of(line), new_tag);
                            preview.push((line.to_string(), new_line.clone()));
                            need_add = false;
                            out.push(new_line);
                            continue;
                        }
                        None => {}
                    }
                }
                out.push(line.to_string());
                continue;
            }
            // `tags:` 后紧跟列表：记住这一支，后面遇到 `- x` 才算标签项
            in_tags_list = is_tags_key(line);
            out.push(line.to_string());
            continue;
        }

        // 正文：只替换完整的 `#tag` token
        let (new_line, hit) = rewrite_inline(line, target, replace);
        found |= hit;
        if hit && replace == Some("") && new_line.trim().is_empty() {
            // 整行只剩空白：连这一行一起删掉，顺手把多出来的那个空行也收掉
            preview.push((line.to_string(), String::new()));
            let next_is_blank = lines
                .get(ix + 1)
                .map(|next| next.trim().is_empty())
                .unwrap_or(true);
            if next_is_blank && matches!(out.last(), Some(last) if last.trim().is_empty()) {
                out.pop();
            }
            if out.is_empty() {
                skip_next_blank = true;
            }
            continue;
        }
        if new_line != line {
            preview.push((line.to_string(), new_line.clone()));
        }
        out.push(new_line);
    }

    // 加标签：没有 front-matter 可挂，就追加到文末
    if need_add && !inserted {
        let mut tail = out.join("\n");
        if !tail.is_empty() && !tail.ends_with('\n') {
            tail.push('\n');
        }
        if !tail.is_empty() && !tail.ends_with("\n\n") {
            tail.push('\n');
        }
        preview.push((String::new(), format!("#{target}")));
        tail.push_str(&format!("#{target}\n"));
        return (tail, preview, found);
    }

    // `lines()` 丢掉了末尾的换行信息：原来有就补回去
    let mut joined = out.join("\n");
    if text.ends_with('\n') && !joined.is_empty() {
        joined.push('\n');
    }
    (joined, preview, found)
}

/// 一行是不是代码围栏
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// 行首缩进
fn indent_of(line: &str) -> String {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>()
}

/// 这一行是不是 front-matter 里的 `tags:` 键（值是空的，下面跟列表）
fn is_tags_key(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "tags:" || trimmed == "tags: []" || trimmed == "tags: [ ]"
}

/// 取 `tags: [a, b]` 里的值部分（含方括号）；不是这种写法返回 None
///
/// `tags: []` 也算：它是一份合法的空列表，加标签时要往里填。
fn tags_value(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("tags:")?;
    let rest = rest.trim();
    if !rest.starts_with('[') || !rest.ends_with(']') {
        return None;
    }
    Some(rest.to_string())
}

/// 把 `tags:` 行的值换成新的（保留缩进）
fn replace_value(line: &str, value: &str) -> String {
    format!("{}tags: {}", indent_of(line), value)
}

/// front-matter 里的列表项 `- x`（返回去掉引号后的名字）
fn list_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix('-')?;
    Some(unquote(rest.trim()))
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

/// 改 `[a, b]` 里的条目
///
/// 返回 `(新值, 是否命中)`：新值为 `None` 表示删空了，整行要去掉。
fn edit_list(value: &str, target: &str, replace: Option<&str>) -> (Option<String>, bool) {
    let inner = &value[1..value.len() - 1];
    let entries: Vec<&str> = inner.split(',').map(|e| e.trim()).collect();
    let mut out: Vec<String> = Vec::new();
    let mut hit = false;

    for entry in &entries {
        if entry.is_empty() {
            continue;
        }
        let name = unquote(entry);
        if name == target {
            hit = true;
            match replace {
                Some("") => continue,
                Some(new_tag) => out.push(quote_like(entry, new_tag)),
                None => out.push(entry.to_string()),
            }
            continue;
        }
        out.push(entry.to_string());
    }

    // 加标签：列表里没有就补一个
    if replace.is_none() && !hit {
        out.push(target.to_string());
    }
    if out.is_empty() {
        return (None, hit);
    }
    (Some(format!("[{}]", out.join(", "))), hit)
}

/// 沿用原来条目的引号风格
fn quote_like(entry: &str, name: &str) -> String {
    if entry.starts_with('"') {
        format!("\"{name}\"")
    } else if entry.starts_with('\'') {
        format!("'{name}'")
    } else {
        name.to_string()
    }
}

/// 替换一行里完整的 `#tag` token
///
/// `replace`：`Some(新名字)` 改名、`Some("")` 删除、`None` 只探测。
fn rewrite_inline(line: &str, target: &str, replace: Option<&str>) -> (String, bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut found = false;
    let mut at_start = true;
    let mut ix = 0;

    while ix < chars.len() {
        let c = chars[ix];
        let after_space = at_start || chars[ix - 1].is_whitespace();
        if c == '#' && after_space {
            let mut end = ix + 1;
            while end < chars.len() {
                let ch = chars[end];
                if ch.is_whitespace() || ch == '#' {
                    break;
                }
                end += 1;
            }
            let token: String = chars[ix + 1..end].iter().collect();
            if !token.is_empty() && token == target {
                found = true;
                match replace {
                    Some("") => {}
                    Some(new_tag) => {
                        out.push('#');
                        out.push_str(new_tag);
                    }
                    None => {
                        out.push('#');
                        out.push_str(&token);
                    }
                }
            } else {
                out.push('#');
                out.push_str(&token);
            }
            ix = end;
            at_start = false;
            continue;
        }
        out.push(c);
        if !c.is_whitespace() {
            at_start = false;
        }
        ix += 1;
    }
    (out, found)
}

// ── 计划生成 ────────────────────────────────────────────────

/// 生成计划：读候选笔记 → 算预览 → 定每条的状态
///
/// **只读**：这一步绝不写盘。读不到的记为 `Failed`，不中断整批。
pub fn plan(kind: TaskKind, target: &str, new_tag: &str, candidates: &[PathBuf]) -> Vec<TaskItem> {
    let mut items = Vec::with_capacity(candidates.len());
    for path in candidates {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                items.push(TaskItem {
                    path: path.clone(),
                    preview: Vec::new(),
                    state: ItemState::Failed(format!("读不到：{err}")),
                });
                continue;
            }
        };
        let (preview, state) =
            match apply_text(kind, &text, target, new_tag).or_skip("没有这个标签") {
                TagOutcome::Changed(_) => {
                    (preview_of(&text, kind, target, new_tag), ItemState::Pending)
                }
                TagOutcome::Skipped(reason) => (Vec::new(), ItemState::Skipped(reason)),
                TagOutcome::Unchanged => (Vec::new(), ItemState::Skipped("没有这个标签".into())),
            };
        items.push(TaskItem {
            path: path.clone(),
            preview,
            state,
        });
    }
    items
}

/// 计划要用预览，因此改动时再算一次：`rewrite` 的返回值里带着它
fn preview_of(text: &str, kind: TaskKind, target: &str, new_tag: &str) -> Vec<(String, String)> {
    let replace = match kind {
        TaskKind::Retag => Some(new_tag),
        TaskKind::RemoveTag => Some(""),
        TaskKind::AddTag => None,
    };
    let (_, preview, _) = rewrite(text, target, replace);
    preview
}

// ── 落盘 ────────────────────────────────────────────────────

/// 任务根目录：`<vault>/.xink/agent/tasks`
pub fn tasks_root(vault_root: &Path) -> PathBuf {
    vault_root.join(AI_DIR_NAME).join(TASKS_DIR)
}

/// 单个任务的目录
pub fn task_dir(vault_root: &Path, id: &str) -> PathBuf {
    tasks_root(vault_root).join(id)
}

/// 新任务的 id：种类 + 标签 + 时间戳，人一眼能认出来
pub fn new_id(kind: TaskKind, target: &str) -> String {
    format!("{}-{}-{}", kind.id(), slug(target), now_secs())
}

fn slug(text: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    out.trim_matches('-').to_string()
}

/// 落盘：`task.json` + 人可读的 `progress.md`
pub fn save(vault_root: &Path, task: &Task) -> Result<(), String> {
    let dir = task_dir(vault_root, &task.id);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    write_json_atomic(&dir.join(TASK_FILE), task).map_err(|err| err.to_string())?;
    std::fs::write(dir.join(PROGRESS_FILE), progress_md(task)).map_err(|err| err.to_string())
}

/// 读回一个任务；不存在或损坏返回 None（不阻塞）
pub fn load(vault_root: &Path, id: &str) -> Option<Task> {
    read_json::<Task>(&task_dir(vault_root, id).join(TASK_FILE)).ok()
}

/// 列出库内所有任务，按更新时间倒序（「继续上次」取第一条）
pub fn list(vault_root: &Path) -> Vec<Task> {
    let Ok(entries) = std::fs::read_dir(tasks_root(vault_root)) else {
        return Vec::new();
    };
    let mut tasks: Vec<Task> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(|name| name.to_string()))
        .filter_map(|id| load(vault_root, &id))
        .collect();
    tasks.sort_by_key(|task| std::cmp::Reverse(task.updated_at));
    tasks
}

/// 人可读的进度：`progress.md`
pub fn progress_md(task: &Task) -> String {
    let (done, skipped, failed) = task.stats();
    let mut out = format!(
        "# {}\n\n- 状态：{}\n- 待处理：{}\n- 已完成 {done} · 跳过 {skipped} · 失败 {failed}\n\n",
        task.title(),
        task.status.label(),
        task.pending(),
    );
    for item in &task.items {
        let name = item
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| item.path.to_string_lossy().to_string());
        out.push_str(&format!("- {} — {}\n", name, item.state.label()));
    }
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/tasks.rs"]
mod tests;
