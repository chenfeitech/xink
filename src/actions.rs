//! 应用级 Action 定义（快捷键与命令面板的统一入口）

use std::collections::BTreeMap;

use gpui_kit::component::dock::ClosePanel;
use gpui_kit::*;

actions!(
    ink,
    [
        /// 打开一个文件夹作为笔记库
        OpenFolder,
        /// 新建笔记
        NewNote,
        /// 新建文件夹
        NewFolder,
        /// 删除选中项
        DeleteItem,
        /// 重命名选中项（F2，或右键菜单触发）
        RenameItem,
        /// 保存当前笔记
        Save,
        /// 打开设置
        OpenSettings,
        /// 加粗：包裹选中文本或插入 ** 标记
        FormatBold,
        /// 斜体：包裹选中文本或插入 * 标记
        FormatItalic,
        /// 删除线：包裹选中文本或插入 ~~ 标记
        FormatStrikethrough,
        /// 行内代码：包裹选中文本或插入 ` 标记
        FormatInlineCode,
        /// 链接：包裹选中文本或插入 [](url) 占位
        FormatLink,
        /// 在浅色 / 深色之间切换主题
        ToggleTheme,
        /// 打开命令面板：按文件名搜索并打开笔记
        OpenCommandPalette,
        /// 打开全文搜索：按内容检索
        OpenSearch,
        /// 打开 / 关闭编辑器下方的 AI 对话条
        ToggleAiChat
    ]
);

/// 按 `editor.keymap` 生成最终的键位表
///
/// 键位表本身在 [`crate::core::keymap`]（纯数据，可单测），这里只负责把
/// "动作名 + 键串" 变成 gpui 的 `KeyBinding`——只有这里能拿到具体的 Action
/// 类型（其中 `ClosePanel` 来自 dock 组件，core 层不引用组件）。
/// 覆盖值的合法性已在 core 侧校验过，因此 `KeyBinding::new` 不会 panic。
///
/// 设计见 `docs/config/keymap-wiring-design.md` §3.3。
pub fn key_bindings(overrides: &BTreeMap<String, String>) -> Vec<KeyBinding> {
    crate::core::keymap::resolve(overrides)
        .into_iter()
        .filter_map(|resolved| binding_for(resolved.action, &resolved.keystroke, resolved.context))
        .collect()
}

/// 动作名 → `KeyBinding`；未知动作名返回 `None`（不产生绑定）
fn binding_for(action: &str, keystroke: &str, context: Option<&str>) -> Option<KeyBinding> {
    Some(match action {
        "OpenFolder" => KeyBinding::new(keystroke, OpenFolder, context),
        "NewNote" => KeyBinding::new(keystroke, NewNote, context),
        "NewFolder" => KeyBinding::new(keystroke, NewFolder, context),
        "DeleteItem" => KeyBinding::new(keystroke, DeleteItem, context),
        "Save" => KeyBinding::new(keystroke, Save, context),
        "ClosePanel" => KeyBinding::new(keystroke, ClosePanel, context),
        "RenameItem" => KeyBinding::new(keystroke, RenameItem, context),
        "OpenSettings" => KeyBinding::new(keystroke, OpenSettings, context),
        "FormatBold" => KeyBinding::new(keystroke, FormatBold, context),
        "FormatItalic" => KeyBinding::new(keystroke, FormatItalic, context),
        "FormatStrikethrough" => KeyBinding::new(keystroke, FormatStrikethrough, context),
        "FormatInlineCode" => KeyBinding::new(keystroke, FormatInlineCode, context),
        "FormatLink" => KeyBinding::new(keystroke, FormatLink, context),
        "ToggleTheme" => KeyBinding::new(keystroke, ToggleTheme, context),
        "OpenCommandPalette" => KeyBinding::new(keystroke, OpenCommandPalette, context),
        "OpenSearch" => KeyBinding::new(keystroke, OpenSearch, context),
        "ToggleAiChat" => KeyBinding::new(keystroke, ToggleAiChat, context),
        _ => return None,
    })
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../tests/actions.rs"]
mod tests;
