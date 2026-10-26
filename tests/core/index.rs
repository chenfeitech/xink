use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

fn note(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, body).unwrap();
    path
}

fn index_of(root: &Path) -> NoteIndex {
    NoteIndex::build(root).expect("构建索引")
}

// ── 构建范围 ──────────────────────────────────────────────────

#[test]
fn build_collects_nested_markdown_only() {
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "# A");
    note(root, "sub/b.md", "# B");
    note(root, "sub/deep/c.md", "# C");
    // 数据文件与图片不进索引
    note(root, "conf.json", "{}");
    note(root, "logo.png", "x");

    let index = index_of(root);
    assert_eq!(index.len(), 3, "只有 .md 进索引");
    assert_eq!(index.root(), Some(root));
}

#[test]
fn build_skips_ai_dir() {
    // `.xink/` 是 AI 的机器目录（skills 等）：`vault::should_ignore` 恒跳，
    // 因此目录树里就没有它，索引里也不该出现。这条锁住 M2 的行为。
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "# A");
    note(root, ".xink/skills/summarize/SKILL.md", "# Skill");

    let index = index_of(root);
    assert_eq!(index.len(), 1, "AI 目录里的 .md 不该进索引");
    assert!(
        index
            .notes()
            .all(|n| !n.path.to_string_lossy().contains(".xink")),
        "索引里不应出现 .xink 下的笔记"
    );
}

#[test]
fn build_is_empty_when_no_vault() {
    let index = NoteIndex::new();
    assert!(index.is_empty());
    assert_eq!(index.root(), None);
    assert!(index.backlinks(Path::new("/x/a.md")).is_empty());
    assert!(index.tags().is_empty());
    assert_eq!(index.resolve_wikilink("a"), Resolution::Missing);
}

// ── 标题 ──────────────────────────────────────────────────────

#[test]
fn title_prefers_h1_then_any_heading_then_filename() {
    let dir = tmp();
    let root = dir.path();
    let h1 = note(root, "h1.md", "前言\n# 真正的标题\n## 小节\n");
    let other = note(root, "other.md", "## 只有二级\n");
    let plain = note(root, "plain.md", "没有标题\n");

    let index = index_of(root);
    assert_eq!(index.note(&h1).unwrap().title, "真正的标题");
    assert_eq!(index.note(&other).unwrap().title, "只有二级");
    assert_eq!(index.note(&plain).unwrap().title, "plain", "退化到文件名");
}

// ── 反向链接 ──────────────────────────────────────────────────

#[test]
fn backlinks_list_sources_with_lines() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "# 目标\n");
    note(root, "one.md", "见 [[target]]\n尾行\n");
    note(root, "sub/two.md", "也见 [[target|别名]]\n");

    let index = index_of(root);
    let refs = index.backlinks(&target);
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert_eq!(refs[0].from, root.join("one.md"));
    assert_eq!(refs[0].line, 0);
    assert_eq!(refs[0].target, "target");
    assert_eq!(refs[1].from, root.join("sub/two.md"));
    assert_eq!(refs[1].target, "target");
}

#[test]
fn backlinks_ignore_self_and_disappear_after_removal() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "自引 [[target]]\n");
    let source = note(root, "source.md", "引用 [[target]]\n");

    let mut index = index_of(root);
    assert!(
        index.backlinks(&target).iter().all(|r| r.from != target),
        "自引不算反链"
    );

    index.remove_file(&source);
    assert!(index.backlinks(&target).is_empty(), "来源被删后反链消失");
}

// ── 标签 ──────────────────────────────────────────────────────

#[test]
fn tags_are_aggregated_case_insensitively() {
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "#rust #想法\n");
    note(root, "b.md", "#Rust #想法\n");
    note(root, "c.md", "#日记\n");

    let index = index_of(root);
    let tags = index.tags();
    assert_eq!(
        tags,
        vec![
            // 数量相同按字典序（ASCII 在前）
            ("rust".to_string(), 2),
            ("想法".to_string(), 2),
            ("日记".to_string(), 1)
        ],
        "大小写归一 + 按数量降序：{tags:?}"
    );
    assert_eq!(
        index.notes_with_tag("#RUST"),
        vec![root.join("a.md"), root.join("b.md")]
    );
}

