// 显式导入而非 `use super::*`：本文件顶部有 `use gpui_kit::*`，
// 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性，导致宏展开递归。
use super::{Format, Insertion, format_icon_svg, is_line_start};
use std::collections::HashSet;

/// 全部操作的清单。新增操作时需同步补充，否则下述遍历类用例会漏掉新操作。
const ALL: [Format; 13] = [
    Format::Bold,
    Format::Italic,
    Format::Strikethrough,
    Format::Heading,
    Format::BulletList,
    Format::NumberedList,
    Format::TaskList,
    Format::Quote,
    Format::CodeBlock,
    Format::Table,
    Format::Link,
    Format::Image,
    Format::HorizontalRule,
];

// ── 操作名称 ──────────────────────────────────────────────

#[test]
fn format_label_names_every_operation() {
    assert_eq!(Format::Bold.label(), "加粗");
    assert_eq!(Format::Italic.label(), "斜体");
    assert_eq!(Format::Strikethrough.label(), "删除线");
    assert_eq!(Format::Heading.label(), "标题");
    assert_eq!(Format::BulletList.label(), "无序列表");
    assert_eq!(Format::NumberedList.label(), "有序列表");
    assert_eq!(Format::TaskList.label(), "任务列表");
    assert_eq!(Format::Quote.label(), "引用");
    assert_eq!(Format::CodeBlock.label(), "代码块");
    assert_eq!(Format::Table.label(), "表格");
    assert_eq!(Format::Link.label(), "链接");
    assert_eq!(Format::Image.label(), "图片");
    assert_eq!(Format::HorizontalRule.label(), "分割线");
}

#[test]
fn operation_labels_are_unique_and_non_empty() {
    let mut seen = HashSet::new();
    for cmd in ALL {
        let label = cmd.label();
        assert!(!label.is_empty(), "{cmd:?} 缺少中文名称");
        assert!(
            seen.insert(label),
            "按钮名称重复，会导致状态栏提示歧义：{label}"
        );
    }
    assert_eq!(seen.len(), ALL.len());
}

// ── 图标 ──────────────────────────────────────────────────

#[test]
fn every_operation_exposes_a_wellformed_svg_icon() {
    for cmd in ALL {
        let svg = format_icon_svg(cmd);
        assert!(svg.starts_with("<svg"), "{cmd:?} 图标根节点不是 svg");
        assert!(svg.ends_with("</svg>"), "{cmd:?} 图标未闭合");
        assert!(
            svg.contains("xmlns=\"http://www.w3.org/2000/svg\""),
            "{cmd:?} 缺少 xmlns 命名空间"
        );
        assert!(
            svg.contains("viewBox=\"0 0 24 24\""),
            "{cmd:?} 画布不是 24x24，与工具栏其它图标不一致"
        );
        assert!(
            svg.contains("stroke=\"currentColor\""),
            "{cmd:?} 未使用 currentColor，图标将无法随主题变色"
        );
        assert!(
            svg.len() > "<svg></svg>".len() + 100,
            "{cmd:?} 图标内容过短，疑似空图标"
        );
    }
}

#[test]
fn icons_are_unique_per_operation() {
    let mut seen = HashSet::new();
    for cmd in ALL {
        assert!(
            seen.insert(format_icon_svg(cmd)),
            "{cmd:?} 复用了其它操作的图标，用户无法区分按钮"
        );
    }
}

// ── 行首判定：块级插入是否需要补换行的边界条件 ──────────────

#[test]
fn is_line_start_treats_text_start_and_newline_as_line_start() {
    assert!(is_line_start(None), "文本开头应视为行首（无需补换行）");
    assert!(is_line_start(Some('\n')), "换行之后应视为行首");
}

#[test]
fn is_line_start_rejects_non_newline_predecessors() {
    assert!(!is_line_start(Some('a')), "单词中间不应视为行首");
    assert!(!is_line_start(Some(' ')), "行内空格后不应视为行首");
    assert!(!is_line_start(Some('\t')), "缩进后不应视为行首");
    assert!(!is_line_start(Some('\r')), "只有 \\n 计作换行，\\r 不算");
    assert!(!is_line_start(Some('|')), "表格单元格内不应视为行首");
}

// ── 插入内容：主要路径的输入输出正确性 ──────────────────────

