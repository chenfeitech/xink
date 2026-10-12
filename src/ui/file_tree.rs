//! 左侧文件树面板：打开库、浏览目录、新建与删除

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tree::{TreeItem, TreeState, tree};
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, Size};

use crate::core::media::is_image_path;
use gpui_kit::*;

use anyhow::anyhow;

use crate::actions::{DeleteItem, NewFolder, NewNote, OpenFolder, RenameItem};
use crate::app::AppState;
use crate::core::watcher::{WatchEvent, WatchKind};
use crate::core::{FileNode, Vault, VaultWatcher};
use crate::ui::tabs;

pub struct FilesPanel {
    focus_handle: FocusHandle,
    tree_state: Entity<TreeState>,
    vault: Option<Vault>,
    status: SharedString,
    /// 首次 render 时恢复上次打开的库，只做一次
    restored: bool,
    /// 已加载的库根路径。与 `AppState.vaults` 的当前库不一致时重新加载（即切换库）。
    applied_vault_root: Option<PathBuf>,
    /// 库目录监听（外部编辑器改动）
    watcher: Option<VaultWatcher>,
    watcher_task: Option<Task<()>>,
    /// 上次**手动**刷新的时刻，用于抑制连点（扫描虽在后台，连点仍会排队多次）
    last_manual_refresh: Option<Instant>,

    /// 路径 → 是否为目录。
    ///
    /// UI 的 `TreeItem` 只保留 id/名称/子节点，不携带"是不是目录"，
    /// 而"有没有子节点"并不能代表类型（空文件夹没有子节点）。
    /// 这里在刷新树时顺带缓存一份，渲染时 O(1) 查表，避免逐行访问磁盘。
    dir_flags: Rc<HashMap<PathBuf, bool>>,
}

impl FilesPanel {
    pub fn new(cx: &mut App) -> Self {
        println!("[xink] FilesPanel::new");
        Self {
            focus_handle: cx.focus_handle(),
            tree_state: cx.new(|cx| TreeState::new(cx)),
            vault: None,
            status: "未打开笔记库".into(),
            restored: false,
            applied_vault_root: None,
            watcher: None,
            watcher_task: None,
            last_manual_refresh: None,

            dir_flags: Rc::new(HashMap::new()),
        }
    }

    // ---- 查询 ----

    fn selected_path(&self, cx: &App) -> Option<PathBuf> {
        let item = self.tree_state.read(cx).selected_item()?.clone();
        Some(PathBuf::from(item.id.to_string()))
    }

    /// 新建操作的目标目录：选中目录用之，选中文件用其父目录，未选中用库根
    fn target_dir(&self, cx: &App) -> Option<PathBuf> {
        let vault = self.vault.as_ref()?;
        match self.selected_path(cx) {
            Some(path) if path.is_dir() => Some(path),
            Some(path) => path.parent().map(|p| p.to_path_buf()),
            None => Some(vault.root().to_path_buf()),
        }
    }

    // ---- 动作 ----

