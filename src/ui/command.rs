//! 命令面板（Cmd+P）：按文件名模糊搜索当前库里的笔记并打开
//!
//! 设计见 `docs/command-palette-design.md`。三条硬约束：
//! 1. **打开只走 `tabs::open_note`**——命中已打开的笔记只激活，不产生第二个标签；
//! 2. **候选来自磁盘扫描**，是派生数据，不落盘、不进索引；
//! 3. **排序用 `core::fuzzy`**——框架 `Command` 组件内置只有子串匹配（`command/item.rs:114`），
//!    没有模糊也没有打分，因此关掉它的过滤，由我们自己算。

use std::cell::Cell;
use std::path::{Path, PathBuf};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::WindowExt;
// `gpui_kit::*` 里的 `Size` 是另一个泛型类型，组件库的 Size 要显式导入
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{Icon, IconName, Sizable, Size};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::ai::skills;
use crate::core::config::AiConfig;
use crate::core::fuzzy::fuzzy_match;
use crate::core::media::{ai_excluded, is_editable_text};
use crate::core::vault::{FileNode, Vault};
use crate::state::workspace::app_data_dir;
use crate::ui::tabs;
use crate::ui::tokens;

/// 最多展示的匹配条数：候选可能上万，全量装进列表会让测量与渲染都变慢
const MAX_RESULTS: usize = 50;
/// 面板宽度（Dialog 默认 448px，命令面板要宽一些以放下路径）
const PALETTE_WIDTH: f32 = 560.;
/// 面板距窗口顶部的距离：命令面板习惯出现在偏上位置，而不是垂直居中
const MARGIN_TOP: f32 = 80.;
/// 列表最大高度
const LIST_MAX_HEIGHT: f32 = 360.;

/// 一条 AI 候选指向什么
///
/// 快捷动作现在也是配置数据（可增删），与 skill 一样只按 id 记住，
/// 正文在执行时再取——因此两者拆成两种目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiTarget {
    /// 快捷动作，按 id 索引
    Action(String),
    /// skill，按 name 索引（执行时再去发现一遍，拿它的正文）
    Skill(String),
}

/// 一个候选
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 用于匹配与展示的文本：文件的相对路径，或 AI 动作名
    pub label: String,
    /// 打开用的绝对路径；AI 动作为空
    pub path: PathBuf,
    /// AI 入口（需求 A9.1：`AI:` 前缀过滤）
    ///
    /// `Some` 时 `path` 为空，确认后不是打开文件，而是对**当前笔记**
    /// 跑这个动作或 skill。
    pub ai: Option<AiTarget>,
    /// 副标题：skill 的 description，帮用户分辨同名相近的技能
    pub hint: Option<String>,
}

/// 命令面板的状态
pub struct CommandPalette {
    state: Entity<CommandState>,
    /// 库内可打开的文件（后台扫描填充）
    files: Vec<Candidate>,
    /// 当前展示的结果：`files` 的下标，按得分降序
    matches: Vec<usize>,
    /// 与 `matches` 同序的命中位置，供高亮使用
    hits: Vec<Vec<usize>>,
    scanning: bool,
    /// 无法列出候选时的提示（没有库 / 扫描失败 / 库是空的）
    notice: Option<String>,
}