// ── 双链解析 ──────────────────────────────────────────────────

#[test]
fn wikilink_resolves_by_relative_path_filename_and_title() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "sub/target.md", "# 真正的标题\n");

    let index = index_of(root);
    assert_eq!(
        index.resolve_wikilink("sub/target"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("sub/target.md"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("target"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("真正的标题"),
        Resolution::Hit(target)
    );
    assert_eq!(index.resolve_wikilink("不存在"), Resolution::Missing);
    assert_eq!(index.resolve_wikilink(""), Resolution::Missing);
}

#[test]
fn wikilink_reports_ambiguity_for_duplicate_names() {
    let dir = tmp();
    let root = dir.path();
    let a = note(root, "x/a.md", "");
    let b = note(root, "y/a.md", "");

    let mut hits = match index_of(root).resolve_wikilink("a") {
        Resolution::Ambiguous(hits) => hits,
        other => panic!("应为歧义: {other:?}"),
    };
    hits.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(hits, expected);
}

// ── 增量更新 ──────────────────────────────────────────────────

#[test]
fn reindex_after_save_updates_backlinks_and_tags() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "# 目标\n");
    let source = note(root, "source.md", "草稿\n");

    let mut index = index_of(root);
    assert!(index.backlinks(&target).is_empty());
    assert!(index.tags().is_empty());

    // 编辑并保存：内容与事实都变了
    fs::write(&source, "见 [[target]] #想法\n").unwrap();
    index.reindex_file(&source);

    assert_eq!(index.backlinks(&target).len(), 1, "保存后反链应出现");
    assert_eq!(index.tags(), vec![("想法".to_string(), 1)]);
}

#[test]
fn reindex_is_idempotent() {
    let dir = tmp();
    let root = dir.path();
    let only = note(root, "only.md", "[[x]] #a\n");

    let mut index = index_of(root);
    let before = index.len();
    for _ in 0..3 {
        index.reindex_file(&only);
    }
    assert_eq!(index.len(), before, "重复重解析不应产生重复条目");
    assert_eq!(index.tags(), vec![("a".to_string(), 1)]);
}

#[test]
fn reindex_ignores_data_files_and_missing_paths() {
    let dir = tmp();
    let root = dir.path();
    let data = note(root, "conf.json", "{}");

    let mut index = index_of(root);
    index.reindex_file(&data);
    assert!(index.is_empty(), "数据文件不进索引");

    index.reindex_file(&root.join("不存在.md"));
    assert!(index.is_empty(), "读不到的文件不进索引");
}

// ── 派生表一致性 ──────────────────────────────────────────────
//
// 设计见 `docs/navigation/index-derived-tables-design.md` §7。
// 派生表是增量维护的，命门就在"摘旧 / 建新"这一步；下面让两条路径
// （一次性 build vs 反复 detach+attach）跑同一批数据，逐项比对查询结果。

/// 把一个索引的全部查询口收成一份可比对的字符串
fn snapshot(index: &NoteIndex, probe: &[PathBuf]) -> String {
    let mut out = String::new();
    for path in probe {
        out.push_str(&format!(
            "backlinks {path:?} = {:?}\n",
            index.backlinks(path)
        ));
    }
    out.push_str(&format!("tags = {:?}\n", index.tags()));
    for (tag, _) in index.tags() {
        out.push_str(&format!("tag {tag} = {:?}\n", index.notes_with_tag(&tag)));
    }
    for target in ["a", "b", "sub/b", "sub/b.md", "真正的标题", "不存在的笔记"] {
        out.push_str(&format!(
            "resolve {target} = {:?}\n",
            index.resolve_wikilink(target)
        ));
    }
    out
}

