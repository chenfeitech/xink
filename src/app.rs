//! 工作区：DockArea 三栏布局（左文件树 / 中编辑 / 右预览）

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::TextViewState;
use gpui_kit::component::dock::{
    DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
};
use gpui_kit::component::theme::{Theme, ThemeMode as KitThemeMode};
use gpui_kit::*;

use crate::actions::{OpenCommandPalette, OpenSearch, OpenSettings, ToggleAiChat, ToggleTheme};
use std::path::Path;

use crate::core::config::{AppConfig, ThemeMode};
// `Action` 与 gpui 的 `Action` trait 同名（上面 `use gpui_kit::*` 会带进来），
// 因此取个别名，避免"看起来是类型、其实是 trait"的编译错误
use crate::core::ai::prompt::Action as AiAction;
use crate::core::index::{IndexChanged, NoteIndex};
use crate::core::sync::{AnchorTable, ScrollAnchor};
use crate::core::vault::VaultRegistry;
use crate::core::{resolve_active, tab_key};
use crate::state::workspace;
use crate::ui::outline::OutlineState;
use crate::ui::{EditorPanel, FilesPanel, OpenTabs, PreviewPanel, RailState, tabs};

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
    /// 当前活动文档的**滚动锚点表**：顶层块 ↔ 源码行。
    ///
    /// 由活动编辑器在刷新预览时写入（预览面板拿不到源文本，只能从这里取）。
    /// 为 `None` 表示当前文档无法按行映射（数据文件），两端退回比例同步。
    pub anchors: Option<Arc<AnchorTable>>,
    /// 调试用：两端最近一次读到的锚点数值（编辑区, 预览区），供标题栏显示
    pub debug_anchors: Option<(f32, f32)>,
}

/// 活动预览文档发生变化（切换笔记标签）
pub struct ActiveDocChanged(pub Entity<TextViewState>);

/// 预览面板要显示的图片发生变化；`None` 表示回到 Markdown 预览
pub struct PreviewImageChanged(pub Option<PathBuf>);

/// 编辑器滚动位置变化，参数是滚动锚点（Markdown 为源码行号，否则为比例）。
/// 由 EditorPanel 发出，PreviewPanel 据此同步预览滚动。
pub struct EditorScrolled(pub ScrollAnchor);

/// 预览滚动位置变化，参数是滚动锚点（Markdown 为源码行号，否则为比例）。
/// 由 PreviewPanel 发出，EditorPanel 据此同步编辑器滚动。
pub struct PreviewScrolled(pub ScrollAnchor);

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
    /// 文件树面板的弱引用，供"活动笔记变化时把树定位过去"使用。
    ///
    /// 文件树是 dock 面板、由两处创建（默认布局与布局恢复的面板工厂），
    /// 不像 `outline` / `rail` 那样能在 `set_global` 时直接持有，
    /// 因此由 `FilesPanel::new` 自注册——与 `EditorPanel` 登记 `empty_panels`
    /// 同一套做法。未创建 / 已释放时为 `None`，调用方按"没有就算了"处理。
    pub files: Option<WeakEntity<FilesPanel>>,
    /// 已注册的库与当前选择（多库机制，见 `docs/multi-vault-design.md`）。
    ///
    /// 库的当前路径统一由 `vaults.active_root()` 派生，避免两处状态不一致。
    pub vaults: VaultRegistry,
    /// 当前笔记的大纲（编辑器的派生状态，见 `docs/outline-design.md`）。
    pub outline: Entity<OutlineState>,
    /// 全库索引（双链 / 反链 / 标签 / 搜索的共用底座）。
    ///
    /// 是磁盘的派生数据，不落盘：启动与切库时后台重建，
    /// 单文件保存与监听事件做增量更新。详见 `docs/note-index-design.md`。
    pub notes: Entity<NoteIndex>,
    /// 最右侧按钮条的状态：当前展开哪个抽屉（大纲 / 反链 / 标签）。
    ///
    /// 抽屉本身是瞬时 UI、不落盘，这里只记"按钮高亮"，
    /// 由 `ui::drawer` 在关闭时清回 None。见 `docs/right-rail-drawer-design.md`。
    pub rail: Entity<RailState>,
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