impl CommandPalette {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        let mut palette = Self {
            state,
            files: Vec::new(),
            matches: Vec::new(),
            hits: Vec::new(),
            scanning: false,
            notice: None,
        };
        palette.start_scan(window, cx);
        palette
    }

    /// 把焦点交给查询框。
    ///
    /// `Dialog` 打开时抢走的是它自己的焦点句柄，不显式聚焦的话
    /// 用户得先点一下输入框才能打字。
    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    /// 后台扫描当前库。
    ///
    /// 扫描是同步 IO，绝不能在主线程做（万级目录会明显卡顿），
    /// 因此丢给 `background_spawn`，完成后再回到实体上更新候选。
    fn start_scan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // skill 只在扫描时才发现一次： discovering 要读盘，放在主线程会卡，
        // 这里把配置一起带进后台任务（配置很小，克隆一份最省事）
        let (root, ai) = cx
            .try_global::<AppState>()
            .map(|state| (state.vaults.active_root(), state.config.ai.clone()))
            .unwrap_or((None, AiConfig::default()));

        let Some(root) = root else {
            self.notice = Some("还没有打开库：先在左侧打开一个文件夹".to_string());
            return;
        };

        self.scanning = true;
        self.notice = None;
        self.state.update(cx, |state, cx| {
            state.set_loading(true, window, cx);
        });

        let task = cx.background_spawn(async move { collect_candidates(&root, &ai) });
        cx.spawn_in(window, async move |this, window| {
            let result = task.await;
            _ = this.update_in(window, |this, window, cx| {
                this.apply_scan(result, window, cx);
            });
        })
        .detach();
    }

    /// 扫描结果落地：更新候选并按当前查询重算一次。
    ///
    /// 扫描期间用户输入不会被打断——这里只是用新候选把结果重算一遍。
    fn apply_scan(
        &mut self,
        result: Result<Vec<Candidate>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scanning = false;
        self.state.update(cx, |state, cx| {
            state.set_loading(false, window, cx);
        });

        match result {
            Ok(files) => {
                self.files = files;
                self.notice = if self.files.is_empty() {
                    Some("库里还没有笔记".to_string())
                } else {
                    None
                };
            }
            Err(err) => {
                // 保留已有候选：扫描失败不该让面板变空
                self.notice = Some(format!("扫描库失败：{err}"));
            }
        }

        let query = self.state.read(cx).query(cx).to_string();
        self.refresh_matches(&query, cx);
    }

    /// 查询变化：重算匹配结果
    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.refresh_matches(query, cx);
    }

    /// 按查询重新打分排序，只保留前 [`MAX_RESULTS`] 条
    fn refresh_matches(&mut self, query: &str, cx: &mut Context<Self>) {
        self.matches.clear();
        self.hits.clear();
        if self.files.is_empty() {
            return;
        }

        // 空查询不做打分：得分差异只来自长度，排序会显得随机，
        // 这时按扫描顺序（相对路径字典序）展示最可预期
        if query.trim().is_empty() {
            self.matches.extend(0..self.files.len().min(MAX_RESULTS));
            self.hits.resize(self.matches.len(), Vec::new());
            cx.notify();
            return;
        }

        let mut scored: Vec<(usize, u32, Vec<usize>)> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(ix, candidate)| {
                fuzzy_match(query, &candidate.label)
                    .map(|matched| (ix, matched.score, matched.positions))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(MAX_RESULTS);

        for (ix, _, positions) in scored {
            self.matches.push(ix);
            self.hits.push(positions);
        }
        cx.notify();
    }

    /// 打开选中的候选。
    ///
    /// 先关浮层再打开笔记：反过来的话，`close_dialog` 会把焦点还给
    /// "打开面板前的元素"，刚聚焦的编辑区会被抢回去。
    fn confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = self
            .matches
            .get(row)
            .and_then(|ix| self.files.get(*ix))
            .cloned()
        else {
            return;
        };
        close_palette(window, cx);
        // AI 入口不打开文件：对当前笔记跑它（没打开笔记时函数自己会提示）
        match candidate.ai {
            Some(AiTarget::Action(id)) => {
                crate::app::run_ai_action(&id, window, cx);
                return;
            }
            Some(AiTarget::Skill(name)) => {
                crate::app::run_ai_skill(&name, window, cx);
                return;
            }
            None => {}
        }
        tabs::open_note(candidate.path, window, cx);
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // 预先取好文本：items 的构造器要 `move`，不能在闭包里再借用 self
        // 第四位是"是不是 AI 入口"：决定条目用哪个图标
        let rows: Vec<(String, Vec<usize>, Option<String>, bool)> = self
            .matches
            .iter()
            .zip(self.hits.iter())
            .filter_map(|(ix, hits)| {
                self.files.get(*ix).map(|c| {
                    (
                        c.label.clone(),
                        hits.clone(),
                        c.hint.clone(),
                        c.ai.is_some(),
                    )
                })
            })
            .collect();
        let notice = self.notice.clone().unwrap_or_default();

        let palette = Command::new(&self.state)
            // 自己算匹配与排序，组件只负责渲染与键盘导航
            .filterable(false)
            .bordered(false)
            .placeholder("搜索文件名…")
            .max_h(px(LIST_MAX_HEIGHT))
            .items(rows.into_iter().map(|(label, hits, hint, is_ai)| {
                CommandItem::new().child(move |_, cx| {
                    let mut row = div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(tokens::SPACE_SM))
                        // 条目以前没有任何图标：文件和 AI 动作长得一样
                        .child(
                            Icon::new(if is_ai {
                                IconName::Bot
                            } else {
                                IconName::FileText
                            })
                            .with_size(Size::Size(px(14.)))
                            .text_color(cx.theme().muted_foreground),
                        )
                        .child(highlighted_label(&label, &hits, cx));
                    // 渲染闭包是 `Fn`（每帧都可能再调一次），不能把 hint 移走
                    if let Some(hint) = hint.clone() {
                        row = row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(hint),
                        );
                    }
                    row
                })
            }))
            .empty(move |_, _, cx| {
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(notice.clone())
            })
            .on_query({
                let entity = entity.clone();
                move |query: &str, _, cx| {
                    let query = query.to_string();
                    entity.update(cx, |this, cx| this.set_query(&query, cx));
                }
            })
            .on_confirm(move |path, window, cx| {
                let row = path.row;
                entity.update(cx, |this, cx| this.confirm(row, window, cx));
            })
            .on_cancel(close_palette);

        div().flex_col().child(palette).child(hint_row(cx))
    }
}