#[test]
fn incremental_updates_match_full_build() {
    let dir = tmp();
    let root = dir.path();
    let a = note(
        root,
        "a.md",
        "# 真正的标题\n\n#rust #想法 [[b]] [[sub/b]] [[不存在的笔记]]\n",
    );
    let b = note(root, "b.md", "# B\n\n#想法 [[a]] [[a|别名]] 自引 [[b]]\n");
    let c = note(root, "sub/b.md", "# 另一个B\n\n#日记 [[a]]\n");
    let probe = vec![a, b, c];

    let expected = snapshot(&index_of(root), &probe);

    // 每篇都 detach + attach 一遍
    let mut rebuilt = index_of(root);
    for path in &probe {
        rebuilt.reindex_file(path);
    }
    assert_eq!(
        snapshot(&rebuilt, &probe),
        expected,
        "逐篇重解析后应与全量构建一致"
    );

    // 全删再全加
    let mut cycled = index_of(root);
    for path in &probe {
        cycled.remove_file(path);
    }
    assert!(cycled.tags().is_empty(), "全部摘除后标签表应为空");
    assert!(
        cycled.backlinks(&probe[0]).is_empty(),
        "全部摘除后反链表应为空"
    );
    for path in &probe {
        cycled.reindex_file(path);
    }
    assert_eq!(
        snapshot(&cycled, &probe),
        expected,
        "全删全加后应与全量构建一致"
    );
}

/// 随机增删改 200 轮，每轮都和"此刻重新全量构建"比对
///
/// 固定种子的 LCG：失败可复现，且覆盖人肉想不到的组合
/// （同一路径反复删建、标签与双链同时变化、自引的出现与消失）。
#[test]
fn stays_consistent_under_random_add_edit_remove() {
    let dir = tmp();
    let root = dir.path();
    let names = ["a.md", "b.md", "sub/c.md", "sub/d.md", "e.md"];
    for (ix, name) in names.iter().enumerate() {
        if let Some(parent) = root.join(name).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        note(root, name, &format!("# 标题{ix}\n\n#t{ix} [[a]]\n"));
    }

    let mut index = index_of(root);
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };

    for round in 0..200 {
        let ix = next() % names.len();
        let path = root.join(names[ix]);
        let op = next() % 3;
        match op {
            // 改写：换标签与双链
            0 => {
                let tag = next() % 6;
                let target = names[next() % names.len()].trim_end_matches(".md");
                fs::write(&path, format!("# 标题{ix}\n\n#t{tag} [[{target}]]\n")).unwrap();
                index.reindex_file(&path);
            }
            // 删除
            1 => {
                let _ = fs::remove_file(&path);
                index.remove_file(&path);
            }
            // 重建（内容随机）
            _ => {
                let tag = next() % 6;
                fs::write(&path, format!("# 标题{ix}\n\n#t{tag} [[a]] [[b]]\n")).unwrap();
                index.reindex_file(&path);
            }
        }

        let probe: Vec<PathBuf> = names.iter().map(|n| root.join(n)).collect();
        assert_eq!(
            snapshot(&index, &probe),
            snapshot(&index_of(root), &probe),
            "第 {round} 轮（操作 {op} 于 {}）增量结果与全量构建不一致",
            names[ix]
        );
    }
}

/// 同一篇笔记里重复写同一个标签只算一篇
///
/// `by_tag` 用 `HashSet` 而非 `Vec`：这条锁住它与旧实现（靠 `contains` 去重）
/// 的计数口径一致。
#[test]
fn duplicate_tags_in_one_note_count_once() {
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "#rust #RUST #rust #想法\n");

    let index = index_of(root);
    assert_eq!(
        index.tags(),
        vec![("rust".to_string(), 1), ("想法".to_string(), 1)],
        "重复标签应只算一篇: {:?}",
        index.tags()
    );
    assert_eq!(index.notes_with_tag("rust").len(), 1);
}

