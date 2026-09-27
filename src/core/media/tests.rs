use std::path::Path;

use super::is_image_path;

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