    fn open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择笔记库".into()),
        });

        cx.spawn_in(window, async move |this, window| {
            // prompt 的 oneshot 是双层 Result：外层是通道取消，内层是平台调用结果
            match prompt.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(root) = paths.into_iter().next() {
                        // 已注册则切换，否则新增；文件树由守卫在下一帧重载
                        _ = this.update_in(window, |_, _, cx| {
                            crate::ui::vault_manager::create_or_switch_vault(root, cx)
                        });
                    }
                }
                Ok(Ok(None)) => {} // 用户取消
                Ok(Err(err)) => eprintln!("[xink] 目录选择对话框失败: {err:?}"),
                Err(_) => {} // 通道被取消
            }
        })
        .detach();
    }

    fn new_note(&mut self, _: &NewNote, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.target_dir(cx) else {
            self.status = "请先打开笔记库".into();
            cx.notify();
            return;
        };
        self.new_note_in(dir, window, cx);
    }

    fn new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.target_dir(cx) else {
            self.status = "请先打开笔记库".into();
            cx.notify();
            return;
        };
        self.new_folder_in(dir, window, cx);
    }

    /// 在指定目录下新建笔记
    fn new_note_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_new_path(&dir, Some("未命名.md"));

        cx.spawn_in(window, async move |this, window| {
            if let Ok(Ok(Some(path))) = prompt.await {
                _ = this.update_in(window, |this, window, cx| {
                    this.create_file(&path, false, window, cx);
                });
            }
        })
        .detach();
    }

    /// 在指定目录下新建文件夹
    fn new_folder_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_new_path(&dir, Some("新建文件夹"));

        cx.spawn_in(window, async move |this, window| {
            if let Ok(Ok(Some(path))) = prompt.await {
                _ = this.update_in(window, |this, window, cx| {
                    this.create_file(&path, true, window, cx);
                });
            }
        })
        .detach();
    }

    fn delete_item(&mut self, _: &DeleteItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_path(cx) else {
            self.status = "请先选中要删除的项".into();
            cx.notify();
            return;
        };
        self.delete_node(path, window, cx);
    }

    /// 删除指定路径（弹窗二次确认），目标由右键菜单或选中项决定
    fn delete_node(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let message = format!("确定删除「{name}」？此操作会直接从磁盘删除。");
        let answer = window.prompt(
            PromptLevel::Warning,
            message.as_str(),
            None,
            &["删除", "取消"],
            cx,
        );

        cx.spawn_in(window, async move |this, window| {
            if answer.await == Ok(0) {
                _ = this.update_in(window, |this, window, cx| {
                    if let Some(vault) = &this.vault {
                        match vault.remove(&path) {
                            Ok(()) => this.status = format!("已删除 {name}").into(),
                            Err(err) => this.status = format!("删除失败: {err}").into(),
                        }
                    }
                    this.refresh(window, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 右键菜单所需的目录推导：目录本身就是目标，文件取其父目录
    fn target_dir_for(&self, path: &Path) -> PathBuf {
        if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| {
                self.vault
                    .as_ref()
                    .map(|v| v.root().to_path_buf())
                    .unwrap_or_default()
            })
        }
    }

    /// F2 快捷键 / 动作入口：对当前选中项重命名
    fn rename_item(&mut self, _: &RenameItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_path(cx) else {
            self.status = "请先选中要重命名的项".into();
            cx.notify();
            return;
        };
        self.rename_node(path, window, cx);
    }

    /// 对指定路径重命名：弹出输入框，处理确认 / 取消
    fn rename_node(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let old_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let prompt = cx.prompt_for_new_path(&parent, Some(&old_name));

        cx.spawn_in(window, async move |this, window| {
            if let Ok(Ok(Some(new_path))) = prompt.await {
                let new_name = new_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                // 用户在对话框里取消会拿到 None；空名直接忽略
                if new_name.is_empty() {
                    return;
                }
                _ = this.update_in(window, |this, window, cx| {
                    this.rename_to(&path, &new_name, window, cx);
                });
            }
        })
        .detach();
    }

    /// 执行重命名，并校验重名冲突（文件自动补 .md 后缀，目录直接改名）
    fn rename_to(
        &mut self,
        from: &Path,
        new_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = &self.vault else {
            return;
        };
        let Some(parent) = from.parent() else {
            return;
        };
        let is_file = from.is_file();
        let final_name = if is_file {
            with_md_suffix(new_name)
        } else {
            new_name.to_string()
        };
        let old_name = from
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // 同名（忽略大小写）视为未修改，直接忽略，避免误操作
        if final_name.eq_ignore_ascii_case(&old_name) {
            return;
        }

        let to = parent.join(&final_name);
        if to.exists() {
            self.status = format!("重命名失败：已存在 “{final_name}”").into();
            cx.notify();
            return;
        }

        let result = if is_file {
            vault.rename(from, new_name)
        } else {
            std::fs::rename(from, &to)
                .map(|_| to.clone())
                .map_err(|e| anyhow!(e))
        };

        match result {
            Ok(to) => self.status = format!("已重命名为 {}", to.display()).into(),
            Err(e) => self.status = format!("重命名失败: {e}").into(),
        }
        self.refresh(window, cx);
        cx.notify();
    }

    fn move_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_path(cx) else {
            self.status = "请先选中要移动的项".into();
            cx.notify();
            return;
        };
        self.move_node(path, window, cx);
    }

    /// 将指定路径移动到其他目录（弹窗选择目标）
    fn move_node(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择目标目录".into()),
        });

        cx.spawn_in(window, async move |this, window| {
            if let Ok(Ok(Some(dirs))) = prompt.await {
                if let Some(dir) = dirs.into_iter().next() {
                    _ = this.update_in(window, |this, window, cx| {
                        if let Some(vault) = &this.vault {
                            match vault.move_to(&path, &dir) {
                                Ok(to) => this.status = format!("已移动到 {}", to.display()).into(),
                                Err(err) => this.status = format!("移动失败: {err}").into(),
                            }
                        }
                        this.refresh(window, cx);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// 启动时恢复当前选中的库（由 `AppState.vaults` 决定，不再是单值记录）
    fn restore_last_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root())
        else {
            return;
        };
        match Vault::open(root.clone()) {
            Ok(vault) => {
                println!("[xink] 恢复上次库: {}", root.display());
                self.applied_vault_root = Some(root);
                self.vault = Some(vault);
                self.refresh(window, cx);
                self.start_watcher(window, cx);
            }
            Err(err) => {
                eprintln!("[xink] 恢复上次库失败: {err:?}");
                self.status = "上次库已失效，请重新打开".into();
                cx.notify();
            }
        }
    }

    /// 在中间区域打开笔记。
    ///
    /// 打开逻辑（去重、认领空白页、登记、落盘）统一在 `tabs::open_note`，
    /// 这里只补上文件面板自己的事：通知重绘。
    fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !tabs::open_note(path.clone(), window, cx) {
            return;
        }
        cx.notify();
    }

    /// 启动库目录监听。变更经 channel 事件驱动，不做定时轮询（省 CPU）。
    fn start_watcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = &self.vault else {
            return;
        };
        let mut watcher = match VaultWatcher::new(vault.root()) {
            Ok(w) => w,
            Err(err) => {
                eprintln!("[xink] 启动文件监听失败: {err:?}");
                return;
            }
        };
        let Some(rx) = watcher.take_receiver() else {
            return;
        };
        self.watcher = Some(watcher);

        // 旧任务会被覆盖而取消，保证一个库只跑一个监听循环
        self.watcher_task = Some(cx.spawn_in(window, async move |this, window| {
            while let Ok(event) = rx.recv_async().await {
                match this.update_in(window, |this, window, cx| {
                    this.handle_watch_event(event, window, cx)
                }) {
                    Ok(_) => {}
                    // 面板已释放，退出循环
                    Err(_) => break,
                }
            }
        }));
    }

    /// 处理一次变更：内容变化交给已打开的笔记，结构变化才重扫文件树
    fn handle_watch_event(
        &mut self,
        event: WatchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let structure_changed = matches!(event.kind, WatchKind::Created | WatchKind::Removed);
        println!(
            "[xink] 检测到 {} 个路径变更（结构变化: {structure_changed}）",
            event.paths.len()
        );

        // 已打开的笔记交给编辑器处理（自动重载或提示）。
        // 事件里的路径未经规范化，匹配由注册表内部按 tab_key 完成
        let panels: Vec<_> = cx
            .try_global::<AppState>()
            .map(|state| state.tabs.panels_for(&event.paths))
            .unwrap_or_default();

        for panel in panels {
            panel.update(cx, |editor, cx| editor.handle_external_change(window, cx));
        }

        // 只有新建/删除才重扫整棵树。
        // 自动保存产生的是内容写入，若也重扫，编辑时会持续卡顿。
        if structure_changed {
            self.refresh(window, cx);
        }
    }

    // ---- 内部 ----

    fn create_file(
        &mut self,
        path: &Path,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = &self.vault else { return };
        let Some(parent) = path.parent() else { return };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let result = if is_dir {
            vault.create_dir(parent, &name)
        } else {
            vault.create_note(parent, &name)
        };

        match result {
            Ok(created) => {
                self.status = format!("已创建 {}", created.display()).into();
            }
            Err(err) => {
                self.status = format!("创建失败: {err}").into();
            }
        }
        self.refresh(window, cx);
        cx.notify();
    }

    /// 后台扫描并刷新树，避免阻塞 UI
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = &self.vault else { return };
        let root = vault.root().to_path_buf();

        let scan = cx
            .background_executor()
            .spawn(async move { scan_root(root) });

        cx.spawn_in(window, async move |this, window| match scan.await {
            Ok(node) => {
                _ = this.update_in(window, |this, _window, cx| this.apply_tree(node, cx));
            }
            Err(err) => {
                _ = this.update_in(window, |this, _window, cx| {
                    this.status = format!("扫描失败: {err}").into();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 手动刷新：立即重新扫描当前库。
    ///
    /// 监听（`VaultWatcher`）已经会自动同步外部改动，这个按钮是**兜底**——
    /// 用于监听可能漏掉的场景（休眠唤醒后、网络盘、权限变化），
    /// 或用户只是想马上确认一次目录状态。
    fn refresh_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(last) = self.last_manual_refresh {
            if last.elapsed() < std::time::Duration::from_millis(300) {
                return;
            }
        }
        self.last_manual_refresh = Some(Instant::now());
        self.status = "正在扫描…".into();
        self.refresh(window, cx);
    }

    fn apply_tree(&mut self, node: FileNode, cx: &mut Context<Self>) {
        let notes = count_notes(&node);
        // 目录类型随树一起缓存：空文件夹没有子节点，无法靠子节点数量判断
        let mut flags = HashMap::new();
        collect_dir_flags(&node, &mut flags);
        self.dir_flags = Rc::new(flags);
        // 库根默认展开
        let items = vec![to_tree_item(&node).expanded(true)];
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
        println!("[xink] 文件树已更新: {notes} 篇笔记");
        self.status = format!("{notes} 篇笔记").into();
        cx.notify();
    }

}

/// 在后台线程执行：构造 Vault 并扫描
fn scan_root(root: PathBuf) -> anyhow::Result<FileNode> {
    Vault::open(root)?.scan()
}

fn to_tree_item(node: &FileNode) -> TreeItem {
    let id: SharedString = node.path.to_string_lossy().to_string().into();
    let mut item = TreeItem::new(id, node.name.as_str());
    for child in &node.children {
        item = item.child(to_tree_item(child));
    }
    item
}

/// 递归收集「路径 → 是否目录」，供渲染时判断节点类型
fn collect_dir_flags(node: &FileNode, out: &mut HashMap<PathBuf, bool>) {
    out.insert(node.path.clone(), node.is_dir);
    for child in &node.children {
        collect_dir_flags(child, out);
    }
}

fn count_notes(node: &FileNode) -> usize {
    if node.is_dir {
        node.children.iter().map(count_notes).sum()
    } else {
        1
    }
}

/// 为文件名补齐 .md 后缀（与 Vault::rename 的行为保持一致）
fn with_md_suffix(name: &str) -> String {
    if name.to_lowercase().ends_with(".md") {
        name.to_string()
    } else {
        format!("{name}.md")
    }
}

impl EventEmitter<PanelEvent> for FilesPanel {}

impl Focusable for FilesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for FilesPanel {
    fn panel_name(&self) -> &'static str {
        "FilesPanel"
    }
}

impl Panel for FilesPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "文件"
    }
}

impl Render for FilesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 目录树外观配置
        let cfg = crate::app::config(cx);
        // 图标尺寸取自配置，工具栏/树/底部设置入口保持一致
        let icon_px = px(cfg.basic.icon_size as f32);
        // 目录标记表：Rc 克隆代价很低，可安全移入逐行渲染的闭包
        let dir_flags = self.dir_flags.clone();
        if !self.restored {
            self.restored = true;
            self.restore_last_vault(window, cx);
        }

        // 当前库发生变化（在库选择器里切换 / 新打开了一个库）→ 重新加载树与监听。
        // 用 applied_vault_root 守卫，避免每帧重复加载。
        let active_root = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root());
        if active_root != self.applied_vault_root {
            self.applied_vault_root = active_root.clone();
            match active_root {
                Some(root) => match Vault::open(root.clone()) {
                    Ok(vault) => {
                        // 旧库的标签不再属于当前库，先关掉（这里才有 window 可以移除面板）
                        crate::ui::tabs::close_notes_outside(&root, window, cx);
                        println!("[xink] 切换库: {}", root.display());
                        self.vault = Some(vault);
                        // 退出图片预览：否则上一个库的图片会留在预览面板里
                        cx.update_global::<AppState, _>(|state, cx| {
                            state.preview.update(cx, |preview, cx| {
                                preview.image = None;
                                cx.emit(crate::app::PreviewImageChanged(None));
                            });
                        });
                        self.refresh(window, cx);
                        self.start_watcher(window, cx);
                    }
                    Err(err) => {
                        self.status = format!("打开库失败: {err}").into();
                        cx.notify();
                    }
                },
                None => {
                    self.vault = None;
                    self.status = "未打开笔记库".into();
                }
            }
        }
        let started = Instant::now();
        let tree_state = self.tree_state.clone();
        let weak = cx.weak_entity();
        // 供右键菜单闭包单独持有，避免与树渲染闭包争用同一份 weak
        let weak_menu = weak.clone();

        let view = div()
            .id("files-panel")
            .size_full()
            .flex()
            .flex_col()
            .key_context("FilesPanel")
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::new_note))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::delete_item))
            .on_action(cx.listener(Self::rename_item))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .p_2()
                    .child(
                        Button::new("open-folder")
                            .icon(IconName::FolderOpen)
                            .small()
                            .tooltip("打开笔记库")
                            .on_click(cx.listener(|this, _, window, cx| {
                                Self::open_folder(this, &OpenFolder, window, cx)
                            })),
                    )
                    .child(
                        Button::new("new-note")
                            .icon(IconName::FileText)
                            .small()
                            .tooltip("新建笔记")
                            .on_click(cx.listener(|this, _, window, cx| {
                                Self::new_note(this, &NewNote, window, cx)
                            })),
                    )
                    .child(
                        Button::new("new-folder")
                            .icon(IconName::Folder)
                            .small()
                            .tooltip("新建目录")
                            .on_click(cx.listener(|this, _, window, cx| {
                                Self::new_folder(this, &NewFolder, window, cx)
                            })),
                    )
                    .child(
                        Button::new("rename-item")
                            .icon(IconName::Replace)
                            .small()
                            .tooltip("重命名")
                            .on_click(cx.listener(|this, _, window, cx| {
                                Self::rename_item(this, &RenameItem, window, cx)
                            })),
                    )
                    .child(
                        Button::new("move-item")
                            .icon(IconName::ArrowRight)
                            .small()
                            .tooltip("移动到其他目录")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.move_item(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("refresh-tree")
                            // 顺序很关键：`.small()` 必须在 `.icon()` **之前**——
                            // 它会重置图标尺寸，放在后面会把显式设置的尺寸覆盖成空。
                            // 另外组件库内置图标集没有刷新，这里用完整图标集（Lucide）。
                            .small()
                            .icon(Icon::new(AssetIcon::RefreshCw).with_size(Size::Size(px(14.))))
                            .tooltip("刷新：重新扫描当前库（监听失灵时的兜底）")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.refresh_now(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("delete-item")
                            .icon(IconName::Delete)
                            .small()
                            .tooltip("删除")
                            .danger()
                            .on_click(cx.listener(|this, _, window, cx| {
                                Self::delete_item(this, &DeleteItem, window, cx)
                            })),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    tree(&self.tree_state, move |ix, entry, _selected, _w, _cx| {
                        let item = entry.item();
                        let depth = entry.depth();
                        let path = PathBuf::from(item.id.to_string());
                        // 目录类型取自扫描时缓存的标记。不能再用「有没有子节点」推断——
                        // 空文件夹没有子节点，会被误判成文件（图标、加粗、点击行为全错）。
                        // 查不到时退回一次磁盘判断，兼容 id 与原始路径不完全一致的情况。
                        let is_dir = dir_flags.get(&path).copied().unwrap_or_else(|| path.is_dir());
                        let state = tree_state.clone();
                        let weak_click = weak.clone();

                        // 目录：折叠用闭口文件夹 + 向右箭头，展开用开口文件夹 + 向下箭头
                        // 文件：文件图标，左侧留白与箭头对齐，保证名称落在同一列
                        // 目录树的字体与图标尺寸均取自配置，改完立即生效
                        let font = &cfg.basic.tree_font;
                        let icon_px = px(cfg.basic.icon_size as f32);
                        // 用完整图标集（gpui_kit::assets）：组件库内置的那 101 个
                        // 图标里没有图片相关图标
                        // 图标包一层 `flex_none`：它是固定尺寸元素，可用宽度不足时
                        // 绝不能被 flex 压缩，否则名称起始列会随文件名长度漂移。
                        let icon = |name: AssetIcon| {
                            div()
                                .flex_none()
                                .child(Icon::new(name).with_size(Size::Size(icon_px)))
                        };
                        let mut row = div()
                            .flex()
                            .items_center()
                            .gap_1()
                            // 行宽恒等于可用宽度：既不因长文件名撑开，也不随面板宽度抖动
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .font_family(font.family.clone())
                            .text_size(px(font.size))
                            .line_height(px(font.size * font.line_height))
                            .font_weight(FontWeight(font.weight.value()));
                        if is_dir {
                            if entry.is_expanded() {
                                row = row
                                    .child(icon(AssetIcon::ChevronDown))
                                    .child(icon(AssetIcon::FolderOpen));
                            } else {
                                row = row
                                    .child(icon(AssetIcon::ChevronRight))
                                    .child(icon(AssetIcon::FolderClosed));
                            }
                        } else if is_image_path(&path) {
                            // 图片文件：专用图标，点击打开图片预览而不是当笔记打开
                            row = row
                                .child(div().flex_none().w(icon_px))
                                .child(icon(AssetIcon::FileImage));
                        } else {
                            row = row
                                .child(div().flex_none().w(icon_px))
                                .child(icon(AssetIcon::FileText));
                        }
                        // 目录名加粗，与文件形成强弱对比。
                        // 名称占满剩余空间并单行截断：超长时不挤压前面的图标列，也不撑宽行。
                        // 框架没有省略号（truncate）API，这里用 nowrap + 裁剪代替。
                        let name = div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(item.label.to_string());
                        let name = if is_dir {
                            name.font_weight(FontWeight::BOLD)
                        } else {
                            name
                        };
                        row = row.child(name);

                        ListItem::new(("file-tree-item", ix))
                            // 按层级缩进，让父子关系一眼可辨。
                            // 固定像素缩进：不随面板宽度变化，各层级列位置恒定。
                            .pl(px(6. + depth as f32 * 16.))
                            .min_w_0()
                            .overflow_hidden()
                            .child(row)
                            .on_click(move |_, window, cx| {
                                state.update(cx, |state, cx| {
                                    state.set_selected_index(Some(ix), cx);
                                });
                                // 目录的展开/折叠由 Tree 自身处理，这里只负责打开文件
                                if !is_dir {
                                    if is_image_path(&path) {
                                        // 图片不进编辑器，在预览面板里显示（不新开窗口）
                                        crate::app::show_preview_image(path.clone(), cx);
                                    } else if let Some(panel) = weak_click.upgrade() {
                                        panel.update(cx, |p, cx| {
                                            p.open_note(path.clone(), window, cx)
                                        });
                                    }
                                }
                            })
                    })
                    // 右键上下文菜单：面向单节点，提供重命名 / 删除 / 新建 / 移动等操作
                    .context_menu(move |ix, entry, menu, _window, _cx| {
                        let path = PathBuf::from(entry.item().id.to_string());
                        let weak = weak_menu.clone();

                        let w_rename = weak.clone();
                        let p_rename = path.clone();
                        let w_delete = weak.clone();
                        let p_delete = path.clone();
                        let w_new_note = weak.clone();
                        let p_new_note = path.clone();
                        let w_new_folder = weak.clone();
                        let p_new_folder = path.clone();
                        let w_move = weak.clone();
                        let p_move = path.clone();

                        menu.item(PopupMenuItem::new("重命名").on_click(move |_, window, cx| {
                            if let Some(panel) = w_rename.upgrade() {
                                panel.update(cx, |this, cx| {
                                    this.tree_state.update(cx, |s, cx| {
                                        s.set_selected_index(Some(ix), cx);
                                    });
                                    this.rename_node(p_rename.clone(), window, cx);
                                });
                            }
                        }))
                        .item(PopupMenuItem::new("删除").on_click(move |_, window, cx| {
                            if let Some(panel) = w_delete.upgrade() {
                                panel.update(cx, |this, cx| {
                                    this.delete_node(p_delete.clone(), window, cx);
                                });
                            }
                        }))
                        .separator()
                        .item(
                            PopupMenuItem::new("在此新建笔记").on_click(move |_, window, cx| {
                                if let Some(panel) = w_new_note.upgrade() {
                                    panel.update(cx, |this, cx| {
                                        let dir = this.target_dir_for(&p_new_note);
                                        this.new_note_in(dir, window, cx);
                                    });
                                }
                            }),
                        )
                        .item(PopupMenuItem::new("在此新建文件夹").on_click(
                            move |_, window, cx| {
                                if let Some(panel) = w_new_folder.upgrade() {
                                    panel.update(cx, |this, cx| {
                                        let dir = this.target_dir_for(&p_new_folder);
                                        this.new_folder_in(dir, window, cx);
                                    });
                                }
                            },
                        ))
                        .item(
                            PopupMenuItem::new("移动到…").on_click(move |_, window, cx| {
                                if let Some(panel) = w_move.upgrade() {
                                    panel.update(cx, |this, cx| {
                                        this.move_node(p_move.clone(), window, cx);
                                    });
                                }
                            }),
                        )
                    }),
                ),
            )
            .child(div().flex_none().p_2().text_xs().child(self.status.clone()))
            // 底部固定区：设置入口常驻侧边栏最底部（左下角），
            // 与上方区域同属 flex_col，但自身 flex_none，因此不随目录树滚动
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.render_vault_switcher(cx))
                    .child(
                        Button::new("open-settings")
                            .icon(Icon::new(IconName::Settings).with_size(Size::Size(icon_px)))
                            .small()
                            .ghost()
                            .label("设置")
                            .tooltip("打开设置（Cmd-,）")
                            .on_click(|_, _, cx| crate::ui::open_settings(cx)),
                    ),
            );

        crate::ui::log_slow("FilesPanel::render", started);
        view
    }
}

