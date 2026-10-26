//! 快捷键绑定表与覆盖解析
//!
//! 设计见 `docs/config/keymap-wiring-design.md`。
//!
//! 这里只有**数据 + 字符串解析**：动作名 / 默认键位 / 上下文，以及
//! `editor.keymap` 覆盖表的校验与合并。`KeyBinding` 的构造不在这里——
//! 那需要具体的 Action 类型（其中 `ClosePanel` 来自 dock 组件），
//! 放在 [`crate::actions::key_bindings`]，core 层因此不必碰组件类型。

use std::collections::{BTreeMap, BTreeSet};

use gpui_kit::Keystroke;

/// 一个可绑定动作
///
/// 与 `src/actions.rs` 里的 action 名一一对应（`ClosePanel` 除外，它来自 dock）。
pub struct Binding {
    /// action 名，同时是 `editor.keymap` 的键
    pub action: &'static str,
    /// 设置项的 id（输入框也按它索引）
    pub id: &'static str,
    /// 设置界面显示的标题
    pub label: &'static str,
    /// 设置界面显示的说明（含默认键位与"重启后生效"提示）
    pub desc: &'static str,
    /// 默认键位；空串表示默认不绑定
    pub default: &'static str,
    /// 键位上下文（如 `Some("FilesPanel")`）
    pub context: Option<&'static str>,
}

/// 可绑定动作表
///
/// 顺序即优先级：键位冲突时靠前的保留，靠后的丢弃。
/// `default` 与 `src/main.rs` 接线前的硬编码键位逐条一致（有单测锁定）。
pub const BINDINGS: &[Binding] = &[
    Binding {
        action: "OpenFolder",
        id: "editor.keymap.OpenFolder",
        label: "打开笔记库",
        desc: "选一个文件夹作为笔记库 · 留空用默认 cmd-o（重启后生效）",
        default: "cmd-o",
        context: None,
    },
    Binding {
        action: "NewNote",
        id: "editor.keymap.NewNote",
        label: "新建笔记",
        desc: "在当前目录下新建一篇笔记 · 留空用默认 cmd-n（重启后生效）",
        default: "cmd-n",
        context: None,
    },
    Binding {
        action: "NewFolder",
        id: "editor.keymap.NewFolder",
        label: "新建文件夹",
        desc: "在当前目录下新建文件夹 · 默认不绑定（重启后生效）",
        default: "",
        context: None,
    },
    Binding {
        action: "DeleteItem",
        id: "editor.keymap.DeleteItem",
        label: "删除选中项",
        desc: "删除文件树里选中的文件或文件夹 · 默认不绑定（重启后生效）",
        default: "",
        context: None,
    },
    Binding {
        action: "Save",
        id: "editor.keymap.Save",
        label: "保存当前笔记",
        desc: "立即把当前笔记写盘 · 留空用默认 cmd-s（重启后生效）",
        default: "cmd-s",
        context: None,
    },
    Binding {
        action: "ClosePanel",
        id: "editor.keymap.ClosePanel",
        label: "关闭当前标签",
        desc: "关闭当前笔记标签页 · 留空用默认 ctrl-w（重启后生效）",
        default: "ctrl-w",
        context: None,
    },
    Binding {
        action: "RenameItem",
        id: "editor.keymap.RenameItem",
        label: "重命名选中项",
        desc: "重命名文件树里选中的项（仅文件树内生效）· 留空用默认 f2（重启后生效）",
        default: "f2",
        context: Some("FilesPanel"),
    },
    Binding {
        action: "OpenSettings",
        id: "editor.keymap.OpenSettings",
        label: "打开设置",
        desc: "打开设置窗口 · 留空用默认 cmd-,（重启后生效）",
        default: "cmd-,",
        context: None,
    },
    Binding {
        action: "FormatBold",
        id: "editor.keymap.FormatBold",
        label: "加粗",
        desc: "包裹选中文本为 **粗体** · 留空用默认 cmd-b（重启后生效）",
        default: "cmd-b",
        context: None,
    },
    Binding {
        action: "FormatItalic",
        id: "editor.keymap.FormatItalic",
        label: "斜体",
        desc: "包裹选中文本为 *斜体* · 留空用默认 cmd-i（重启后生效）",
        default: "cmd-i",
        context: None,
    },
    Binding {
        action: "FormatStrikethrough",
        id: "editor.keymap.FormatStrikethrough",
        label: "删除线",
        desc: "包裹选中文本为 ~~删除线~~ · 留空用默认 cmd-shift-x（重启后生效）",
        default: "cmd-shift-x",
        context: None,
    },
    Binding {
        action: "FormatInlineCode",
        id: "editor.keymap.FormatInlineCode",
        label: "行内代码",
        desc: "包裹选中文本为 `行内代码` · 留空用默认 cmd-e（重启后生效）",
        default: "cmd-e",
        context: None,
    },
    Binding {
        action: "FormatLink",
        id: "editor.keymap.FormatLink",
        label: "链接",
        desc: "把选中文本变成 [文字](链接) · 留空用默认 cmd-k（重启后生效）",
        default: "cmd-k",
        context: None,
    },
    Binding {
        action: "ToggleTheme",
        id: "editor.keymap.ToggleTheme",
        label: "切换明暗主题",
        desc: "在浅色 / 深色之间切换 · 留空用默认 cmd-shift-l（重启后生效）",
        default: "cmd-shift-l",
        context: None,
    },
    Binding {
        action: "OpenCommandPalette",
        id: "editor.keymap.OpenCommandPalette",
        label: "命令面板",
        desc: "按文件名搜索并打开笔记 · 留空用默认 cmd-p（重启后生效）",
        default: "cmd-p",
        context: None,
    },
    Binding {
        action: "OpenSearch",
        id: "editor.keymap.OpenSearch",
        label: "全文搜索",
        desc: "按内容检索整个库 · 留空用默认 cmd-shift-f（重启后生效）",
        default: "cmd-shift-f",
        context: None,
    },
    Binding {
        action: "ToggleAiChat",
        id: "editor.keymap.ToggleAiChat",
        label: "AI 对话条",
        desc: "打开 / 关闭编辑器下方的 AI 对话条 · 留空用默认 cmd-shift-a（重启后生效）",
        default: "cmd-shift-a",
        context: None,
    },
];

