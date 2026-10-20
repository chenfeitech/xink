//! Vault 管理窗口：对已注册的库做增删改查
//!
//! 入口在目录树左下角的菜单里（设计见 `docs/multi-vault-design.md` §6.4）。
//! 与库选择器的分工：**选择器负责"切换当前库"，这里负责"维护库列表本身"**。

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::Root;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::vault::{VaultEntry, VaultRegistry, same_path};
use crate::state::workspace;

/// 窗口各部分的固定尺寸。
///
/// 与设置窗口 `layout` 常量同一思路：框架没有"测量元素"的通用 API，
/// 这里把每部分高度固定下来，窗口高度即可按库数量精确计算。
mod layout {
    pub const WIDTH: f32 = 560.;
    pub const HEADER_H: f32 = 44.;
    pub const FOOTER_H: f32 = 52.;
    /// 单个库卡片的高度（名称行 + 路径行 + 操作行）
    pub const ROW_H: f32 = 96.;
    /// 没有库时的空态高度
    pub const EMPTY_H: f32 = 64.;
    pub const MIN_H: f32 = 220.;
    pub const MAX_H: f32 = 720.;

    pub fn window_height(rows: usize) -> f32 {
        let list = if rows == 0 {
            EMPTY_H
        } else {
            rows as f32 * ROW_H
        };
        (HEADER_H + list + FOOTER_H).clamp(MIN_H, MAX_H)
    }
}

/// 库管理窗口的内容
pub struct VaultManagerView {
    /// 名称编辑框，按库 id 索引
    names: HashMap<String, Entity<InputState>>,
    /// 待确认删除的库 id：删除是破坏性操作，用两段式确认防误删
    pending_delete: Option<String>,
    /// 上次自适应所用的内容指纹（库数量），变化时才 resize
    size_fingerprint: Option<usize>,
}

impl VaultManagerView {
    fn new() -> Self {
        Self {
            names: HashMap::new(),
            pending_delete: None,
            size_fingerprint: None,
        }
    }

    fn registry(cx: &App) -> VaultRegistry {
        cx.try_global::<AppState>()
            .map(|state| state.vaults.clone())
            .unwrap_or_default()
    }

    /// 让输入框与注册表同步：新库建框、删掉的库清框，值不同则回显。
    /// `set_value` 不触发 Change 事件，因此回显不会被误判成用户改名。
    fn sync_inputs(
        &mut self,
        registry: &VaultRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for entry in &registry.vaults {
            let id = entry.id.clone();
            let state = self.names.entry(id.clone()).or_insert_with(|| {
                let state = cx.new(|cx| InputState::new(window, cx));
                // 改名即时保存：与"改完立即生效"的整体风格一致
                let id_for_event = id.clone();
                cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, ev: &InputEvent, _window, cx| {
                        if !matches!(ev, InputEvent::Change) {
                            return;
                        }
                        if let Some(state) = this.names.get(&id_for_event) {
                            let value = state.read(cx).value().to_string();
                            rename_vault(&id_for_event, value, cx);
                        }
                    },
                )
                .detach();
                state
            });
            if state.read(cx).value() != entry.name {
                let name = entry.name.clone();
                state.update(cx, |s, cx| s.set_value(name, window, cx));
            }
        }
        self.names
            .retain(|id, _| registry.vaults.iter().any(|v| &v.id == id));
    }
}