/// 面板底部的键位提示
///
/// 命令面板以前一个按键提示都没有。条目的候选是文件与 AI 动作，都不是
/// gpui 的 `Action`、没有绑定可查，所以这里提示的是面板自己的操作键。
fn hint_row(cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(tokens::SPACE_XS))
        .px(px(tokens::SPACE_SM))
        .py(px(tokens::SPACE_XS))
        .border_t_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(muted);
    for (ix, (key, label)) in [("up", "移动"), ("enter", "打开"), ("escape", "关闭")]
        .iter()
        .enumerate()
    {
        if ix > 0 {
            row = row.child(div().child("·"));
        }
        // 键位解析不出来就不画这个键，不显示空标签
        if let Ok(stroke) = Keystroke::parse(key) {
            row = row.child(Kbd::new(stroke));
        }
        row = row.child(div().child(*label));
    }
    row
}

/// 把路径按命中位置切段渲染：命中的字符加粗并染色。
///
/// 模糊匹配一定要给出"为什么命中"的反馈，否则用户看到一串无关结果
/// 只会觉得排序在乱来。
fn highlighted_label(label: &str, hits: &[usize], cx: &App) -> Div {
    let hit_color = cx.theme().primary;
    let mut segments: Vec<(bool, String)> = Vec::new();
    let mut next_hit = 0usize;

    for (ix, ch) in label.chars().enumerate() {
        let hit = hits.get(next_hit) == Some(&ix);
        if hit {
            next_hit += 1;
        }
        match segments.last_mut() {
            Some((last_hit, text)) if *last_hit == hit => text.push(ch),
            _ => segments.push((hit, ch.to_string())),
        }
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .text_sm()
        .children(segments.into_iter().map(move |(hit, text)| {
            let mut span = div().child(text);
            if hit {
                span = span.text_color(hit_color).font_weight(FontWeight::BOLD);
            }
            span
        }))
}

/// 打开命令面板（Cmd+P）。
///
/// 挂到窗口的 dialog 层——gpui-kit 0.7 起 `Root` 自己托管浮层，
/// 通过 `WindowExt::open_dialog`（`window.open_dialog`）打开即可。
pub fn open_command_palette(window: &mut Window, cx: &mut App) {
    // 视图必须在 build 闭包 **之外** 创建一次。
    //
    // build 闭包是"渲染闭包"而不是一次性构造器：dialog 层重建时会再跑一次。
    // 在闭包里 `cx.new` 等于"每敲一个键就换一个面板实例"，输入内容与选中项
    // 被清空，表现就是"能弹出来但打不了字"。
    let view = cx.new(|cx| CommandPalette::new(window, cx));
    // 只在本次打开的**首次** build 里给焦点：泛层重建时无条件 focus 会去抢
    // 别的控件或嵌套 dialog 的焦点（gpui-kit 0.8 的契约要求）。
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            view.update(cx, |palette, cx| palette.focus(window, cx));
        }
        let view = view.clone();
        dialog
            .w(px(PALETTE_WIDTH))
            .margin_top(px(MARGIN_TOP))
            // 命令面板不需要右上角的关闭按钮：Esc 与点击遮罩都能关
            .close_button(false)
            .overlay(true)
            .overlay_closable(true)
            .content(move |content, _, _| content.child(view.clone()))
    });
}