/// 后台重建全库索引（启动、切换库时调用）。
///
/// 构建是同步 IO（读全库笔记），必须在后台线程做；完成前旧索引继续服务，
/// 因此大库启动不会被索引拖慢。设计见 `docs/note-index-design.md` §3。
pub fn rebuild_index(cx: &mut App) {
    let Some((root, notes)) = cx.try_global::<AppState>().and_then(|state| {
        state
            .vaults
            .active_root()
            .map(|root| (root, state.notes.clone()))
    }) else {
        return;
    };

    let task = cx.background_spawn(async move { NoteIndex::build(&root) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            notes.update(cx, |index, cx| {
                match result {
                    Ok(built) => {
                        println!("[xink] 索引已构建: {} 篇笔记", built.len());
                        *index = built;
                        cx.emit(IndexChanged);
                    }
                    Err(err) => eprintln!("[xink] 构建索引失败: {err}"),
                }
                cx.notify();
            });
        });
    })
    .detach();
}

/// 笔记保存后重解析。
///
/// 内容变更只产生 `Modified` 事件、不会触发结构重扫，因此必须由保存处显式挂钩，
/// 否则改完 `[[双链]]` 与 `#标签` 后索引还是旧的。
pub fn index_note_saved(path: &Path, cx: &mut App) {
    let Some(notes) = cx.try_global::<AppState>().map(|s| s.notes.clone()) else {
        return;
    };
    notes.update(cx, |index, cx| {
        index.reindex_file(path);
        cx.emit(IndexChanged);
        cx.notify();
    });
}

/// 笔记被删除后从索引里剔除
pub fn index_note_removed(path: &Path, cx: &mut App) {
    let Some(notes) = cx.try_global::<AppState>().map(|s| s.notes.clone()) else {
        return;
    };
    notes.update(cx, |index, cx| {
        index.remove_file(path);
        cx.emit(IndexChanged);
        cx.notify();
    });
}

/// 修改配置的**唯一入口**。
///
/// 调用后依次完成：应用改动 → 校正越界值 → 落盘 → 应用主题 → 重绘所有窗口。
/// 因此任何配置变更都是"即时生效"，不需要重启应用。
pub fn update_config(cx: &mut App, f: impl FnOnce(&mut AppConfig)) {
    if cx.try_global::<AppState>().is_none() {
        return;
    }
    cx.update_global::<AppState, _>(|state, _cx| {
        f(&mut state.config);
        state.config.sanitize();
        if let Err(err) = state.config.save() {
            eprintln!("[xink] 保存配置失败: {err:?}");
        }
    });

    // `apply_theme` 必须等上面的租借结束再调。
    //
    // `update_global` 是"租借"语义：`App::lease_global` 会先把 AppState 从
    // globals 表里 **remove**（gpui app.rs:2147），闭包跑完才 insert 回去。
    // 因此在闭包里 `cx.try_global::<AppState>()` 恒为 None，`apply_theme` 会
    // 退化成 `ThemeMode::System`——用户选的浅色/深色被系统外观顶掉，表现就是
    // "切换了没反应"；连带 `AppState.dark` 也写不进去（同样判空失败），
    // 于是 `toggle_theme` 拿着过期的 dark 继续算，越切越乱。
    apply_theme(None, cx);
    // 让所有窗口重绘，新配置立刻可见
    cx.refresh_windows();
}

