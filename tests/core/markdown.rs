use super::*;

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
