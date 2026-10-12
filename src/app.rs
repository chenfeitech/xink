//! 工作区：DockArea 三栏布局（左文件树 / 中编辑 / 右预览）

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::TextViewState;
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::component::theme::{Theme, ThemeColor};
use gpui_kit::*;

use crate::actions::OpenSettings;
use crate::core::config::{AppConfig, ThemeMode};
use crate::core::vault::VaultRegistry;
use crate::core::{resolve_active, tab_key};
use crate::state::workspace;
use crate::ui::{EditorPanel, FilesPanel, OpenTabs, PreviewPanel, tabs};

/// 预览调度中心。
///
/// 关键点：**每篇笔记持有自己的 TextViewState**，切换标签时整体替换引用，
/// 而不是把新内容 set_text 进同一个文档——后者会触发整篇 Markdown 重新解析，
/// 是切换标签卡顿的根源。
pub struct PreviewState {
    /// 当前活动的预览文档
    pub active_doc: Option<Entity<TextViewState>>,
    /// 图片预览：点击笔记里的图片或目录树中的图片文件时，预览面板改为显示这张图。
    ///
    /// 这里是"复用同一个预览面板"而不是新开窗口：预览位置一致、不产生窗口冗余。
    /// 切换/编辑笔记时会被清空，自动回到 Markdown。
    pub image: Option<PathBuf>,
}

/// 活动预览文档发生变化（切换笔记标签）
pub struct ActiveDocChanged(pub Entity<TextViewState>);

/// 预览面板要显示的图片发生变化；`None` 表示回到 Markdown 预览
pub struct PreviewImageChanged(pub Option<PathBuf>);

/// 编辑器滚动位置变化，参数为滚动比例（0~1）。
/// 由 EditorPanel 发出，PreviewPanel 据此同步预览滚动。
pub struct EditorScrolled(pub f32);

/// 预览滚动位置变化，参数为滚动比例（0~1）。
/// 由 PreviewPanel 发出，EditorPanel 据此同步编辑器滚动。
pub struct PreviewScrolled(pub f32);

impl EventEmitter<ActiveDocChanged> for PreviewState {}
impl EventEmitter<EditorScrolled> for PreviewState {}
impl EventEmitter<PreviewScrolled> for PreviewState {}
impl EventEmitter<PreviewImageChanged> for PreviewState {}

/// 让预览面板显示一张图片（点击图片时用）
pub fn show_preview_image(path: PathBuf, cx: &mut App) {
    cx.update_global::<AppState, _>(|state, cx| {
        state.preview.update(cx, |preview, cx| {
            preview.image = Some(path.clone());
            cx.emit(PreviewImageChanged(Some(path)));
        });
    });
}

/// 跨面板共享的应用状态。
/// 文件树需要往中间区域添加笔记标签页，因此持有 dock_area 的引用。
pub struct AppState {
    pub dock_area: Entity<DockArea>,
    pub preview: Entity<PreviewState>,
    /// 已打开的笔记标签：按路径去重（同一文件只有一个实例）、按打开顺序排列，
    /// 并记录当前活动标签。详见 `docs/DESIGN.md` §2.5。
    pub tabs: OpenTabs,
    /// 空白编辑页（未打开任何笔记时的占位）。
    ///
    /// 布局恢复时会用工厂重建保存过的 EditorPanel，而它们没有笔记状态，
    /// 整批都是空白。这里集中登记：供笔记"认领"，认领不完的只保留一个。
    pub empty_panels: Vec<WeakEntity<EditorPanel>>,
    /// 应用配置（基本 / 编辑器 / 预览三类）。
    ///
    /// 修改请统一走 [`update_config`]：它会校正取值、落盘并重绘窗口，
    /// 保证"改完立即生效、无需重启"。
    pub config: AppConfig,
    /// 当前是否为深色主题（含"跟随系统"解析后的结果）。
    ///
    /// 由 [`apply_theme`] 维护：框架没有"查询当前明暗"的 API，
    /// 而代码块高亮主题需要据此选择，因此在这里记录一次供各处读取。
    pub dark: bool,
    /// 已注册的库与当前选择（多库机制，见 `docs/multi-vault-design.md`）。
    ///
    /// 库的当前路径统一由 `vaults.active_root()` 派生，避免两处状态不一致。
    pub vaults: VaultRegistry,
}

