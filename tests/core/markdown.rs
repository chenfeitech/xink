use super::*;
use std::fs;

#[test]
fn rewrites_relative_image_to_file_url() {
    let out = resolve_image_urls(
        "![封面](assets/cover.png)",
        Path::new("/notes"),
        ImageOptions::default(),
    );
    assert!(
        out.contains("file:///notes/assets/cover.png"),
        "应补全为 file:// URL，实际: {out}"
    );
}

#[test]
fn keeps_remote_and_data_urls() {
    let md = "![a](https://x.com/a.png) ![c](data:image/png;base64,xx)";
    let out = resolve_image_urls(md, Path::new("/notes"), ImageOptions::default());
    assert!(out.contains("https://x.com/a.png"));
    assert!(out.contains("data:image/png;base64,xx"));
    // 远程与内嵌图片无法在本地预览，不改写也不包链接
    assert!(!out.contains("file://"), "远程/内嵌图不应被改写: {out}");
}

#[test]
fn rewrites_absolute_path_to_file_url() {
    // 绝对路径同样要转 file://：否则框架会把它当 URI 交给 HTTP 客户端而加载不出来
    let out = resolve_image_urls(
        "![b](/abs/b.png)",
        Path::new("/notes"),
        ImageOptions::default(),
    );
    assert!(
        out.contains("file:///abs/b.png"),
        "绝对路径应转为 file://: {out}"
    );
}

#[test]
fn local_image_becomes_data_url_and_is_clickable() {
    // 内容随意：这里只做 base64 编码，不解码图片
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shot.png");
    std::fs::write(&path, b"not-a-real-png").unwrap();

    let out = resolve_image_urls("![图](shot.png)", dir.path(), ImageOptions::default());
    assert!(
        out.contains("data:image/png;base64,"),
        "本地图片应转为 data URL: {out}"
    );
    // 渲染成 <img> 并包进指向自身的链接，点击才能打开大图（框架只有链接回调）
    assert!(
        out.starts_with("[<img src=\"data:"),
        "图片应渲染成 img 并包成链接: {out}"
    );
    assert!(out.contains("alt=\"图\""), "应保留 alt: {out}");
    assert!(
        out.contains(&format!("](file://{})", encode_path(&path))),
        "链接应指回本地路径: {out}"
    );
}

#[test]
fn missing_local_image_falls_back_to_file_url() {
    // 文件不存在：转不出 data URL，退回 file://，链接仍指向正确位置
    let out = resolve_image_urls(
        "![图](nope.png)",
        Path::new("/notes"),
        ImageOptions::default(),
    );
    assert!(out.contains("file:///notes/nope.png"), "实际: {out}");
    assert!(!out.contains("data:"), "不应编造 data URL: {out}");
}

#[test]
fn base64_matches_standard_vectors() {
    assert_eq!(encode_base64(b""), "");
    assert_eq!(encode_base64(b"f"), "Zg==");
    assert_eq!(encode_base64(b"fo"), "Zm8=");
    assert_eq!(encode_base64(b"foo"), "Zm9v");
    assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
}

/// 递归查找是否存在链接节点
fn has_link(node: &markdown::mdast::Node) -> bool {
    if matches!(node, markdown::mdast::Node::Link(_)) {
        return true;
    }
    node.children()
        .map(|cs| cs.iter().any(has_link))
        .unwrap_or(false)
}

#[test]
fn rendered_image_wrapped_in_link_is_clickable() {
    // 图片渲染成 <img> 后仍必须包在链接里：框架只有链接点击回调，
    // 不包就点不出预览窗口。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.png");
    std::fs::write(&path, b"png-bytes").unwrap();

    let out = resolve_image_urls("![图](a.png)", dir.path(), ImageOptions::default());
    assert!(out.starts_with("[<img "), "应渲染成 img: {out}");
    assert!(
        out.contains(&format!("](file://{})", encode_path(&path))),
        "img 必须被包进指向原图的链接: {out}"
    );

    // 生成的源文必须能被解析器接受并解析出链接节点
    let ast = markdown::to_mdast(&out, &parse_options_like_gpui(false)).unwrap();
    assert!(has_link(&ast), "应解析出链接节点: {out}");
}

