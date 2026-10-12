//! 右侧预览面板：渲染当前活动笔记的 Markdown

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::base::Theme as BaseTheme;
use gpui_kit::base::{ScrollbarHandle, TextView, TextViewState, TextViewStyle};
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;

use crate::app::{
    ActiveDocChanged, AppState, EditorScrolled, PreviewImageChanged, PreviewScrolled,
};
use crate::core::config::{LinkOpen, PreviewDensity, resolve_code_theme};
use crate::core::highlight::highlight_with_theme;
use crate::core::markdown::path_from_file_url;
use crate::core::media::is_image_path;

pub struct PreviewPanel {
    focus_handle: FocusHandle,
    /// 当前显示的文档。切换笔记时整体替换引用，已解析过的文档不再重解析。
    doc: Option<Entity<TextViewState>>,
    /// 图片预览模式：非空时面板显示这张图片，而不是 Markdown
    image: Option<PathBuf>,
    /// 上次推送给编辑器的滚动比例
    last_emitted_ratio: f32,
    /// 上次从编辑器接收并应用的滚动比例，用于吸收回显、避免同步循环
    last_received_ratio: f32,
}

impl PreviewPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            doc: None,
            image: None,
            last_emitted_ratio: 0.0,
            last_received_ratio: 0.0,
        };

        let Some(state) = cx.try_global::<AppState>().map(|s| s.preview.clone()) else {
            return this;
        };

        // 笔记可能先于预览面板创建，先把已存在的活动文档接上
        this.doc = state.read(cx).active_doc.clone();
        this.image = state.read(cx).image.clone();

        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &ActiveDocChanged, _, cx| {
                this.doc = Some(ev.0.clone());
                // 切换/编辑笔记时退出图片模式，回到 Markdown
                this.image = None;
                cx.notify();
            },
        )
        .detach();

        // 图片预览：与 Markdown 共用同一个预览面板，不再新开窗口
        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &PreviewImageChanged, _, cx| {
                this.image = ev.0.clone();
                cx.notify();
            },
        )
        .detach();

        // 编辑器滚动 → 同步预览滚动
        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &EditorScrolled, _, cx| {
                let EditorScrolled(ratio) = ev;
                this.apply_editor_ratio(*ratio, cx);
            },
        )
        .detach();

        // 启动预览滚动同步轮询（框架在实体移除后不再回调，循环自动停止）
        this.schedule_scroll_tick(window, cx);

        this
    }

    /// 应用编辑器推送的滚动比例到本预览文档。
    fn apply_editor_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        // 图片模式下没有 Markdown 可同步
        if self.image.is_some() {
            return;
        }
        // 关闭同步滚动后，编辑器滚动不再带动预览
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }
        let Some(doc) = &self.doc else {
            return;
        };
        let target_y = {
            let st = doc.read(cx);
            let list = st.list_state();
            let content_h = list.content_size().height;
            let viewport_h = list.viewport_bounds().size.height;
            let max = (content_h - viewport_h).max(px(1.));
            -(ratio.clamp(0.0, 1.0) * max)
        };
        doc.update(cx, |st, cx| {
            st.list_state().set_offset(point(px(0.), target_y));
            cx.notify();
        });
        // 记录"我刚按编辑器目标滚到的位置"，供后续回显吸收
        self.last_received_ratio = ratio;
    }

    /// 每帧轮询预览滚动比例，变化超阈值则推给编辑器（含回显吸收，避免循环）。
    fn tick_scroll_sync(&mut self, cx: &mut Context<Self>) {
        const EPS: f32 = 0.002;

        // 图片模式下没有 Markdown 可同步
        if self.image.is_some() {
            return;
        }

        // 关闭同步滚动后，不再把预览滚动推给编辑器
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }

        let ratio = if let Some(doc) = &self.doc {
            let st = doc.read(cx);
            let list = st.list_state();
            let content_h = list.content_size().height;
            let viewport_h = list.viewport_bounds().size.height;
            let max = (content_h - viewport_h).max(px(1.));
            let off = list.offset().y;
            (-off / max).clamp(0.0, 1.0)
        } else {
            self.last_emitted_ratio = 0.0;
            self.last_received_ratio = 0.0;
            return;
        };

        // 回显吸收：本次比例与"编辑器给我的目标"一致 → 是我刚应用的同步结果
        if (ratio - self.last_received_ratio).abs() <= EPS {
            self.last_emitted_ratio = ratio;
            return;
        }
        // 真实用户滚动：与上一次推送一致则跳过
        if (ratio - self.last_emitted_ratio).abs() <= EPS {
            return;
        }
        self.last_emitted_ratio = ratio;
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |_, cx| {
                cx.emit(PreviewScrolled(ratio));
            });
        });
    }

    /// 调度下一帧滚动同步轮询。框架在实体被移除后不再回调，循环自动停止。
    fn schedule_scroll_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, move |this, window, cx| {
            this.tick_scroll_sync(cx);
            this.schedule_scroll_tick(window, cx);
        });
    }
}

impl EventEmitter<PanelEvent> for PreviewPanel {}

impl Focusable for PreviewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for PreviewPanel {
    fn panel_name(&self) -> &'static str {
        "PreviewPanel"
    }
}

impl Panel for PreviewPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 密度菜单与"预览"同一行：水平对齐、间距由 gap 统一控制
        div()
            .flex()
            .items_center()
            .gap_2()
            .child("预览")
            .child(density_menu(crate::app::config(cx).preview.density))
    }
}