impl Global for AppState {}

impl AppState {
    /// 取出一个仍存活的空白页给笔记使用（取走后不再计入）
    pub fn take_empty_panel(&mut self) -> Option<Entity<EditorPanel>> {
        while let Some(weak) = self.empty_panels.pop() {
            if let Some(panel) = weak.upgrade() {
                return Some(panel);
            }
        }
        None
    }

    /// 把空白页还回去（认领后载入失败时使用，维持"无笔记时恰好一个空白页"）
    pub fn return_empty_panel(&mut self, panel: &Entity<EditorPanel>) {
        if !self.is_empty_panel(panel) {
            self.empty_panels.push(panel.downgrade());
        }
    }

    /// 取消一个面板的"空白页"身份。
    ///
    /// 新建的面板在构造时就登记为空白页；一旦它要承载笔记，必须立刻撤销该身份，
    /// 否则 `sync_placeholder` 会把它当占位页直接从 dock 里移除。
    pub fn forget_empty_panel(&mut self, panel: &Entity<EditorPanel>) {
        let id = panel.entity_id();
        self.empty_panels.retain(|weak| match weak.upgrade() {
            Some(existing) => existing.entity_id() != id,
            None => false,
        });
    }

    /// 是否存在仍存活的空白页
    pub fn has_empty_panel(&self) -> bool {
        self.empty_panels
            .iter()
            .any(|weak| weak.upgrade().is_some())
    }

    fn is_empty_panel(&self, panel: &Entity<EditorPanel>) -> bool {
        let id = panel.entity_id();
        self.empty_panels
            .iter()
            .filter_map(|weak| weak.upgrade())
            .any(|existing| existing.entity_id() == id)
    }
}

/// 读取当前配置。配置结构体很小，直接克隆一份，避免长期持有全局借用。
pub fn config(cx: &App) -> AppConfig {
    cx.try_global::<AppState>()
        .map(|state| state.config.clone())
        .unwrap_or_default()
}

/// 修改配置的**唯一入口**。
///
/// 调用后依次完成：应用改动 → 校正越界值 → 落盘 → 应用主题 → 重绘所有窗口。
/// 因此任何配置变更都是"即时生效"，不需要重启应用。
pub fn update_config(cx: &mut App, f: impl FnOnce(&mut AppConfig)) {
    if cx.try_global::<AppState>().is_none() {
        return;
    }
    cx.update_global::<AppState, _>(|state, cx| {
        f(&mut state.config);
        state.config.sanitize();
        if let Err(err) = state.config.save() {
            eprintln!("[xink] 保存配置失败: {err:?}");
        }
        apply_theme(None, cx);
        // 让所有窗口重绘，新配置立刻可见
        cx.refresh_windows();
    });
}

/// 应用主题配置：System 跟随系统外观，Light / Dark 强制切换。
pub fn apply_theme(window: Option<&mut Window>, cx: &mut App) {
    let mode = cx
        .try_global::<AppState>()
        .map(|state| state.config.basic.theme)
        .unwrap_or(ThemeMode::System);

    let dark = match mode {
        ThemeMode::System => {
            // 先取外观再同步：`sync_system_appearance` 要拿走 window 的可变借用
            let appearance = window
                .as_ref()
                .map(|w| w.appearance())
                .unwrap_or_else(|| cx.window_appearance());
            Theme::sync_system_appearance(window, cx);
            matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            )
        }
        ThemeMode::Light => {
            Theme::global_mut(cx).colors = *ThemeColor::light();
            false
        }
        ThemeMode::Dark => {
            Theme::global_mut(cx).colors = *ThemeColor::dark();
            true
        }
    };

    // 记录解析结果：代码块高亮主题等需要"当前是否深色"。
    //
    // 必须判空：本函数也会被"系统外观变化"等窗口事件调用，而那些事件可能早于
    // `AppState` 的注册（`set_global`）。此时 `update_global` 会因找不到 global 直接 panic。
    // 未注册时跳过即可——随后启动流程会再调用一次本函数补上。
    if cx.try_global::<AppState>().is_some() {
        cx.update_global::<AppState, _>(|state, _| state.dark = dark);
    }
}

