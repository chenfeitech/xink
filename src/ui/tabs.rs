//! 编辑区标签：注册表 + 打开笔记的唯一入口
//!
//! 两条规则在此落地，任何"打开一篇笔记"的需求都必须走 [`open_note`]，不得自建打开逻辑：
//! 1. 同一文件只允许一个标签，重复打开只是激活已有标签；
//! 2. 已打开的标签按打开顺序持久化，启动时据此恢复（顺序 + 活动标签）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::base::dock::PanelId;
use gpui_kit::component::dock::{DockArea, DockPlacement, panel_handle};
use gpui_kit::*;

use crate::app::{AppState, sync_placeholder};
use crate::core::media::is_editable_text;
use crate::core::tab_key;
use crate::ui::EditorPanel;

/// 已打开的笔记标签。
///
/// `order` 是顺序的唯一来源（也是落盘与恢复的顺序），`panels` 只负责按路径查找。
/// 二者分开是因为 `HashMap` 的迭代顺序不确定——若直接拿它落盘，每次保存得到的
/// 标签顺序都不同，恢复顺序就会随机漂移。
///
/// 面板以 `WeakEntity` 持有：标签关闭后引用自动失效，
/// [`OpenTabs::len`] / [`OpenTabs::keys`] 只统计仍存活的标签，无需各处手动清理状态。
pub struct OpenTabs {
    order: Vec<PathBuf>,
    panels: HashMap<PathBuf, WeakEntity<EditorPanel>>,
    active: Option<PathBuf>,
}

impl Default for OpenTabs {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenTabs {
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            panels: HashMap::new(),
            active: None,
        }
    }

    /// 是否一篇笔记都没打开（已关闭的标签不计入）
    pub fn is_empty(&self) -> bool {
        self.keys().is_empty()
    }

    /// 仍存活的标签数量
    pub fn len(&self) -> usize {
        self.keys().len()
    }

    /// 该路径对应的标签；面板已关闭则视为未打开。参数必须是 [`tab_key`] 的结果
    pub fn get(&self, key: &Path) -> Option<Entity<EditorPanel>> {
        self.panels.get(key)?.upgrade()
    }

    /// 登记一个标签并置为活动。已存在则只更新面板与活动标记，顺序保持不变。
    pub fn insert(&mut self, key: PathBuf, panel: &Entity<EditorPanel>) {
        if !self.panels.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.panels.insert(key.clone(), panel.downgrade());
        self.active = Some(key);
    }

    /// 移除一个标签（关闭 / 清空为空白页时）
    pub fn remove(&mut self, key: &Path) {
        self.panels.remove(key);
        self.order.retain(|k| k != key);
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
    }

    /// 标记活动标签。路径未登记时忽略
    pub fn activate(&mut self, key: &PathBuf) {
        if self.panels.contains_key(key) {
            self.active = Some(key.clone());
        }
    }

    /// 当前活动标签的路径
    pub fn active(&self) -> Option<&PathBuf> {
        self.active
            .as_ref()
            .filter(|key| self.panels.contains_key(*key))
    }

    /// 按打开顺序返回仍存活的标签路径
    pub fn keys(&self) -> Vec<PathBuf> {
        self.alive().map(|(key, _)| key.clone()).collect()
    }

    /// 找出与这批路径对应的面板（供外部变更监听匹配用）。
    /// 事件里的路径未经规范化，因此统一按 [`tab_key`] 比较。
    pub fn panels_for(&self, paths: &[PathBuf]) -> Vec<Entity<EditorPanel>> {
        self.alive()
            .filter(|(key, _)| paths.iter().any(|p| tab_key(p) == **key))
            .filter_map(|(_, weak)| weak.upgrade())
            .collect()
    }

    fn alive(&self) -> impl Iterator<Item = (&PathBuf, &WeakEntity<EditorPanel>)> {
        self.order
            .iter()
            .filter_map(|key| self.panels.get(key).map(|panel| (key, panel)))
            .filter(|(_, panel)| panel.upgrade().is_some())
    }
}