#[test]
fn inline_operations_wrap_selection_with_matching_markers() {
    assert_eq!(
        Format::Bold.insertion(),
        Insertion::Surround {
            before: "**",
            after: "**"
        }
    );
    assert_eq!(
        Format::Italic.insertion(),
        Insertion::Surround {
            before: "*",
            after: "*"
        }
    );
    assert_eq!(
        Format::Strikethrough.insertion(),
        Insertion::Surround {
            before: "~~",
            after: "~~"
        }
    );
    assert_eq!(
        Format::Link.insertion(),
        Insertion::Surround {
            before: "[",
            after: "](url)"
        }
    );
    assert_eq!(
        Format::Image.insertion(),
        Insertion::Surround {
            before: "![",
            after: "](url)"
        }
    );
}

#[test]
fn block_operations_insert_line_prefixes() {
    assert_eq!(Format::Heading.insertion(), Insertion::Block("# "));
    assert_eq!(Format::BulletList.insertion(), Insertion::Block("- "));
    assert_eq!(Format::NumberedList.insertion(), Insertion::Block("1. "));
    assert_eq!(Format::TaskList.insertion(), Insertion::Block("- [ ] "));
    assert_eq!(Format::Quote.insertion(), Insertion::Block("> "));
    assert_eq!(
        Format::HorizontalRule.insertion(),
        Insertion::Block("---\n")
    );
}

#[test]
fn wrapping_selected_text_produces_expected_markdown() {
    let cases = [
        (Format::Bold, "重点", "**重点**"),
        (Format::Italic, "强调", "*强调*"),
        (Format::Strikethrough, "作废", "~~作废~~"),
        (Format::Link, "官网", "[官网](url)"),
        (Format::Image, "截图", "![截图](url)"),
    ];
    for (cmd, selected, expected) in cases {
        let Insertion::Surround { before, after } = cmd.insertion() else {
            panic!("{cmd:?} 应为行内包裹操作");
        };
        assert_eq!(format!("{before}{selected}{after}"), expected);
    }
}

#[test]
fn surround_markers_leave_a_slot_for_the_cursor() {
    // 无选中时插入 before+after，光标停在 before 之后，因此两侧标记都不能为空
    for cmd in ALL {
        if let Insertion::Surround { before, after } = cmd.insertion() {
            assert!(!before.is_empty(), "{cmd:?} 缺少前置标记");
            assert!(!after.is_empty(), "{cmd:?} 缺少后置标记，光标无处可放");
        }
    }
}

#[test]
fn code_block_template_leaves_an_empty_line_inside_fences() {
    let Insertion::Template { before, after } = Format::CodeBlock.insertion() else {
        panic!("代码块应为模板插入");
    };
    assert_eq!(before, "```\n");
    assert_eq!(after, "\n```\n");
    // 光标停在 before 之后，整体等价于一个带空行的围栏代码块
    assert_eq!(format!("{before}{after}"), "```\n\n```\n");
}

#[test]
fn table_template_has_header_separator_and_data_row() {
    let Insertion::Template { before, after } = Format::Table.insertion() else {
        panic!("表格应为模板插入");
    };
    assert!(
        before.ends_with("| "),
        "光标应停在首个数据单元格内，实际前缀：{before:?}"
    );
    let rendered = format!("{before}{after}");
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "表格模板应为表头/分隔行/数据行三行：{rendered:?}"
    );
    assert!(
        lines[0].starts_with('|') && lines[0].ends_with('|'),
        "表头需以竖线包裹：{:?}",
        lines[0]
    );
    assert!(
        lines[1].contains("---"),
        "第二行必须是分隔行：{:?}",
        lines[1]
    );
    assert!(
        lines[2].starts_with('|'),
        "第三行需为数据行：{:?}",
        lines[2]
    );
    assert_eq!(lines[0].matches('|').count(), 3, "模板应为两列（3 个竖线）");
}

// ── 防御：任何操作都必须有可插入的内容 ──────────────────────

#[test]
fn every_operation_inserts_non_empty_content() {
    for cmd in ALL {
        match cmd.insertion() {
            Insertion::Surround { before, after } => assert!(
                !before.is_empty() && !after.is_empty(),
                "{cmd:?} 的包裹标记为空"
            ),
            Insertion::Block(text) => assert!(!text.is_empty(), "{cmd:?} 的块级前缀为空"),
            Insertion::Template { before, after } => assert!(
                !before.is_empty() && !after.is_empty(),
                "{cmd:?} 的模板为空"
            ),
        }
    }
}
