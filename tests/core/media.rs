use super::{FileKind, file_kind};
use std::path::Path;

use super::{is_editable_text, is_image_path};

#[test]
fn detects_common_image_formats() {
    for name in [
        "a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp", "a.bmp", "a.svg", "a.ico",
    ] {
        assert!(is_image_path(Path::new(name)), "应识别为图片: {name}");
    }
}

#[test]
fn detection_is_case_insensitive() {
    assert!(is_image_path(Path::new("/a/b.PNG")));
    assert!(is_image_path(Path::new("/a/b.JPEG")));
    assert!(is_image_path(Path::new("/a/b.SVG")));
}

#[test]
fn rejects_non_image_files() {
    for name in ["note.md", "main.rs", "config.json", "noext", "a.png.bak"] {
        assert!(!is_image_path(Path::new(name)), "不应识别为图片: {name}");
    }
}

#[test]
fn file_kind_dir_takes_precedence() {
    // 名为 x.md 的目录也不应判成笔记
    assert_eq!(file_kind(Path::new("/vault/x.md"), true), FileKind::Dir);
    assert_eq!(file_kind(Path::new("/vault/assets"), true), FileKind::Dir);
}

#[test]
fn file_kind_recognizes_markdown() {
    assert_eq!(file_kind(Path::new("note.md"), false), FileKind::Markdown);
    assert_eq!(
        file_kind(Path::new("note.markdown"), false),
        FileKind::Markdown
    );
}

#[test]
fn file_kind_recognizes_images_case_insensitively() {
    assert_eq!(file_kind(Path::new("a.png"), false), FileKind::Image);
    assert_eq!(file_kind(Path::new("b.JPG"), false), FileKind::Image);
    assert_eq!(file_kind(Path::new("c.webp"), false), FileKind::Image);
}

#[test]
fn file_kind_recognizes_code_archive_and_sheet() {
    assert_eq!(file_kind(Path::new("main.rs"), false), FileKind::Code);
    assert_eq!(file_kind(Path::new("a.tar.gz"), false), FileKind::Archive);
    assert_eq!(file_kind(Path::new("s.xlsx"), false), FileKind::Spreadsheet);
}

#[test]
fn file_kind_recognizes_data_files() {
    assert_eq!(file_kind(Path::new("c.json"), false), FileKind::Data);
    assert_eq!(file_kind(Path::new("d.yaml"), false), FileKind::Data);
    assert_eq!(file_kind(Path::new("d.yml"), false), FileKind::Data);
    assert_eq!(file_kind(Path::new("e.toml"), false), FileKind::Data);
    // 备份文件不算数据文件
    assert_eq!(file_kind(Path::new("c.json.bak"), false), FileKind::Other);
}

#[test]
fn is_editable_text_covers_markdown_and_data_files() {
    // 注：`.markdown` 只有图标判定认（`file_kind`），打开与树显示仍只认 `.md`，
    // 这是既有行为，本次不改。
    for name in ["a.md", "a.json", "a.yaml", "a.yml", "a.toml"] {
        assert!(is_editable_text(Path::new(name)), "{name} 应可编辑");
    }
    for name in ["a.txt", "a.rs", "a.png", "a", "a.json.bak"] {
        assert!(!is_editable_text(Path::new(name)), "{name} 不应可编辑");
    }
}

#[test]
fn file_kind_recognizes_audio_and_video() {
    for name in ["a.mp3", "b.wav", "c.flac", "d.m4a", "e.ogg"] {
        assert_eq!(
            file_kind(Path::new(name), false),
            FileKind::Audio,
            "{name} 应归入 Audio"
        );
    }
    for name in ["f.mp4", "g.mov", "h.mkv", "i.webm", "j.avi"] {
        assert_eq!(
            file_kind(Path::new(name), false),
            FileKind::Video,
            "{name} 应归入 Video"
        );
    }
}

#[test]
fn file_kind_falls_back_to_other() {
    // 无扩展名、未知扩展名归 Other（音视频、json/yaml/toml 已各自成类）
    for name in ["noext", "x.qqq", "a.mp3.bak"] {
        assert_eq!(
            file_kind(Path::new(name), false),
            FileKind::Other,
            "{name} 应归入 Other"
        );
    }
}

#[test]
fn file_kind_handles_multi_dot_names() {
    assert_eq!(file_kind(Path::new("a.b.png"), false), FileKind::Image);
    assert_eq!(file_kind(Path::new("a.b.md"), false), FileKind::Markdown);
}