/// 当前是否为深色主题（"跟随系统"已解析为具体结果）。
pub fn is_dark(cx: &App) -> bool {
    cx.try_global::<AppState>()
        .map(|state| state.dark)
        .unwrap_or(true)
}

/// 空白页同步。
///
/// 不变式：
/// - 一篇笔记都没打开 → 编辑区**恰好**存在一个空白页
/// - 只要打开了笔记 → 空白页不存在
///
/// 所有改变"已打开笔记集合"的地方（打开、关闭、启动恢复）都必须调用它，
/// 由它单点维护，避免各处各自为政导致出现多个空白页。
pub fn sync_placeholder(window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };
    let has_notes = !state.tabs.is_empty();
    let has_empty = state.has_empty_panel();
    let dock_area = state.dock_area.clone();

    match (has_notes, has_empty) {
        // 有笔记却还留着空白页 → 全部移除
        (true, true) => {
            let mut removing = Vec::new();
            cx.update_global::<AppState, _>(|state, _| {
                state.empty_panels.retain(|weak| match weak.upgrade() {
                    Some(panel) => {
                        removing.push(panel);
                        false
                    }
                    None => false,
                });
            });
            for panel in removing {
                dock_area.update(cx, |area, cx| {
                    area.remove_panel(panel, window, cx);
                });
            }
        }
        // 一篇笔记都没有，且空白页缺失 → 补一个
        (false, false) => {
            let panel = cx.new(|cx| EditorPanel::new(window, cx));
            cx.update_global::<AppState, _>(|state, _| {
                state.empty_panels.push(panel.downgrade());
            });
            dock_area.update(cx, |area, cx| {
                area.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
            });
        }
        // 其余情况状态已经正确
        _ => {}
    }

    prune_empty_panels(window, cx);
}

/// 只保留一个空白页，移除其余。
///
/// 只在事件回调等**安全时机**调用：绝不能在渲染过程中改动布局结构，
/// 那会导致 non-unwinding panic。
pub fn prune_empty_panels(window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };
    if state.empty_panels.len() <= 1 {
        return;
    }
    let dock_area = state.dock_area.clone();

    // 保留第一个仍存活的空白页
    let Some(keep) = state
        .empty_panels
        .iter()
        .find_map(|weak| weak.upgrade())
        .map(|panel| panel.entity_id())
    else {
        return;
    };

    let mut removing = Vec::new();
    cx.update_global::<AppState, _>(|state, _| {
        state.empty_panels.retain(|weak| match weak.upgrade() {
            Some(panel) if panel.entity_id() == keep => true,
            Some(panel) => {
                removing.push(panel);
                false
            }
            None => false,
        });
    });

    if !removing.is_empty() {
        println!("[xink] 清理 {} 个多余空白页", removing.len());
    }
    for panel in removing {
        dock_area.update(cx, |area, cx| {
            area.remove_panel(panel, window, cx);
        });
    }
}

const MAIN_DOCK_ID: &str = "main-dock";
/// 布局 schema 版本。默认布局结构变更时递增，旧布局将无法恢复并回落默认值。
const MAIN_DOCK_VERSION: usize = 2;

pub struct Workspace {
    dock_area: Entity<DockArea>,
    _dock_skin: Rc<DockSkin>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (dock_area, skin) =
            DockSkin::dock_area(MAIN_DOCK_ID, Some(MAIN_DOCK_VERSION), window, cx);