#[test]
fn hidden_display_degrades_image_to_text_link() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"x").unwrap();

    let opts = ImageOptions {
        display: ImageDisplay::Hidden,
        width_percent: 100,
    };
    let out = resolve_image_urls("![图](a.png)", dir.path(), opts);
    assert!(!out.contains("<img"), "不应内联渲染图片: {out}");
    assert!(out.starts_with("[图]("), "应退化为文本链接: {out}");
}

#[test]
fn image_width_percent_is_written_into_style() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"x").unwrap();

    let opts = ImageOptions {
        display: ImageDisplay::FitWidth,
        width_percent: 60,
    };
    let out = resolve_image_urls("![图](a.png)", dir.path(), opts);
    assert!(out.contains("style=\"width: 60%\""), "应写入宽度: {out}");
}

#[test]
fn alt_text_containing_the_url_is_not_rewritten() {
    // 回归：URL 只应替换一次，alt 文本里的同名字符串不能被改成 data URL
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.png"), b"x").unwrap();

    let out = resolve_image_urls("![a.png](a.png)", dir.path(), ImageOptions::default());
    assert!(
        out.contains("alt=\"a.png\""),
        "alt 应保留为 a.png（URL 只替换一次）: {}",
        &out[..out.len().min(60)]
    );
}

// 图片扩展名的判据统一在 `core::media`，那里有专门的用例

#[test]
fn encodes_spaces_and_non_ascii() {
    let out = resolve_image_urls(
        "![a](my image.png)",
        Path::new("/n"),
        ImageOptions::default(),
    );
    assert!(out.contains("my%20image.png"), "空格应被编码: {out}");
}