/// 解析结果：一个动作最终生效的键位
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// action 名
    pub action: &'static str,
    pub keystroke: String,
    /// 键位上下文
    pub context: Option<&'static str>,
}

/// 归一化：去首尾空白、内部空白压成一个空格、转小写
///
/// `CMD-S` 与 ` cmd-s ` 等价，避免手改配置时因为大小写被判成无效。
pub fn normalize(raw: &str) -> String {
    raw.split_whitespace()
        .map(|part| part.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 键串是否合法：逐段用框架的 `Keystroke::parse` 校验
///
/// 多段（如 `cmd-k cmd-s`）解析能通过，但**不承诺**行为正确——
/// 见设计文档 §9 风险 3。
pub fn is_valid(raw: &str) -> bool {
    !raw.trim().is_empty()
        && raw
            .split_whitespace()
            .all(|part| match Keystroke::parse(part) {
                // `cmd-` 这种"只有修饰键"的串能解析通过（key 为空串），
                // 但绑上去不会有任何按键能触发，因此判为非法。
                Ok(keystroke) => !keystroke.key.is_empty(),
                Err(_) => false,
            })
}

/// 取某个动作的覆盖值（没有覆盖就是空串）
pub fn override_of(overrides: &BTreeMap<String, String>, action: &str) -> String {
    overrides
        .get(action)
        .map(|raw| normalize(raw))
        .unwrap_or_default()
}

/// 覆盖表 → 最终键位表
///
/// 规则（设计文档 §3.1）：
/// - 覆盖值为空、或非法 → 用默认键位
/// - 默认键位为空 → 该动作不产生绑定
/// - 键位冲突（多个动作抢同一个键串）→ 保留表中靠前的，其余丢弃并打日志
pub fn resolve(overrides: &BTreeMap<String, String>) -> Vec<Resolved> {
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();

    for binding in BINDINGS {
        let candidate = override_of(overrides, binding.action);
        let preferred = if !candidate.is_empty() && is_valid(&candidate) {
            candidate
        } else {
            binding.default.to_string()
        };
        // 覆盖值与更靠前的动作撞了：退回默认键位，尽量不让用户"改完没反应"
        let keystroke = if used.contains(&preferred) {
            binding.default.to_string()
        } else {
            preferred
        };
        if keystroke.is_empty() {
            continue;
        }
        if used.contains(&keystroke) {
            eprintln!(
                "[xink] 快捷键冲突：{keystroke} 已绑给更靠前的动作，{} 这次不生效",
                binding.action
            );
            continue;
        }
        used.insert(keystroke.clone());
        out.push(Resolved {
            action: binding.action,
            keystroke,
            context: binding.context,
        });
    }

    out
}

/// 配置校正：把 `editor.keymap` 里的垃圾项清掉
///
/// 与 `AppConfig::sanitize` 的其它钳制同一套路——**不留"存了但不生效"的键**：
/// 空值、非法键串、未知动作名一律删除（等价于"用默认键位"）。
pub fn sanitize(overrides: &mut BTreeMap<String, String>) {
    let known: BTreeSet<&str> = BINDINGS.iter().map(|b| b.action).collect();
    overrides.retain(|action, _| known.contains(action.as_str()));
    for raw in overrides.values_mut() {
        *raw = normalize(raw);
    }
    overrides.retain(|_, raw| is_valid(raw));
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/keymap.rs"]
mod tests;
