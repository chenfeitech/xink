//! 命令面板（Cmd+P）：按文件名模糊搜索当前库里的笔记并打开
//!
//! 设计见 `docs/command-palette-design.md`。三条硬约束：
//! 1. **打开只走 `tabs::open_note`**——命中已打开的笔记只激活，不产生第二个标签；
//! 2. **候选来自磁盘扫描**，是派生数据，不落盘、不进索引；
//! 3. **排序用 `core::fuzzy`**——框架 `Command` 组件内置只有子串匹配（`command/item.rs:114`），
//!    没有模糊也没有打分，因此关掉它的过滤，由我们自己算。

use std::path::{Path, PathBuf};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Root;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::fuzzy::fuzzy_match;
use crate::core::media::is_editable_text;
use crate::core::vault::{FileNode, Vault};
use crate::ui::tabs;

/// 最多展示的匹配条数：候选可能上万，全量装进列表会让测量与渲染都变慢
const MAX_RESULTS: usize = 50;
/// 面板宽度（Dialog 默认 448px，命令面板要宽一些以放下路径）
const PALETTE_WIDTH: f32 = 560.;
/// 面板距窗口顶部的距离：命令面板习惯出现在偏上位置，而不是垂直居中
const MARGIN_TOP: f32 = 80.;
/// 列表最大高度
const LIST_MAX_HEIGHT: f32 = 360.;

/// 一个候选文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 用于匹配与展示的文本：相对库根的路径
    pub label: String,
    /// 打开用的绝对路径
    pub path: PathBuf,
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
        let root = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root());

        let Some(root) = root else {
            self.notice = Some("还没有打开库：先在左侧打开一个文件夹".to_string());
            return;
        };

        self.scanning = true;
        self.notice = None;
        self.state.update(cx, |state, cx| {
            state.set_loading(true, window, cx);
        });

        let task = cx.background_spawn(async move { collect_candidates(&root) });
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
        let Some(path) = self
            .matches
            .get(row)
            .and_then(|ix| self.files.get(*ix))
            .map(|candidate| candidate.path.clone())
        else {
            return;
        };
        close_palette(window, cx);
        tabs::open_note(path, window, cx);
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // 预先取好文本：items 的构造器要 `move`，不能在闭包里再借用 self
        let rows: Vec<(String, Vec<usize>)> = self
            .matches
            .iter()
            .zip(self.hits.iter())
            .filter_map(|(ix, hits)| self.files.get(*ix).map(|c| (c.label.clone(), hits.clone())))
            .collect();
        let notice = self.notice.clone().unwrap_or_default();

        Command::new(&self.state)
            // 自己算匹配与排序，组件只负责渲染与键盘导航
            .filterable(false)
            .bordered(false)
            .placeholder("搜索文件名…")
            .max_h(px(LIST_MAX_HEIGHT))
            .items(rows.into_iter().map(|(label, hits)| {
                CommandItem::new().child(move |_, cx| highlighted_label(&label, &hits, cx))
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
            .on_cancel(close_palette)
    }
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
/// 通过 `Root::open_dialog` 挂到窗口的 dialog 层——`Workspace::render`
/// 已经渲染了 `render_dialog_layer`，不渲染这一层浮层就不会出现。
pub fn open_command_palette(window: &mut Window, cx: &mut App) {
    Root::update(window, cx, |root, window, cx| {
        root.open_dialog(
            move |dialog, window, cx| {
                let view = cx.new(|cx| CommandPalette::new(window, cx));
                view.update(cx, |palette, cx| palette.focus(window, cx));
                dialog
                    .w(px(PALETTE_WIDTH))
                    .margin_top(px(MARGIN_TOP))
                    // 命令面板不需要右上角的关闭按钮：Esc 与点击遮罩都能关
                    .close_button(false)
                    .overlay(true)
                    .overlay_closable(true)
                    .content(move |content, _, _| content.child(view.clone()))
            },
            window,
            cx,
        )
    });
}

/// 关闭命令面板
fn close_palette(window: &mut Window, cx: &mut App) {
    Root::update(window, cx, |root, window, cx| {
        root.close_dialog(window, cx);
    });
}

/// 扫描库内可打开的文件（在后台线程执行）。
///
/// 只收 [`is_editable_text`] 命中的文件：与目录树、打开入口用同一判据，
/// 否则面板里会出现"选了却打不开"的条目。
fn collect_candidates(root: &Path) -> Result<Vec<Candidate>, String> {
    let vault = Vault::open(root.to_path_buf()).map_err(|err| err.to_string())?;
    let tree = vault.scan(false).map_err(|err| err.to_string())?;

    let mut out = Vec::new();
    flatten(root, &tree, &mut out);
    out.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(out)
}

/// 展平目录树；`label` 用相对库根的路径（含扩展名）。
fn flatten(root: &Path, node: &FileNode, out: &mut Vec<Candidate>) {
    if !node.is_dir {
        if is_editable_text(&node.path) {
            let label = node
                .path
                .strip_prefix(root)
                .unwrap_or(node.path.as_path())
                .to_string_lossy()
                .to_string();
            out.push(Candidate {
                label,
                path: node.path.clone(),
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