#[test]
fn leaves_plain_text_untouched() {
    let md = "没有图片的段落";
    assert_eq!(
        resolve_image_urls(md, Path::new("/n"), ImageOptions::default()),
        md
    );
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
    assert_eq!(
        sanitize_for_preview("---\n---\n---\n"),
        "---\n\n---\n\n---\n"
    );
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

#[test]
fn relative_path_same_dir() {
    let base = Path::new("/vault/notes");
    let target = Path::new("/vault/notes/a.png");
    assert_eq!(relative_path(base, target), "a.png");
}

#[test]
fn relative_path_sub_dir() {
    let base = Path::new("/vault");
    let target = Path::new("/vault/assets/a.png");
    assert_eq!(relative_path(base, target), "assets/a.png");
}

#[test]
fn relative_path_parent_dir() {
    let base = Path::new("/vault/notes");
    let target = Path::new("/vault/assets/a.png");
    assert_eq!(relative_path(base, target), "../assets/a.png");
}

#[test]
fn markdown_link_for_image_and_file() {
    assert_eq!(
        markdown_link("a", "assets/a.png", true),
        "![a](assets/a.png)"
    );
    assert_eq!(
        markdown_link("说明", "notes/x.md", false),
        "[说明](notes/x.md)"
    );
}

#[test]
fn markdown_link_wraps_paths_with_spaces() {
    // 含空格不包裹的话 Markdown 会把链接截断
    assert_eq!(
        markdown_link("a b", "assets/a b.png", true),
        "![a b](<assets/a b.png>)"
    );
}

// ── 文档事实解析 ──────────────────────────────────────────────
//
// 设计见 docs/markdown-facts-design.md。行号一律 0-based。

fn tags_of(text: &str) -> Vec<String> {
    parse_facts(text)
        .tags
        .into_iter()
        .map(|tag| tag.name)
        .collect()
}

#[test]
fn parses_atx_headings_with_levels_and_lines() {
    let facts = parse_facts("# 一级\n正文\n### 三级\n");
    assert_eq!(facts.headings.len(), 2);
    assert_eq!(facts.headings[0].level, 1);
    assert_eq!(facts.headings[0].text, "一级");
    assert_eq!(facts.headings[0].line, 0);
    assert_eq!(facts.headings[1].level, 3);
    assert_eq!(facts.headings[1].line, 2);
}

#[test]
fn heading_ignores_closing_hashes_and_hash_without_space() {
    let facts = parse_facts("## 标题 ##\n#标签\n");
    assert_eq!(facts.headings.len(), 1, "`#标签` 是标签不是标题");
    assert_eq!(facts.headings[0].text, "标题");
    assert_eq!(tags_of("#标签"), vec!["标签"]);
}

#[test]
fn code_fence_content_is_not_a_fact() {
    let md = "```\n# 不是标题\n[[不是双链]]\n#不是标签\n```\n# 是标题\n";
    let facts = parse_facts(md);
    assert_eq!(facts.headings.len(), 1);
    assert_eq!(facts.headings[0].text, "是标题");
    assert!(facts.wiki_links.is_empty(), "围栏内的双链不算");
    assert!(facts.tags.is_empty(), "围栏内的标签不算");
}

#[test]
fn unclosed_code_fence_swallows_the_rest() {
    let facts = parse_facts("正文\n```\n# 标题\n");
    assert!(facts.headings.is_empty(), "未闭合围栏之后都不算正文");
}

#[test]
fn inline_code_is_not_a_tag_or_link() {
    let facts = parse_facts("用 `#标签` 与 `[不是链接](x)` 举例\n");
    assert!(facts.tags.is_empty());
    assert!(facts.links.is_empty());
}

#[test]
fn collects_links_including_images_and_titles() {
    let facts = parse_facts("[文档](notes/a.md) ![图](img/a.png) [带标题](b.md \"标题\")\n");
    let targets: Vec<&str> = facts.links.iter().map(|l| l.target.as_str()).collect();
    assert_eq!(targets, vec!["notes/a.md", "img/a.png", "b.md"]);
    assert_eq!(facts.links[0].text, "文档");
    assert_eq!(facts.links[2].text, "带标题");
}

#[test]
fn wikilink_supports_alias_and_anchor() {
    let facts = parse_facts("[[目标]] 与 [[目标2|别名]] 与 [[目标3#小节|显示]]\n");
    assert_eq!(facts.wiki_links.len(), 3);
    assert_eq!(facts.wiki_links[0].target, "目标");
    assert_eq!(facts.wiki_links[0].alias, None);
    assert_eq!(facts.wiki_links[1].alias.as_deref(), Some("别名"));
    assert_eq!(facts.wiki_links[2].target, "目标3");
    assert_eq!(facts.wiki_links[2].anchor.as_deref(), Some("小节"));
    assert_eq!(facts.wiki_links[2].alias.as_deref(), Some("显示"));
}

#[test]
fn empty_wikilink_is_dropped() {
    let facts = parse_facts("[[]] 与 [[ ]] 与 [[有效]]\n");
    assert_eq!(facts.wiki_links.len(), 1);
    assert_eq!(facts.wiki_links[0].target, "有效");
}

#[test]
fn tags_require_a_leading_separator_and_non_digit() {
    assert_eq!(tags_of("#想法 正文 #todo"), vec!["想法", "todo"]);
    assert!(tags_of("#123").is_empty(), "纯数字不是标签");
    assert!(
        tags_of("见 [文档](a.md#小节)").is_empty(),
        "URL 锚点不是标签"
    );
    assert!(!tags_of("（#中文标签）").is_empty(), "括号后也算标签");
}

#[test]
fn tags_support_chinese_nesting_and_hyphen() {
    assert_eq!(
        tags_of("#笔记/想法 #rust-lang #2026计划"),
        vec!["笔记/想法", "rust-lang", "2026计划"]
    );
}

#[test]
fn frontmatter_tags_are_collected() {
    let md = "---\ntitle: x\ntags: [想法, todo]\n---\n正文 #补充\n";
    let facts = parse_facts(md);
    let names: Vec<String> = facts.tags.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, vec!["想法", "todo", "补充"]);
    // frontmatter 内的 `title` 不是标签，正文标题才从第 4 行起算
    assert_eq!(facts.tags[2].line, 4);
}

