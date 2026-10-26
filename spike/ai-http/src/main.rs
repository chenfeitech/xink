//! Spike：验证 ureq 对 Ollama 的两种流式协议（native NDJSON / OpenAI 兼容 SSE）
use std::io::{BufRead, BufReader, Read};
use std::time::Instant;

const BASE: &str = "http://localhost:11434";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let model = args.get(1).map(|s| s.as_str()).unwrap_or("qwen3:0.6b");
    let think: bool = args.get(2).map(|s| s == "think").unwrap_or(false);
    println!("think={think}");

    // 1) 模型列表（native）
    let tags: serde_json::Value = ureq::get(format!("{BASE}/api/tags")).call()?.into_body().read_json()?;
    println!("== /api/tags ==");
    for m in tags["models"].as_array().unwrap_or(&vec![]) {
        let caps = m["capabilities"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
        println!("  {:24} ctx={:?} caps=[{caps}]",
            m["name"].as_str().unwrap_or("?"),
            m["details"]["context_length"].as_u64());
    }

    // 2) native /api/chat（NDJSON 流式）
    println!("\n== /api/chat (native NDJSON, model={model}) ==");
    let body = serde_json::json!({
        "model": model,
        "stream": true,
        "think": think,
        "options": { "num_ctx": 4096, "num_predict": 200 },
        "messages": [ {"role":"user","content":"用一句话说明什么是本地优先（local-first）软件。"} ]
    });
    let t0 = Instant::now();
    let mut resp = ureq::post(format!("{BASE}/api/chat")).send_json(body)?;
    println!("  状态码 {}  首字节耗时 {:?}", resp.status(), t0.elapsed());
    let mut reader = BufReader::new(resp.body_mut().as_reader());
    let (mut text, mut frames, mut first, mut think_chars) = (String::new(), 0u32, None, 0usize);
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 { break; }
        if line.trim().is_empty() { continue; }
        frames += 1;
        let v: serde_json::Value = match serde_json::from_str(&line) { Ok(v) => v, Err(e) => { println!("  解析失败: {e}"); break; } };
        if let Some(c) = v["message"]["content"].as_str() {
            if first.is_none() { first = Some(t0.elapsed()); }
            text.push_str(c);
            print!("{c}");
        }
        if let Some(t) = v["thinking"].as_str() { think_chars += t.chars().count(); }
        if v["done"].as_bool() == Some(true) {
            println!("\n  [done] eval_count={:?} eval_duration={:?}", v["eval_count"], v["eval_duration"]);
            break;
        }
    }
    println!("  帧数={frames} 首 token={:?} 总耗时={:?} 正文字数={} 思考字数={think_chars}", first, t0.elapsed(), text.chars().count());

    // 3) OpenAI 兼容 /v1/chat/completions（SSE）
    println!("\n== /v1/chat/completions (SSE, model={model}) ==");
    let body = serde_json::json!({
        "model": model,
        "stream": true,
        "think": think,
        "max_tokens": 200,
        "messages": [ {"role":"user","content":"用一句话说明什么是本地优先（local-first）软件。"} ]
    });
    let t1 = Instant::now();
    let mut resp = ureq::post(format!("{BASE}/v1/chat/completions"))
        .header("Content-Type", "application/json")
        .send(body.to_string())?;
    println!("  状态码 {}  content-type={:?}", resp.status(), resp.headers().get("content-type").map(|v| v.to_str().unwrap_or("?")));
    let mut reader = BufReader::new(resp.body_mut().as_reader());
    let (mut text2, mut chunks, mut first2, mut reason_chars) = (String::new(), 0u32, None, 0usize);
    let mut buf = String::new();
    let mut raw = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 { break; }
        raw.push(line.clone());
        if line.trim().is_empty() {
            // 事件边界：处理 buf 里的 data: 段
            if !buf.is_empty() {
                for d in buf.lines() {
                    let d = d.trim();
                    let payload = match d.strip_prefix("data:") { Some(p) => p.trim(), None => continue };
                    if payload == "[DONE]" { println!("\n  [DONE]"); buf.clear(); break; }
                    let v: serde_json::Value = serde_json::from_str(payload)?;
                    chunks += 1;
                    if let Some(r) = v["choices"].get(0).and_then(|c| c["delta"]["reasoning"].as_str()) { reason_chars += r.chars().count(); }
                    if let Some(c) = v["choices"].get(0).and_then(|c| c["delta"]["content"].as_str()) {
                        if first2.is_none() { first2 = Some(t1.elapsed()); }
                        text2.push_str(c); print!("{c}");
                    }
                }
                buf.clear();
            }
            continue;
        }
        buf.push_str(&line);
    }
    println!("  chunk 数={chunks} 首 token={:?} 总耗时={:?} 正文字数={} 思考字数={reason_chars}", first2, t1.elapsed(), text2.chars().count());
    if std::env::var("SPIKE_RAW").is_ok() { println!("\n-- 原始前 12 行 --\n{}", raw.iter().take(12).cloned().collect::<String>()); }

    // 4) 取消：中途丢弃 reader（模拟用户停止）
    println!("\n== 取消测试 ==");
    let t2 = Instant::now();
    let mut resp = ureq::post(format!("{BASE}/api/chat")).send_json(serde_json::json!({
        "model": model, "stream": true, "options": {"num_predict": 400},
        "messages": [ {"role":"user","content":"请写一篇 800 字的随笔，题目是《本地优先》。"} ]
    }))?;
    {
        let mut r = BufReader::new(resp.body_mut().as_reader());
        let mut line = String::new();
        r.read_line(&mut line)?;           // 只读一帧就丢弃 = 取消
        println!("  读到 1 帧后丢弃 reader，耗时 {:?}", t2.elapsed());
    }
    drop(resp);
    println!("  连接关闭，耗时 {:?}", t2.elapsed());
    Ok(())
}