impl Render for PreviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        let cfg = crate::app::config(cx);

        // 图片预览模式：复用同一个预览面板显示图片，不再为每张图开新窗口。
        // 切换/编辑笔记时会自动清掉 image，回到 Markdown。
        if let Some(path) = self.image.clone() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            return div()
                .id("preview-panel")
                .size_full()
                .flex()
                .flex_col()
                .bg(cx.theme().background)
                .child(
                    div()
                        .flex_none()
                        .p_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_2()
                        .child(
                            img(path)
                                .object_fit(ObjectFit::Contain)
                                .max_w(relative(1.))
                                .max_h(relative(1.)),
                        ),
                );
        }

        let Some(doc) = self.doc.clone() else {
            return div()
                .id("preview-panel")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child("没有打开的笔记");
        };

        // 代码块主题：`auto` 表示跟随当前明暗（浅色主题下不再用深色高亮）。
        // 解析逻辑在配置层（纯函数，有单测覆盖）。
        let code_theme = resolve_code_theme(&cfg.preview.code_theme, crate::app::is_dark(cx));
        let font = cfg.preview.font.clone();

        let density = cfg.preview.density;
        let layout = density.layout();

        // 排版样式：段间距与标题层级随密度变化；颜色沿用主题，明暗自动一致
        // `from_theme` 要 gpui_base 的 Theme（与 gpui_component 的 Theme 是不同类型），
        // 它是全局的，直接取即可——这样颜色随主题，只覆盖布局相关的几项
        let text_style = TextViewStyle::from_theme(&cx.global::<BaseTheme>())
            .with_paragraph_gap(rems(layout.paragraph_gap))
            .with_heading_base_font_size(px(font.size * layout.heading_scale))
            .with_heading_font_size(|level: u8, base: Pixels| {
                let factor = match level {
                    1 => 1.0,
                    2 => 0.87,
                    3 => 0.77,
                    4 => 0.68,
                    5 => 0.62,
                    _ => 0.58,
                };
                base * factor
            });

        let view = div()
            .id("preview-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(font.family.clone())
            .text_size(px(font.size))
            .line_height(px(font.size * font.line_height))
            // 字重接线：之前只应用了字体三件套，weight 配了不生效
            .font_weight(FontWeight(font.weight.value()));

        // 正文最大宽度：用户显式设置优先，其次用当前档位的默认值
        let max_width = if cfg.preview.max_width > 0 {
            cfg.preview.max_width as f32
        } else {
            layout.max_width
        };

        let view = view.child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(max_width))
                .mx_auto()
                .child(
                    TextView::new(&doc)
                        .size_full()
                        .scrollable(true)
                        .px(px(layout.pad_x))
                        .py(px(layout.pad_y))
                        .style(text_style)
                        .code_block_highlighter(move |block| {
                            // 代码块可能没有标注语言
                            let lang = block.lang();
                            let lang = lang.as_ref().map(|s| s.as_ref()).unwrap_or("");
                            highlight_with_theme(lang, block.code().as_ref(), &code_theme)
                                .into_iter()
                                .map(|(range, color)| {
                                    (
                                        range,
                                        HighlightStyle {
                                            color: Some(color),
                                            ..Default::default()
                                        },
                                    )
                                })
                                .collect()
                        })
                        .on_link_click(|url, _event, window, cx| {
                            handle_link_click(url, window, cx);
                        }),
                ),
        );

        crate::ui::log_slow("PreviewPanel::render", started);
        view
    }
}

/// 版面密度菜单（紧凑 / 适中 / 宽松）。
///
/// 挂进面板标题，与"预览"同一行：不额外占一行，且和标题水平对齐。
fn density_menu(density: PreviewDensity) -> impl IntoElement {
    div()
        // 独占点击：避免点菜单时顺带触发标签选中
        .occlude()
        .flex_none()
        .child(
            DropdownButton::new("preview-density")
                .button(
                    Button::new("preview-density-trigger")
                        .small()
                        .ghost()
                        .label(density.label()),
                )
                .dropdown_menu(|menu, _window, _cx| {
                    let mut menu = menu;
                    for option in [
                        PreviewDensity::Compact,
                        PreviewDensity::Normal,
                        PreviewDensity::Relaxed,
                    ] {
                        menu = menu.item(PopupMenuItem::new(option.label()).on_click(
                            move |_, _, cx| {
                                // 走统一入口：改内存 → 落盘 → 刷新窗口，与设置页一致
                                crate::app::update_config(cx, |cfg| {
                                    cfg.preview.density = option;
                                });
                            },
                        ));
                    }
                    menu
                }),
        )
}

/// 点击预览区链接 / 图片的行为
///
/// - 本地图片：预处理时我们把图片包成了指向自身的 `file://` 链接，因此点击图片
///   会走到这里（框架没有图片点击回调，只有链接回调），此时打开图片预览窗口；
/// - 其余链接按 `preview.link_open` 配置处理：应用内打开时尝试作为笔记打开，
///   打不开（非 Markdown / 文件不存在）则退回系统浏览器。
fn handle_link_click(url: &SharedString, window: &mut Window, cx: &mut App) {
    if let Some(path) = path_from_file_url(url) {
        if is_image_path(&path) && path.is_file() {
            // 在预览面板里显示这张图（不再新开窗口）
            crate::app::show_preview_image(path, cx);
            return;
        }
        if crate::app::config(cx).preview.link_open == LinkOpen::InApp
            && crate::ui::tabs::open_note(path, window, cx)
        {
            return;
        }
    }
    cx.open_url(url);
}