        let preview = cx.new(|_| PreviewState {
            active_doc: None,
            image: None,
        });

        // 库注册表：必须先确定当前库，再恢复标签（标签要按当前库过滤）。
        //
        // 校验只做提示，绝不阻塞启动——与配置、布局、标签恢复的容错策略一致。
        let mut vaults = workspace::load_vaults();
        for note in vaults.resolve_active() {
            println!("[xink] {note}");
        }
        // 回落 / 迁移的结果落盘，下次启动不必重算
        if let Err(err) = workspace::save_vaults(&vaults) {
            eprintln!("[xink] 保存库列表失败: {err:?}");
        }

        cx.set_global(AppState {
            dock_area: dock_area.clone(),
            preview,
            tabs: OpenTabs::new(),
            empty_panels: Vec::new(),
            config: AppConfig::load(),
            // 初值，随后由 apply_theme 按主题模式与系统外观校正
            dark: true,
            vaults,
        });

        // 主题等外观类配置在启动时先应用一次
        apply_theme(Some(window), cx);

        match workspace::load_layout() {
            // 布局结构变更过（版本不同）：旧布局不再适用，重建默认布局，
            // 否则用户会停留在旧的混合结构上。
            Ok(state) if state.version != Some(MAIN_DOCK_VERSION) => {
                println!("[xink] 布局结构已变更，重建默认布局");
                Self::reset_default_layout(&dock_area, window, cx);
                Self::persist(&dock_area, cx);
            }
            Ok(state) => match dock_area.update(cx, |area, cx| area.load(state, window, cx)) {
                Ok(_) => println!("[xink] 已恢复上次布局"),
                Err(err) => {
                    eprintln!("[xink] 恢复布局失败，回落默认布局: {err:?}");
                    Self::reset_default_layout(&dock_area, window, cx);
                }
            },
            Err(_) => {
                // 首次启动：生成默认布局并立即持久化
                Self::reset_default_layout(&dock_area, window, cx);
                Self::persist(&dock_area, cx);
            }
        }

        // 布局恢复后再恢复上次打开的笔记
        Self::restore_open_notes(&dock_area, window, cx);
        // 恢复完笔记，按不变式决定空白页的存在与否
        sync_placeholder(window, cx);

        // 注意：`Window::on_action` 只能在 paint 阶段注册，因此"打开设置"的
        // 动作绑定放在 `render` 的根元素上（见 Render::render）。

        cx.subscribe_in(
            &dock_area,
            window,
            |this, dock_area, ev: &DockEvent, window, cx| {
                if matches!(ev, DockEvent::LayoutChanged) {
                    this.save_layout(dock_area, window, cx);
                }
            },
        )
        .detach();

        // 退出前再存一次，避免最后一次改动因防抖未落盘而丢失
        cx.on_app_quit({
            let dock_area = dock_area.clone();
            move |_, cx| {
                let state = dock_area.read(cx).dump(cx);
                // 退出前再存一次打开的标签，确保活动标签是最新的
                Self::save_open_notes(cx);
                cx.background_executor().spawn(async move {
                    if let Err(err) = workspace::save_layout(&state) {
                        eprintln!("保存布局失败: {err:?}");
                    }
                })
            }
        })
        .detach();

