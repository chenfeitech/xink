use super::*;

#[test]
fn rewrites_relative_image_to_file_url() {
    let out = resolve_image_urls("![封面](assets/cover.png)", Path::new("/notes"));
    assert!(
        out.contains("file:///notes/assets/cover.png"),
        "应补全为 file:// URL，实际: {out}"
    );
}

#[test]
fn keeps_remote_and_absolute_urls() {
    let md = "![a](https://x.com/a.png) ![b](/abs/b.png) ![c](data:image/png;base64,xx)";
    let out = resolve_image_urls(md, Path::new("/notes"));
    assert!(out.contains("https://x.com/a.png"));
    assert!(out.contains("(/abs/b.png)"));
    assert!(out.contains("data:image/png;base64,xx"));
    assert!(!out.contains("file://"), "绝对 URL 不应被改写: {out}");
}

#[test]
fn encodes_spaces_and_non_ascii() {
    let out = resolve_image_urls("![a](my image.png)", Path::new("/n"));
    assert!(out.contains("my%20image.png"), "空格应被编码: {out}");
}

#[test]
fn leaves_plain_text_untouched() {
    let md = "没有图片的段落";
    assert_eq!(resolve_image_urls(md, Path::new("/n")), md);
}

// ── 预览前的防御性预处理 ──────────────────────────────────────

#[test]
fn sanitize_separates_consecutive_thematic_breaks() {
    // 这正是让解析器 panic 的输入形态：段落 / --- / --- / 段落 / ---
    let src = "a\n---\n---\nb\n---\n";
    assert_eq!(sanitize_for_preview(src), "a\n---\n\n---\nb\n---\n");
}

#[test]
fn sanitize_handles_three_consecutive_breaks() {
    assert_eq!(sanitize_for_preview("---\n---\n---\n"), "---\n\n---\n\n---\n");
}

#[test]
fn sanitize_leaves_single_break_untouched() {
    let src = "段落一\n\n---\n\n段落二\n";
    assert_eq!(sanitize_for_preview(src), src, "单个分隔线无需改动");
}

#[test]
fn sanitize_preserves_frontmatter() {
    // frontmatter 的两条 --- 并不相邻，不能被插入空行破坏
    let src = "---\ntitle: 标题\n---\n\n正文\n";
    assert_eq!(sanitize_for_preview(src), src);
}

#[test]
fn sanitize_ignores_table_delimiter_rows() {
    // 表格分隔行含 `|`，不是分隔线
    let src = "| 列1 | 列2 |\n| --- | --- |\n| a | b |\n";
    assert_eq!(sanitize_for_preview(src), src);
}

#[test]
fn sanitize_ignores_short_dashes() {
    // `-` / `--` 长度不足 3，不视为分隔线
    let src = "-\n--\n---\n";
    assert_eq!(sanitize_for_preview(src), "-\n--\n---\n");
}

#[test]
fn sanitize_preserves_missing_trailing_newline() {
    assert_eq!(sanitize_for_preview("正文"), "正文");
    assert_eq!(sanitize_for_preview("a\n---\n---\nb"), "a\n---\n\n---\nb");
}

#[test]
fn sanitize_is_idempotent() {
    let src = "a\n---\n---\nb\n---\n";
    let once = sanitize_for_preview(src);
    assert_eq!(sanitize_for_preview(&once), once, "重复处理不应继续增长");
}

// ── 回归：预处理后的内容不得让预览解析器 panic ──────────────────
//
// 说明：`markdown` 仅作为 dev-dependency 引入，用于复现 gpui-base 内部
// （版本一致）的解析行为；生产代码不直接依赖它。

/// 与 gpui-base `MarkdownExtensions::parse_options()` 保持一致
fn parse_options_like_gpui(frontmatter: bool) -> markdown::ParseOptions {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = frontmatter;
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

fn parser_panics(src: &str) -> bool {
    // 解析在独立线程里做：库内部是 panic（不是返回 Err），只能靠捕获来判断
    std::thread::spawn({
        let src = src.to_string();
        move || {
            let a = std::panic::catch_unwind(|| {
                let _ = markdown::to_mdast(&src, &parse_options_like_gpui(false));
            });
            let b = std::panic::catch_unwind(|| {
                let _ = markdown::to_mdast(&src, &parse_options_like_gpui(true));
            });
            a.is_err() || b.is_err()
        }
    })
    .join()
    .unwrap_or(true)
}

#[test]
fn sanitized_thematic_break_sequence_does_not_panic_parser() {
    let raw = "a\n---\n---\nb\n---\n";
    // 先确认这段输入确实是崩溃源（若上游修复了，这条断言会提醒我们可以简化预处理）
    assert!(
        parser_panics(raw),
        "原始输入应当会触发解析器 panic，否则说明上游已修复、预处理可重新评估"
    );
    assert!(
        !parser_panics(&sanitize_for_preview(raw)),
        "预处理后的输入不应再触发 panic"
    );
}

#[test]
fn sanitized_common_constructs_do_not_panic_parser() {
    // 常用语法组合（含我们工具栏插入的模板）在预处理后都应安全
    let samples = [
        "| 列1 | 列2 |\n| --- | --- |\n|  |     |\n",
        "```rust\nfn main() {}\n```\n",
        "- [ ] 任务\n- [x] 完成\n",
        "> 引用\n\n---\n\n段落\n",
        "# 标题\n\n正文\n\n---\n---\n\n结尾\n",
        "![图](assets/a.png)\n\n[链接](https://x.com)\n",
    ];
    for src in samples {
        assert!(
            !parser_panics(&sanitize_for_preview(src)),
            "预处理后仍触发 panic: {src:?}"
        );
    }
}