/// 同一篇笔记被多种写法命中时，引用不重复
///
/// 根目录下的 `a.md` 的 stem 与"相对路径去扩展名"都是 `a`，
/// `link_keys` 去重前会算出两条引用。
#[test]
fn one_link_yields_one_backlink_ref() {
    let dir = tmp();
    let root = dir.path();
    let a = note(root, "a.md", "# A\n");
    note(root, "src.md", "[[a]]\n");

    let index = index_of(root);
    assert_eq!(
        index.backlinks(&a).len(),
        1,
        "同一条双链只应产生一条反链: {:?}",
        index.backlinks(&a)
    );
}

// ── 改名 ──────────────────────────────────────────────────────
//
// 监听把改名报成 `[旧路径, 新路径]`（`core/watcher.rs::classify_kind`），
// 两个路径都会走 `reindex_file`。旧路径读不到文件，若不摘除就会在索引里
// 永久留下一篇并不存在的笔记。

#[test]
fn reindex_drops_entry_when_the_file_is_gone() {
    let dir = tmp();
    let root = dir.path();
    let old = note(root, "old.md", "# 旧\n\n#想法 [[other]]\n");
    let other = note(root, "other.md", "# 其它\n");

    let mut index = index_of(root);
    assert!(index.note(&old).is_some());
    assert!(index.tags().iter().any(|(t, _)| t == "想法"));
    assert_eq!(index.backlinks(&other).len(), 1);

    // 模拟改名：旧路径消失，新路径出现
    let renamed = root.join("new.md");
    fs::rename(&old, &renamed).unwrap();
    for path in [&old, &renamed] {
        index.reindex_file(path);
    }

    assert!(index.note(&old).is_none(), "旧路径应被摘掉");
    assert!(index.note(&renamed).is_some(), "新路径应被登记");
    assert_eq!(
        index.tags(),
        vec![("想法".to_string(), 1)],
        "标签计数不该把新旧两条都算上: {:?}",
        index.tags()
    );
    assert!(
        index.backlinks(&other).iter().all(|r| r.from != old),
        "旧路径写下的引用应被摘掉: {:?}",
        index.backlinks(&other)
    );
    assert_eq!(index.backlinks(&other).len(), 1, "改名后的引用应接上");
}

/// 改成非笔记扩展名：旧条目摘掉，新路径不进索引
#[test]
fn reindex_drops_entry_when_renamed_out_of_markdown() {
    let dir = tmp();
    let root = dir.path();
    let old = note(root, "a.md", "# A\n\n#t\n");

    let mut index = index_of(root);
    let renamed = root.join("a.txt");
    fs::rename(&old, &renamed).unwrap();
    for path in [&old, &renamed] {
        index.reindex_file(path);
    }

    assert!(index.note(&old).is_none(), "旧笔记条目应被摘掉");
    assert!(index.note(&renamed).is_none(), "非笔记不该进索引");
    assert!(
        index.tags().is_empty(),
        "标签也应一起摘掉: {:?}",
        index.tags()
    );
}

/// 一轮改名之后，增量维护的索引仍与全量构建一致
#[test]
fn stays_consistent_after_a_rename() {
    let dir = tmp();
    let root = dir.path();
    let a = note(root, "a.md", "# A\n\n#t [[b]]\n");
    let b = note(root, "b.md", "# B\n\n#t [[a]]\n");

    let mut index = index_of(root);
    let c = root.join("c.md");
    fs::rename(&a, &c).unwrap();
    for path in [&a, &c] {
        index.reindex_file(path);
    }

    let probe = vec![a.clone(), b, c.clone()];
    assert_eq!(
        snapshot(&index, &probe),
        snapshot(&index_of(root), &probe),
        "改名后增量结果应与全量构建一致"
    );
    assert!(index.note(&a).is_none());
    assert!(index.note(&c).is_some());
}