#[test]
fn empty_document_yields_no_facts() {
    let facts = parse_facts("");
    assert!(facts.headings.is_empty());
    assert!(facts.links.is_empty());
    assert!(facts.wiki_links.is_empty());
    assert!(facts.tags.is_empty());
}

// ── 双链改写（预览可点）──────────────────────────────────────
//
// 设计见 docs/wikilink-design.md §4：解析不到就原样保留，
// 代码块与行内代码内一律不动。

use std::path::PathBuf;

/// 只有这两篇能解析到
fn resolver(target: &str) -> Option<PathBuf> {
    match target {
        "笔记" | "sub/目标" => Some(PathBuf::from(format!("/vault/{target}.md"))),
        _ => None,
    }
}

#[test]
fn wikilink_becomes_a_markdown_link() {
    assert_eq!(
        rewrite_wikilinks("见 [[笔记]] 结束", resolver),
        "见 [笔记](file:///vault/%E7%AC%94%E8%AE%B0.md) 结束"
    );
}

#[test]
fn alias_is_used_as_the_label() {
    assert_eq!(
        rewrite_wikilinks("[[笔记|看这里]]", resolver),
        "[看这里](file:///vault/%E7%AC%94%E8%AE%B0.md)"
    );
}

#[test]
fn anchor_keeps_the_whole_note_as_target() {
    // 目标部分仍是 `sub/目标`，锚点只影响展示
    assert_eq!(
        rewrite_wikilinks("[[sub/目标#小节]]", resolver),
        "[sub/目标](file:///vault/sub/%E7%9B%AE%E6%A0%87.md)"
    );
}

#[test]
fn unresolvable_wikilink_is_left_untouched() {
    let md = "[[不存在]] 与 [[ ]] 与 [[]]";
    assert_eq!(rewrite_wikilinks(md, resolver), md, "解析不到必须原样返回");
}

#[test]
fn code_blocks_and_inline_code_are_not_rewritten() {
    let fenced = "```\n[[笔记]]\n```\n[[笔记]]\n";
    assert_eq!(
        rewrite_wikilinks(fenced, resolver),
        "```\n[[笔记]]\n```\n[笔记](file:///vault/%E7%AC%94%E8%AE%B0.md)\n"
    );
    let inline = "例：`[[笔记]]` 与 [[笔记]]";
    assert_eq!(
        rewrite_wikilinks(inline, resolver),
        "例：`[[笔记]]` 与 [笔记](file:///vault/%E7%AC%94%E8%AE%B0.md)"
    );
}

#[test]
fn multiple_wikilinks_in_one_line() {
    assert_eq!(
        rewrite_wikilinks("[[笔记]] 与 [[sub/目标]]", resolver),
        "[笔记](file:///vault/%E7%AC%94%E8%AE%B0.md) 与 \
         [sub/目标](file:///vault/sub/%E7%9B%AE%E6%A0%87.md)"
    );
}

#[test]
fn text_without_wikilinks_is_unchanged() {
    let md = "# 标题\n普通 [链接](a.md) 与 ![图](a.png)\n";
    assert_eq!(rewrite_wikilinks(md, resolver), md);
}

/// 取出某类装饰覆盖的原文，断言用起来比比对字节范围直观
fn spans_of(text: &str, kind: SpanKind) -> Vec<String> {
    decoration_spans(text)
        .into_iter()
        .filter(|span| span.kind == kind)
        .map(|span| text[span.range].to_string())
        .collect()
}

#[test]
fn decoration_marks_heading_hashes() {
    assert_eq!(spans_of("# 一级\n", SpanKind::HeadingMarker), ["#"]);
    assert_eq!(spans_of("### 三级\n", SpanKind::HeadingMarker), ["###"]);
    // 行首缩进的标题也算
    assert_eq!(spans_of("  ## 缩进\n", SpanKind::HeadingMarker), ["##"]);
}

#[test]
fn decoration_does_not_mark_hashtag_like_text() {
    // `#` 后没有空格 → 不是标题，是普通文本/标签
    assert!(spans_of("#hashtag\n", SpanKind::HeadingMarker).is_empty());
    // 超过六级就不算标题
    assert!(spans_of("####### 七级\n", SpanKind::HeadingMarker).is_empty());
}

