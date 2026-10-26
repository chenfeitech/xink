//! SSE 分帧：`tests/` 镜像 `src/`（见 docs/TESTS.md）

use crate::core::ai::sse::{SseFrame, SseParser};

fn parse(chunks: &[&str], finish: bool) -> Vec<SseFrame> {
    let mut parser = SseParser::new();
    let mut out = Vec::new();
    for c in chunks {
        parser.push(c, &mut out);
    }
    if finish {
        parser.finish(&mut out);
    }
    out
}

#[test]
fn 完整帧与结束标记() {
    let frames = parse(&["data: {\"a\":1}\n\n", "data: [DONE]\n\n"], true);
    assert_eq!(
        frames,
        vec![SseFrame::Data("{\"a\":1}".into()), SseFrame::Done]
    );
}

#[test]
fn 半包跨_read_仍能拼出完整帧() {
    // 实测 Ollama 小模型约 1 字一帧，半包是常态而非异常
    let frames = parse(&["data: {\"a\":", "1}\n\nda", "ta: [DONE]\n\n"], true);
    assert_eq!(
        frames,
        vec![SseFrame::Data("{\"a\":1}".into()), SseFrame::Done]
    );
}

#[test]
fn crlf_与注释行() {
    let frames = parse(&[": keep-alive\r\n\r\ndata: hi\r\n\r\n"], true);
    assert_eq!(frames, vec![SseFrame::Data("hi".into())]);
}

#[test]
fn 结尾没有空行时_finish_补出最后一帧() {
    let frames = parse(&["data: tail"], true);
    assert_eq!(frames, vec![SseFrame::Data("tail".into())]);
}

#[test]
fn 多行_data_合并() {
    let frames = parse(&["data: a\ndata: b\n\n"], true);
    assert_eq!(frames, vec![SseFrame::Data("a\nb".into())]);
}

#[test]
fn 空_usage_帧不产生事件内容() {
    // OpenAI 在 include_usage 时会发 choices: [] 的帧，这里只负责分帧
    let frames = parse(&["data: {\"choices\":[]}\n\n"], true);
    assert_eq!(frames, vec![SseFrame::Data("{\"choices\":[]}".into())]);
}