impl Render for VaultManagerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let registry = Self::registry(cx);
        self.sync_inputs(&registry, window, cx);

        // 窗口高度随库数量自适应；仅在数量变化时才 resize，
        // 保留用户手动调整的结果（与设置窗口 §7.1 一致）
        let rows = registry.vaults.len();
        if self.size_fingerprint != Some(rows) {
            self.size_fingerprint = Some(rows);
            let desired = size(px(layout::WIDTH), px(layout::window_height(rows)));
            if window.viewport_size() != desired {
                window.resize(desired);
            }
        }

        let mut list = div().flex_1().min_h_0().flex_col().gap_2().px_3();

        if registry.vaults.is_empty() {
            list = list.child(
                div()
                    .py_6()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("还没有任何库，点下方「新建库」选择一个文件夹"),
            );
        }

        for entry in &registry.vaults {
            let id = entry.id.clone();
            let is_active = registry.active.as_deref() == Some(id.as_str());
            let status = entry.status(&registry.vaults);
            let input = self.names.get(&id).cloned();
            let deleting = self.pending_delete.as_deref() == Some(id.as_str());

            let mut card = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border);
            if is_active {
                card = card.bg(cx.theme().accent.opacity(0.12));
            }

            // 第一行：名称（可编辑）+ 状态
            let mut head = div().flex().items_center().gap_2();
            head = head.child(match input {
                Some(state) => Input::new(&state).small().w(px(180.)).into_any_element(),
                None => div().text_sm().child(entry.name.clone()).into_any_element(),
            });
            if is_active {
                head = head.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("当前库"),
                );
            }
            head = head.child(
                div()
                    .text_xs()
                    .text_color(if status.is_usable() {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().warning
                    })
                    .child(status.message()),
            );
            card = card.child(head).child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(entry.path.display().to_string()),
            );

            // 操作区
            let mut actions = div().flex().items_center().gap_1();
            if !is_active {
                let id_switch = id.clone();
                actions = actions.child(
                    Button::new(format!("vault-use-{id}"))
                        .small()
                        .ghost()
                        .label("切换")
                        .on_click(move |_, _, cx| switch_to_vault(&id_switch, cx)),
                );
            }
            let id_relocate = id.clone();
            actions = actions.child(
                Button::new(format!("vault-relocate-{id}"))
                    .small()
                    .ghost()
                    .label("重新定位")
                    .tooltip("路径失效时选择新的目录")
                    .on_click(move |_, _, cx| prompt_relocate(&id_relocate, cx)),
            );

            if deleting {
                let id_confirm = id.clone();
                actions = actions
                    .child(
                        Button::new(format!("vault-del-ok-{id}"))
                            .small()
                            .danger()
                            .label("确认删除")
                            .tooltip("仅移除记录，磁盘上的文件保留")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                delete_vault(&id_confirm, cx);
                                this.pending_delete = None;
                            })),
                    )
                    .child(
                        Button::new(format!("vault-del-no-{id}"))
                            .small()
                            .ghost()
                            .label("取消")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pending_delete = None;
                                cx.notify();
                            })),
                    );
            } else {
                let id_delete = id.clone();
                actions = actions.child(
                    Button::new(format!("vault-del-{id}"))
                        .small()
                        .ghost()
                        .label("删除")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pending_delete = Some(id_delete.clone());
                            cx.notify();
                        })),
                );
            }

            list = list.child(card.child(actions));
        }

        div()
            .id("vault-manager")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .text_sm()
                    .font_weight(FontWeight(600.))
                    .child("库管理"),
            )
            .child(list)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("vault-new")
                            .small()
                            .primary()
                            .label("新建库")
                            .on_click(|_, _, cx| prompt_create_vault(cx)),
                    ),
            )
    }
}

// ── 单例窗口 ──────────────────────────────────────────────────

struct VaultManagerWindow {
    handle: Option<WindowHandle<Root>>,
}

impl Global for VaultManagerWindow {}

/// 打开库管理窗口；已存在则聚焦已有窗口（与设置窗口一致）
pub fn open_vault_manager(cx: &mut App) {
    let existing = cx.try_global::<VaultManagerWindow>().and_then(|g| g.handle);

    if let Some(handle) = existing {
        let mut alive = false;
        _ = handle.update(cx, |_, window, _cx| {
            window.activate_window();
            alive = true;
        });
        if alive {
            return;
        }
    }

    cx.spawn(async move |cx| {
        match cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|_| VaultManagerView::new());
            cx.new(|cx| Root::new(view, window, cx))
        }) {
            Ok(handle) => {
                cx.update(move |cx| {
                    cx.set_global(VaultManagerWindow {
                        handle: Some(handle),
                    });
                });
            }
            Err(err) => eprintln!("[xink] 打开库管理失败: {err:?}"),
        }
    })
    .detach();
}