#[test]
fn decoration_marks_list_markers() {
    for line in ["- 项\n", "* 项\n", "+ 项\n", "1. 项\n", "12) 项\n"] {
        assert_eq!(
            spans_of(line, SpanKind::ListMarker).len(),
            1,
            "{line} 应有一个列表符号"
        );
    }
    assert_eq!(spans_of("- 项\n", SpanKind::ListMarker), ["-"]);
    assert_eq!(spans_of("12) 项\n", SpanKind::ListMarker), ["12)"]);
    // `*斜体*` 不是列表；分隔线也不是
    assert!(spans_of("*斜体*\n", SpanKind::ListMarker).is_empty());
    assert!(spans_of("---\n", SpanKind::ListMarker).is_empty());
}

#[test]
fn decoration_marks_quote_markers() {
    assert_eq!(spans_of("> 引用\n", SpanKind::QuoteMarker), [">"]);
    // 嵌套引用每个 `>` 都单独装饰
    assert_eq!(spans_of("> > 嵌套\n", SpanKind::QuoteMarker), [">", ">"]);
    // 引用里的列表符号也要能同时识别
    let md = "> - 项\n";
    assert_eq!(spans_of(md, SpanKind::QuoteMarker), [">"]);
    assert_eq!(spans_of(md, SpanKind::ListMarker), ["-"]);
}

#[test]
fn decoration_underlines_link_text() {
    let md = "见 [双链设计](./wikilink.md) 一文\n";
    assert_eq!(spans_of(md, SpanKind::LinkText), ["双链设计"]);
    // 空链接文字不产生零长度范围
    assert!(spans_of("[](a.md)\n", SpanKind::LinkText).is_empty());
    // `[a] [b]` 不是链接
    assert!(spans_of("[a] [b]\n", SpanKind::LinkText).is_empty());
}

#[test]
fn decoration_skips_fenced_code_blocks() {
    // shell 注释以 `#` 开头、YAML 里也有 `- `，围栏内一律不算 Markdown 标记
    let md = "# 标题\n\n```sh\n# 这是注释\n```\n\n- 列表\n";
    assert_eq!(spans_of(md, SpanKind::HeadingMarker), ["#"]);
    assert_eq!(spans_of(md, SpanKind::ListMarker), ["-"]);
}

#[test]
fn decoration_skips_frontmatter() {
    // frontmatter 是 YAML，里面的 `- a` 是数组项，不是 Markdown 列表
    let md = "---\ntags:\n  - a\n---\n\n# 标题\n";
    assert_eq!(spans_of(md, SpanKind::HeadingMarker), ["#"]);
    assert!(spans_of(md, SpanKind::ListMarker).is_empty());
}

#[test]
fn decoration_ranges_are_byte_offsets_of_the_whole_document() {
    // 中文是多字节，范围必须按字节算，否则会切到字符中间
    let md = "标题一\n# 中文标题 [链接](a.md)\n";
    let spans = decoration_spans(md);
    let heading = spans
        .iter()
        .find(|span| span.kind == SpanKind::HeadingMarker)
        .expect("应有标题标记");
    // "标题一\n" 是 9 个 UTF-8 字节（3+3+3+1），所以 `#` 从第 10 个字节开始
    assert_eq!(&md[heading.range.clone()], "#");
    assert_eq!(heading.range.start, "标题一\n".len());
    // 链接文字的字节范围也要能正确切出中文
    let link = spans
        .iter()
        .find(|span| span.kind == SpanKind::LinkText)
        .expect("应有链接");
    assert_eq!(&md[link.range.clone()], "链接");
}

/// 图片很多时 data URL 单张可达数 MB，命中缓存必须共享而不是拷贝
#[test]
fn image_data_url_shares_one_allocation_across_calls() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("a.png");
    fs::write(&png, [137u8, 80, 78, 71, 13, 10, 26, 10]).unwrap();

    let first = image_data_url(&png).expect("PNG 应能编码成 data URL");
    let second = image_data_url(&png).expect("第二次应命中缓存");

    assert!(
        Arc::ptr_eq(&first, &second),
        "命中缓存应复用同一份 data URL，而不是整段拷贝"
    );
    assert!(first.starts_with("data:image/png;base64,"), "实际: {first}");
}