/// 关闭所有不属于 `root` 的标签（切换库时调用）。
///
/// 切换库后旧库的标签不该继续留在界面上——否则会出现
/// "文件树里看不到这篇笔记、标签却还开着"的割裂状态。
///
/// 这里只处理标签注册表与面板，**不改动** `FilesPanel` 的库状态：
/// 后者由它自己按"当前库变化"重新加载。
pub fn close_notes_outside(root: &Path, window: &mut Window, cx: &mut App) {
    let outsiders: Vec<PathBuf> = match cx.try_global::<AppState>() {
        Some(state) => state
            .tabs
            .keys()
            .into_iter()
            .filter(|p| !p.starts_with(root))
            .collect(),
        None => return,
    };
    if outsiders.is_empty() {
        return;
    }

    for path in &outsiders {
        let key = tab_key(path);
        // 与关闭单个标签保持一致：**最后一个面板不移除，而是清空成空白页**。
        // 否则编辑区会一个面板都不剩，DockArea 布局按下标访问空数组会 panic。
        let is_last = cx
            .try_global::<AppState>()
            .map(|state| state.tabs.keys().len() <= 1)
            .unwrap_or(false);
        let panel = cx.try_global::<AppState>().and_then(|s| s.tabs.get(&key));

        if let Some(panel) = panel {
            if is_last {
                panel.update(cx, |editor, cx| editor.clear(window, cx));
            } else {
                let dock_area = cx.try_global::<AppState>().map(|s| s.dock_area.clone());
                if let Some(dock_area) = dock_area {
                    dock_area.update(cx, |area, cx| area.remove_panel(panel, window, cx));
                }
            }
        }
        cx.update_global::<AppState, _>(|state, _| state.tabs.remove(&key));
        crate::app::sync_placeholder(window, cx);
    }

    // 立即写回：否则下次启动又会把别的库的标签恢复出来
    cx.update_global::<AppState, _>(|state, _| {
        if let Err(err) =
            crate::state::workspace::save_open_notes(&state.tabs.keys(), state.tabs.active())
        {
            eprintln!("[xink] 保存标签失败: {err:?}");
        }
    });
}

/// 打开一篇笔记的**唯一入口**。
///
/// 流程：
/// 1. 不是可编辑文本（Markdown / JSON / YAML / TOML）直接忽略；
/// 2. 已打开（`tab_key` 命中）→ 只激活已有标签，不新建、不重新读盘；
/// 3. 未打开 → 优先认领空白占位页，没有才新建面板；
/// 4. 载入成功才登记 + 落盘；失败则回滚（归还空白页 / 丢弃新建面板）。
///
/// 返回是否成功打开（含"激活已有标签"）。
pub fn open_note(path: PathBuf, window: &mut Window, cx: &mut App) -> bool {
    if !is_editable_text(&path) {
        return false;
    }
    let key = tab_key(&path);

    let Some(dock_area) = cx
        .try_global::<AppState>()
        .map(|state| state.dock_area.clone())
    else {
        eprintln!("[xink] AppState 未初始化，无法打开笔记");
        return false;
    };

    // 已打开：激活已有标签即可，绝不新建重复标签，也不重新读盘
    // （重新读盘会丢弃用户尚未保存的编辑）
    if let Some(panel) = cx
        .try_global::<AppState>()
        .and_then(|state| state.tabs.get(&key))
    {
        reveal_panel(&dock_area, &panel, window, cx);
        cx.update_global::<AppState, _>(|state, _| state.tabs.activate(&key));
        persist_open_notes(cx);
        return true;
    }

    // 优先认领空白页，避免凭空多出一个空标签
    let claimed = cx.update_global::<AppState, _>(|state, _| state.take_empty_panel());
    let (panel, created) = match claimed {
        Some(panel) => (panel, false),
        None => {
            let panel = cx.new(|cx| EditorPanel::new(window, cx));
            // 构造时面板把自己登记成了空白页；这里立刻摘掉，
            // 否则随后的 sync_placeholder 会把它当成占位页从 dock 里移除
            cx.update_global::<AppState, _>(|state, _| state.forget_empty_panel(&panel));
            (panel, true)
        }
    };

    // 载入失败：不能把它算作"已打开"，否则空白页永不出现、下次还会尝试恢复它
    if !panel.update(cx, |editor, cx| editor.load_note(window, cx, key.clone())) {
        if !created {
            cx.update_global::<AppState, _>(|state, _| state.return_empty_panel(&panel));
        }
        return false;
    }

    if created {
        let adding = panel.clone();
        dock_area.update(cx, |area, cx| {
            area.add_panel_view(
                panel_handle(adding),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
    }

    cx.update_global::<AppState, _>(|state, _| state.tabs.insert(key, &panel));
    reveal_panel(&dock_area, &panel, window, cx);
    persist_open_notes(cx);
    sync_placeholder(window, cx);
    true
}

/// 把当前仍打开的标签（顺序 + 活动标签）写入 `open_notes.json`。
///
/// 每次集合变化都调用，因此不依赖退出流程也能保证记录是最新的。
pub fn persist_open_notes(cx: &mut App) {
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };
    let paths = state.tabs.keys();
    let active = state.tabs.active().cloned();

    if let Err(err) = crate::state::workspace::save_open_notes(&paths, active.as_ref()) {
        eprintln!("[xink] 保存打开的笔记失败: {err:?}");
    }
}

/// 把面板切到前台并聚焦，但**不**持有它的写租约。
///
/// 必须在 dock 层（而非 `panel.update` 内）选择标签：`select_panel` 会触发 dock 的
/// 同步重渲染，标签栏渲染时用 `read` 读每个面板的标题；若此刻 panel 正被写租约，
/// 就会报 "cannot read EditorPanel while it is already being updated"。
pub fn reveal_panel(
    dock_area: &Entity<DockArea>,
    panel: &Entity<EditorPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    let id = PanelId::from(panel.entity_id());
    dock_area.update(cx, |area, cx| {
        area.select_panel(id, window, cx);
    });
    let handle = panel.read(cx).focus_handle(cx).clone();
    handle.focus(window, cx);
}