// ── 多库：选择器 ──────────────────────────────────────────────
//
// 选择器只负责"切换到哪个库"；库列表本身的增删改用「配置管理」窗口
// （`ui/vault_manager.rs`，设计见 docs/multi-vault-design.md §6.4）。

impl FilesPanel {
    /// 左下角的库选择器：显示当前库名，展开可切换库或进入管理界面
    fn render_vault_switcher(&self, cx: &mut Context<Self>) -> Div {
        let registry = cx
            .try_global::<AppState>()
            .map(|state| state.vaults.clone())
            .unwrap_or_default();
        let current = registry
            .active_entry()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "选择库".to_string());
        let active_id = registry.active.clone();
        let items: Vec<(String, String)> = registry
            .vaults
            .iter()
            .map(|v| (v.id.clone(), v.name.clone()))
            .collect();

        div().flex_1().min_w_0().child(
            DropdownButton::new("vault-switcher")
                .button(Button::new("vault-switcher-btn").small().ghost().label(current))
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (id, name) in items.iter() {
                        let is_active = active_id.as_deref() == Some(id.as_str());
                        let label = if is_active {
                            format!("✓ {name}")
                        } else {
                            name.clone()
                        };
                        let id = id.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            crate::ui::vault_manager::switch_to_vault(&id, cx);
                        }));
                    }
                    // 分隔线以下不是"选哪个库"，而是库列表的维护入口
                    menu = menu
                        .separator()
                        .item(PopupMenuItem::new("打开文件夹作为库…").on_click(|_, _, cx| {
                            crate::ui::vault_manager::prompt_create_vault(cx);
                        }))
                        .item(PopupMenuItem::new("配置管理…").on_click(|_, _, cx| {
                            crate::ui::vault_manager::open_vault_manager(cx);
                        }));
                    menu
                }),
        )
    }
}

#[cfg(test)]
mod tests;