/// 文件被改写后指纹（mtime + 长度）变了，必须重新编码而不是拿旧图
#[test]
fn image_data_url_re_encodes_when_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("a.png");
    fs::write(&png, [1u8, 2, 3]).unwrap();
    let before = image_data_url(&png).expect("应能编码");

    fs::write(&png, [9u8, 9, 9, 9, 9]).unwrap();
    let after = image_data_url(&png).expect("应能重新编码");

    assert_ne!(before, after, "内容变了不该复用旧的 data URL");
}

// ── URL / 路径解码 ────────────────────────────────────────────
//
// 图片能不能显示、点图片能不能定位到原图，全靠这几个函数；写错的表现是
// "图片不显示"或"点了没反应"，本地很难自查。

#[test]
fn file_url_round_trips_to_a_path() {
    assert_eq!(
        path_from_file_url("file:///notes/assets/cover.png"),
        Some(PathBuf::from("/notes/assets/cover.png"))
    );
    // 截图文件名很常见：`%20` 必须还原成空格
    assert_eq!(
        path_from_file_url("file:///notes/a%20b.png"),
        Some(PathBuf::from("/notes/a b.png"))
    );
    assert_eq!(
        path_from_file_url("https://x.io/a.png"),
        None,
        "远程不算文件"
    );
    assert_eq!(path_from_file_url(""), None);
}

#[test]
fn percent_decoding_handles_bad_and_utf8_input() {
    assert_eq!(decode_percent("a%20b"), "a b");
    assert_eq!(decode_percent("%E4%B8%AD"), "中", "多字节要能还原");
    assert_eq!(decode_percent("a%zzb"), "a%zzb", "非法转义原样保留，不能崩");
    assert_eq!(decode_percent("100%"), "100%", "结尾的孤立 % 不能吞掉字符");
    assert_eq!(decode_percent("a%2Fb"), "a/b", "与 encode_path 对称");
    assert_eq!(decode_percent("a%2fb"), "a/b", "小写十六进制也要认");
    assert_eq!(decode_percent("a+b"), "a+b", "+ 不是空格（不是 form 编码）");
}

#[test]
fn remote_urls_are_left_alone() {
    assert!(is_remote_url("https://x.io/a.png"));
    assert!(is_remote_url("http://x.io/a.png"));
    assert!(!is_remote_url("file:///a.png"));
    assert!(!is_remote_url("assets/a.png"));
}

#[test]
fn mime_looks_at_the_lowercased_extension() {
    assert_eq!(
        mime_for(Path::new("a.PNG")),
        Some("image/png"),
        "扩展名要忽略大小写"
    );
    assert_eq!(mime_for(Path::new("a.JPEG")), Some("image/jpeg"));
    assert_eq!(mime_for(Path::new("a.svg")), Some("image/svg+xml"));
    assert_eq!(mime_for(Path::new("a.webp")), Some("image/webp"));
    assert_eq!(mime_for(Path::new("a.txt")), None, "未知扩展名不猜");
    assert_eq!(mime_for(Path::new("noext")), None);
}

#[test]
fn thematic_break_needs_three_identical_marks() {
    for line in ["---", "***", "___", "  ---  ", "-----"] {
        assert!(is_thematic_break(line), "{line} 应是分隔线");
    }
    // 三个不同 / 不足三个 / 混了别的字符，都不算
    for line in ["--", "-*-", "---x", "# 标题", ""] {
        assert!(!is_thematic_break(line), "{line} 不该是分隔线");
    }
}

#[test]
fn replace_last_only_touches_the_final_occurrence() {
    // alt 文本里常常出现与 URL 相同的字符串：`![a.png](a.png)`
    assert_eq!(
        replace_last("![a.png](a.png)", "a.png", "b.png"),
        "![a.png](b.png)"
    );
    assert_eq!(replace_last("no match", "x", "y"), "no match");
}