/// 从命令面板跑一个内置 AI 动作：作用于当前活动标签的笔记（需求 A9.1）
///
/// 与 `Workspace::toggle_ai_chat` 同一条路由。之所以是模块级函数而不是
/// 面板的方法：命令面板的 `cx` 是它自己的 `Context`，只能拿到 `&mut App`。
pub fn run_ai_action(action: AiAction, window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };
    let Some(path) = state.tabs.active().cloned() else {
        return;
    };
    let Some(panel) = state.tabs.get(&path) else {
        return;
    };
    panel.update(cx, |panel, cx| panel.run_ai_action(action, window, cx));
}

/// 从命令面板跑一条 skill：作用于当前活动标签的笔记（需求 A5.3）
///
/// 与 [`run_ai_action`] 同构：skill 是运行时发现的，命令面板只记得名字，
/// 正文在执行时由 `AiChat::run_skill` 重新发现一遍（期间被删掉会提示）。
pub fn run_ai_skill(name: &str, window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };
    let Some(path) = state.tabs.active().cloned() else {
        return;
    };
    let Some(panel) = state.tabs.get(&path) else {
        return;
    };
    panel.update(cx, |panel, cx| panel.run_ai_skill(name, window, cx));
}

/// 应用主题配置：System 跟随系统外观，Light / Dark 强制切换。
///
/// **必须走 [`Theme::change`]**，不能只改 `Theme::global_mut(cx).colors`：
/// 后者只覆盖 component 层的色表，`theme.mode` 仍是旧值（`is_dark()` 分叉出的
/// 背景/输入背景就还是另一套明暗），Base 层 `gpui_base::Theme` 也不会同步
/// （框架在 `Theme::global_mut` 的文档里明确要求改完跑 `sync_base`），
/// 字体与 TextView 默认样式同样不刷新。实测结果是"浅底 + 深色主题的浅色前景"：
/// 文字看不见、图标像消失了一样。
///
/// 另一个坑：**不要在 `cx.update_global::<AppState, _>(…)` 的闭包里调用本函数。**
/// `update_global` 是租借语义（global 会被移出表），闭包内 `try_global` 恒为 None，
/// 主题模式会静默退化成 System。见 [`update_config`] 里的说明。
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
            // 内部就是 `Theme::change(appearance, …)`
            Theme::sync_system_appearance(window, cx);
            matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            )
        }
        ThemeMode::Light => {
            Theme::change(KitThemeMode::Light, window, cx);
            false
        }
        ThemeMode::Dark => {
            Theme::change(KitThemeMode::Dark, window, cx);
            true
        }
    };

    // 抽屉要上下撑满整个界面：`Placement::Right` 的顶边取自
    // `theme.sheet.margin_top`，默认是标题栏高度（`sheet.rs:144`）。
    // 每次 `Theme::change` 之后都要重新压成 0，否则会被主题配置覆盖回去。
    Theme::global_mut(cx).sheet.margin_top = px(0.);

    // 高亮主题：编辑区与预览区共用同一份预置主题。
    //
    // 必须放在 `Theme::change` **之后**：它会把 `highlight_theme` 重置回框架默认，
    // 跟上面 `sheet.margin_top` 是同一个坑。
    // 未命中（主题名写错 / 资源损坏）时保持框架默认，不覆盖。
    let theme_name = cx
        .try_global::<AppState>()
        .map(|state| state.config.highlight.theme.clone())
        .unwrap_or_default();
    if let Some(highlight) = crate::ui::theme::highlight_theme(
        crate::core::theme::normalize_theme_name(&theme_name),
        dark,
    ) {
        Theme::global_mut(cx).highlight_theme = highlight;
    }

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
/// 递增到 6：右侧 dock 只保留「预览」，大纲 / 反链 / 标签改为最右侧按钮条 +
/// 抽屉，旧布局不再适用。详见 `docs/right-rail-drawer-design.md`。
const MAIN_DOCK_VERSION: usize = 6;

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
            anchors: None,
            debug_anchors: None,
        });
        let notes = cx.new(|_| NoteIndex::new());
        let outline = cx.new(|_| OutlineState::new());
        let rail = cx.new(|_| RailState::new());

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
            outline,
            notes,
            rail,
            files: None,
        });

        // 主题等外观类配置在启动时先应用一次
        apply_theme(Some(window), cx);

        // 索引在后台构建：启动不被大库拖慢（设计 §3）
        rebuild_index(cx);

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
    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::open_settings(window, cx);
    }

    /// 在浅色 / 深色之间切换主题（Cmd-Shift-L）。
    ///
    /// 走 `update_config` 而不是直接改 Theme：配置要落盘，重启后保持用户的选择。
    fn toggle_theme(&mut self, _: &ToggleTheme, _window: &mut Window, cx: &mut Context<Self>) {
        let (mode, dark) = match cx.try_global::<AppState>() {
            Some(state) => (state.config.basic.theme, state.dark),
            None => return,
        };
        let next = mode.toggled(dark);
        update_config(cx, |cfg| cfg.basic.theme = next);
    }

    /// 打开命令面板（Cmd+P），由根元素上的 `.on_action` 触发。
    fn open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::open_command_palette(window, cx);
    }

    /// 打开全文搜索（Cmd-Shift-F），同样注册在根元素上。
    fn open_search(&mut self, _: &OpenSearch, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::open_search(window, cx);
    }

    /// 打开 / 关闭编辑器下方的 AI 对话条（Cmd-Shift-A）
    ///
    /// 对话条属于某个编辑页（聊天对象就是那一篇笔记），因此路由到当前活动标签；
    /// 没有打开的笔记时什么都不做——AI 没有可作用的对象。
    fn toggle_ai_chat(&mut self, _: &ToggleAiChat, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = cx.try_global::<AppState>() else {
            return;
        };
        let Some(path) = state.tabs.active().cloned() else {
            return;
        };
        let Some(panel) = state.tabs.get(&path) else {
            return;
        };
        panel.update(cx, |panel, cx| panel.toggle_ai_chat(window, cx));
    }

    fn reset_default_layout(dock_area: &Entity<DockArea>, window: &mut Window, cx: &mut App) {
        let files = cx.new(FilesPanel::new);
        let editor = cx.new(|cx| EditorPanel::new(window, cx));
        let preview = cx.new(|cx| PreviewPanel::new(window, cx));

        // 编辑区独占 center：所有笔记标签都开在这里，不会跑到预览去
        let center = DockLayout::tabs().panel_view(panel_handle(editor), cx);
        // 右侧 dock 只放预览：大纲 / 反链 / 标签已改为最右侧按钮条 + 抽屉
        // （`docs/right-rail-drawer-design.md`）
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-kit 0.7 起 `Root` 自己托管浮层：sheet / dialog / notification
        // 三层由 Root 在窗口层渲染，手动的 `render_*_layer` 已被删除。
        // 最右侧按钮条（大纲 / 反链 / 标签）
        let rail = cx
            .try_global::<AppState>()
            .map(|state| state.rail.clone().into_any_element());

        div()
            .id("workspace")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            // 按钮条是绝对定位的浮层，不占布局宽度；z 序由 Root 托管的浮层决定，
            // 模态（命令面板等）打开时会自动盖在它上面。
            .children(rail)
            // 打开设置（Cmd-,）：必须在渲染阶段注册，动作由子面板冒泡至此
            .on_action(cx.listener(Self::open_settings))
            // 切换主题（Cmd-Shift-L）：同上，注册在根元素上以覆盖任意焦点位置
            .on_action(cx.listener(Self::toggle_theme))
            // 命令面板（Cmd-P）：同理必须注册在根元素，否则焦点在编辑区时收不到
            .on_action(cx.listener(Self::open_command_palette))
            // 全文搜索（Cmd-Shift-F）
            .on_action(cx.listener(Self::open_search))
            // AI 面板（Cmd-Shift-A）
            .on_action(cx.listener(Self::toggle_ai_chat))
    }
}