/// 关闭命令面板
fn close_palette(window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
}

/// 扫描库内可打开的文件（在后台线程执行）。
///
/// 只收 [`is_editable_text`] 命中的文件：与目录树、打开入口用同一判据，
/// 否则面板里会出现"选了却打不开"的条目。
fn collect_candidates(root: &Path, cfg: &AiConfig) -> Result<Vec<Candidate>, String> {
    let vault = Vault::open(root.to_path_buf()).map_err(|err| err.to_string())?;
    let tree = vault.scan(false).map_err(|err| err.to_string())?;

    let mut out = Vec::new();
    flatten(root, &tree, &mut out);
    // 快捷 AI 动作（需求 A9.1）：`AI:` 前缀能过滤出来，它们作用于当前笔记。
    // 列表来自配置，用户新增 / 停用的都会立刻反映。
    for action in cfg.active_actions() {
        out.push(Candidate {
            label: format!("AI: {}", action.name),
            path: PathBuf::new(),
            ai: Some(AiTarget::Action(action.id.clone())),
            hint: None,
        });
    }
    // 运行时发现的 skill：同样的 `AI:` 前缀，副标题是 description。
    // 总开关关掉就一个都不出现；解析失败的那批只在设置里可见，不进面板。
    if cfg.skills_enabled {
        let user_dir = skills::user_skills_dir(&app_data_dir());
        let (found, _issues) = skills::discover(Some(root), &user_dir);
        for skill in skills::enabled(&found, &cfg.disabled_skills) {
            out.push(Candidate {
                label: format!("AI: {}", skill.name),
                path: PathBuf::new(),
                ai: Some(AiTarget::Skill(skill.name.clone())),
                hint: Some(skill.description.clone()),
            });
        }
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(out)
}

/// 展平目录树；`label` 用相对库根的路径（含扩展名）。
fn flatten(root: &Path, node: &FileNode, out: &mut Vec<Candidate>) {
    if !node.is_dir {
        // 目录层已经跳过 `.xink`（`vault::should_ignore`），这里再挡一道：
        // 命令面板的语义就是"人的笔记"，AI 目录不该出现在候选里
        if is_editable_text(&node.path) && !ai_excluded(&node.path) {
            let label = node
                .path
                .strip_prefix(root)
                .unwrap_or(node.path.as_path())
                .to_string_lossy()
                .to_string();
            out.push(Candidate {
                label,
                path: node.path.clone(),
                ai: None,
                hint: None,
            });
        }
        return;
    }
    for child in &node.children {
        flatten(root, child, out);
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/command.rs"]
mod tests;