        Self {
            dock_area,
            _dock_skin: skin,
        }
    }

    fn save_layout(
        &mut self,
        dock_area: &Entity<DockArea>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::persist(dock_area, cx);
    }

    fn persist(dock_area: &Entity<DockArea>, cx: &mut App) {
        let state = dock_area.read(cx).dump(cx);
        if let Err(err) = workspace::save_layout(&state) {
            eprintln!("[xink] 保存布局失败: {err:?}");
        }
        Self::save_open_notes(cx);
    }

    /// 记录当前仍打开的笔记（顺序 + 活动标签），供下次启动恢复
    fn save_open_notes(cx: &mut App) {
        tabs::persist_open_notes(cx);
    }

    /// 恢复上次打开的笔记。
    ///
    /// 走的是与手动打开**完全相同**的 [`tabs::open_note`] 入口，
    /// 因此恢复同样受去重规则约束：历史记录里若有重复路径，最终也只有一个标签。
    ///
    /// 时机必须是"布局恢复之后、`sync_placeholder` 之前"：
    /// 布局恢复会重建出一批空白 `EditorPanel`，先让它们存在才能被认领；
    /// 认领完再由 `sync_placeholder` 决定空白页的去留——
    /// 只要成功恢复出一篇笔记，就不会显示空白编辑页。
    fn restore_open_notes(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let session = workspace::load_open_notes();
        // 只恢复属于当前库的标签：`open_notes.json` 里可能残留其它库的路径，
        // 全部恢复会让切换库后仍看到别的库的笔记。
        let active_root = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root());
        let paths: Vec<PathBuf> = match &active_root {
            Some(root) => session
                .paths
                .into_iter()
                .filter(|p| p.starts_with(root))
                .collect(),
            None => Vec::new(),
        };

        if paths.is_empty() {
            println!("[xink] 没有属于当前库的历史标签，显示空白编辑页");
            return;
        }
        println!("[xink] 恢复 {} 个标签页", paths.len());

        let mut restored = Vec::new();
        for path in &paths {
            if tabs::open_note(path.clone(), window, cx) {
                restored.push(tab_key(path));
            } else {
                eprintln!("[xink] 恢复失败，跳过: {}", path.display());
            }
        }

        // 全部恢复完再激活，避免边建边切导致预览被反复重建
        let active = resolve_active(session.active.as_ref().map(|path| tab_key(path)), &restored);
        if let Some(panel) = active.and_then(|key| {
            cx.try_global::<AppState>()
                .and_then(|state| state.tabs.get(&key))
        }) {
            tabs::reveal_panel(dock_area, &panel, window, cx);
        }

        // 写回一次：恢复失败的条目从记录里消失（自愈），下次不再白试
        Self::save_open_notes(cx);
    }

    /// 打开设置对话框（Cmd-,）。
    ///
    /// 由 [`Render::render`] 里根元素上的 `.on_action` 触发；动作从子面板冒泡到这里。
    fn open_settings(&mut self, _: &OpenSettings, _window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::open_settings(cx);
    }

    fn reset_default_layout(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let files = cx.new(|cx| FilesPanel::new(cx));
        let editor = cx.new(|cx| EditorPanel::new(window, cx));
        let preview = cx.new(|cx| PreviewPanel::new(window, cx));

        // 编辑区独占 center：所有笔记标签都开在这里，不会跑到预览去
        let center = DockLayout::tabs().panel_view(panel_handle(editor), cx);
        // 预览是独立的右侧 dock，与编辑区职责分离
        let right = DockLayout::tabs().panel_view(panel_handle(preview), cx);

        let left = DockLayout::v_split()
            .child(DockLayout::tabs().panel_view(panel_handle(files), cx), None);

        dock_area.update(cx, |view, cx| {
            view.set_center(center, window, cx);
            view.set_dock(DockPlacement::Left, left, window, cx);
            view.set_dock_size(DockPlacement::Left, px(260.), window, cx);
            view.set_dock(DockPlacement::Right, right, window, cx);
            view.set_dock_size(DockPlacement::Right, px(420.), window, cx);
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = gpui_kit::component::Root::render_sheet_layer(window, cx);
        let dialog_layer = gpui_kit::component::Root::render_dialog_layer(window, cx);
        let notification_layer = gpui_kit::component::Root::render_notification_layer(window, cx);

        div()
            .id("workspace")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
            // 打开设置（Cmd-,）：必须在渲染阶段注册，动作由子面板冒泡至此
            .on_action(cx.listener(Self::open_settings))
    }
}