// ── 库操作（注册表增删改，全部即时落盘）────────────────────────

/// 写入注册表并刷新界面（文件树会在下一帧按当前库变化重载）
fn commit(registry: VaultRegistry, cx: &mut App) {
    if let Err(err) = workspace::save_vaults(&registry) {
        eprintln!("[xink] 保存库列表失败: {err:?}");
    }
    cx.update_global::<AppState, _>(|state, _| state.vaults = registry);
    cx.refresh_windows();
}

/// 切换到指定库
pub fn switch_to_vault(id: &str, cx: &mut App) {
    let Some(mut registry) = cx.try_global::<AppState>().map(|s| s.vaults.clone()) else {
        return;
    };
    if registry.find(id).is_none() {
        return;
    }
    registry.active = Some(id.to_string());
    commit(registry, cx);
}

/// 选择目录并作为库打开：已注册则切换，否则新增
pub fn prompt_create_vault(cx: &mut App) {
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("选择笔记库".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = prompt.await else {
            return;
        };
        let Some(root) = paths.into_iter().next() else {
            return;
        };
        cx.update(move |cx| create_or_switch_vault(root, cx));
    })
    .detach();
}

/// 注册（必要时）并切换到该目录
pub fn create_or_switch_vault(root: PathBuf, cx: &mut App) {
    if !root.is_dir() {
        eprintln!("[xink] 不是有效目录: {}", root.display());
        return;
    }
    let mut registry = cx
        .try_global::<AppState>()
        .map(|state| state.vaults.clone())
        .unwrap_or_default();

    let id = match registry
        .vaults
        .iter()
        .find(|v| same_path(&v.path, &root))
        .map(|v| v.id.clone())
    {
        Some(id) => id,
        None => {
            let entry = VaultEntry::new(root.clone(), VaultEntry::default_name(&root));
            let id = entry.id.clone();
            registry.vaults.push(entry);
            id
        }
    };
    registry.active = Some(id);
    commit(registry, cx);
}

/// 为失效的库重新选择路径；**id 保持不变**，将来挂在该库上的状态不会丢
fn prompt_relocate(id: &str, cx: &mut App) {
    let id = id.to_string();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("选择库的新位置".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = prompt.await else {
            return;
        };
        let Some(root) = paths.into_iter().next() else {
            return;
        };
        cx.update(move |cx| relocate_vault(&id, root, cx));
    })
    .detach();
}

/// 把某个库指向新的目录
pub fn relocate_vault(id: &str, path: PathBuf, cx: &mut App) {
    if !path.is_dir() {
        eprintln!("[xink] 不是有效目录: {}", path.display());
        return;
    }
    let Some(mut registry) = cx.try_global::<AppState>().map(|s| s.vaults.clone()) else {
        return;
    };
    let Some(entry) = registry.vaults.iter_mut().find(|v| v.id == id) else {
        return;
    };
    entry.path = path;
    commit(registry, cx);
}

/// 重命名：空名称回落到目录名
fn rename_vault(id: &str, name: String, cx: &mut App) {
    let Some(mut registry) = cx.try_global::<AppState>().map(|s| s.vaults.clone()) else {
        return;
    };
    let Some(index) = registry.vaults.iter().position(|v| v.id == id) else {
        return;
    };
    let path = registry.vaults[index].path.clone();
    let name = if name.trim().is_empty() {
        VaultEntry::default_name(&path)
    } else {
        name
    };
    if registry.vaults[index].name == name {
        return;
    }
    registry.vaults[index].name = name;
    commit(registry, cx);
}

/// 删除库：只从注册表移除，**绝不删磁盘文件**；删完回落到其它可用库
pub fn delete_vault(id: &str, cx: &mut App) {
    let Some(mut registry) = cx.try_global::<AppState>().map(|s| s.vaults.clone()) else {
        return;
    };
    registry.vaults.retain(|v| v.id != id);
    if registry.active.as_deref() == Some(id) {
        registry.active = None;
        registry.resolve_active();
    }
    commit(registry, cx);
}
