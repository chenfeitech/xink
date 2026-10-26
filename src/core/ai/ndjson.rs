//! NDJSON 增量行切分（Ollama native `/api/chat` 用这个）
//!
//! 与 SSE 的区别：没有空行分帧，一行一个 JSON。仍然不能假设一次 read 就是一整行。

pub struct NdjsonParser {
    buf: String,
}

impl Default for NdjsonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl NdjsonParser {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    pub fn push(&mut self, chunk: &str, out: &mut Vec<String>) {
        self.buf.push_str(chunk);
        self.drain(out, false);
    }

    pub fn finish(&mut self, out: &mut Vec<String>) {
        self.drain(out, true);
    }

    fn drain(&mut self, out: &mut Vec<String>, flush_tail: bool) {
        while let Some(pos) = self.buf.find('\n') {
            let line = self.buf[..pos].trim_end_matches('\r').to_string();
            self.buf = self.buf[pos + 1..].to_string();
            if !line.trim().is_empty() {
                out.push(line);
            }
        }
        if flush_tail && !self.buf.trim().is_empty() {
            out.push(self.buf.trim_end_matches('\r').to_string());
            self.buf.clear();
        }
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/ndjson.rs"]
mod tests;
