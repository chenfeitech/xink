//! SSE（Server-Sent Events）增量解析
//!
//! 只做分帧，不做业务字段解析——各家把正文放在不同字段里，
//! 那部分在 `openai.rs`。分帧必须**按事件边界**而不是按单次 socket read，
//! 因为一次 read 可能只有半帧（实测小模型约 1 字一帧）。

/// 一帧 SSE
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseFrame {
    /// `data:` 的内容（多行 data 已用 `\n` 拼接）
    Data(String),
    /// `data: [DONE]`
    Done,
}

pub struct SseParser {
    buf: String,
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    /// 追加一段收到的文本，把解析出的帧放进 `out`
    pub fn push(&mut self, chunk: &str, out: &mut Vec<SseFrame>) {
        self.buf.push_str(chunk);
        self.drain(out, false);
    }

    /// 流结束：把残留的最后一个事件也吐出来（有些实现结尾没有空行）
    pub fn finish(&mut self, out: &mut Vec<SseFrame>) {
        self.drain(out, true);
    }

    fn drain(&mut self, out: &mut Vec<SseFrame>, flush_tail: bool) {
        // 事件分隔符是空行；同时兼容 LF 与 CRLF
        while let Some(pos) = find_blank_line(&self.buf) {
            let block = self.buf[..pos].to_string();
            let rest = self.buf[pos..].to_string();
            // 跳过分隔符本身（\n\n 或 \r\n\r\n）
            let skip = if rest.starts_with("\r\n\r\n") {
                4
            } else if rest.starts_with("\n\n") {
                2
            } else {
                // 只可能是 \n\n 的变体；防御性处理单个空行
                rest.chars().take_while(|c| *c == '\n' || *c == '\r').count()
            };
            self.buf = rest[skip..].to_string();
            if let Some(frame) = parse_block(&block) {
                out.push(frame);
            }
        }
        if flush_tail && !self.buf.trim().is_empty() {
            if let Some(frame) = parse_block(&self.buf) {
                out.push(frame);
            }
            self.buf.clear();
        }
    }
}

/// 找空行分隔符的位置
fn find_blank_line(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            let next = bytes.get(i + 1).copied();
            if next == Some(b'\n') {
                return Some(i);
            }
            // \r\n\r\n：i 指向第一个 \n，后面是 \r\n
            if next == Some(b'\r') && bytes.get(i + 2).copied() == Some(b'\n') {
                return Some(i);
            }
        }
    }
    None
}

/// 解析一个事件块：只认 `data:`，忽略注释（`:` 开头）与其它字段
fn parse_block(block: &str) -> Option<SseFrame> {
    let mut data: Vec<&str> = Vec::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.trim_start_matches(' '));
        }
    }
    if data.is_empty() {
        return None;
    }
    let joined = data.join("\n");
    if joined.trim() == "[DONE]" {
        Some(SseFrame::Done)
    } else {
        Some(SseFrame::Data(joined))
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/sse.rs"]
mod tests;
