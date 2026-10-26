use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1) Agent + 全局超时
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let t = Instant::now();
    let mut resp = agent.get("http://localhost:11434/api/tags").call()?;
    let v: serde_json::Value = resp.body_mut().read_json()?;
    println!("tags ok in {:?}, models={}", t.elapsed(), v["models"].as_array().map(|a| a.len()).unwrap_or(0));

    // 2) Agent + 流式 native
    let body = serde_json::json!({
        "model": "qwen3:0.6b", "stream": true, "think": false,
        "options": {"num_ctx": 4096, "num_predict": 40},
        "messages": [{"role":"user","content":"说两个字：你好"}]
    });
    let t = Instant::now();
    let mut resp = agent.post("http://localhost:11434/api/chat").send_json(body)?;
    println!("status={} in {:?}", resp.status(), t.elapsed());
    let mut r = BufReader::new(resp.body_mut().as_reader());
    let mut line = String::new();
    let mut n = 0;
    while r.read_line(&mut line)? > 0 { n += 1; line.clear(); if n > 3 { break; } }
    println!("frames read={n} elapsed={:?}", t.elapsed());

    // 3) 超时路径：连一个不存在的端口
    let bad: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(300)))
        .build()
        .into();
    let t = Instant::now();
    let e = bad.get("http://localhost:9/api/tags").call();
    println!("bad -> err={:?} in {:?}", e.err().map(|e| e.to_string()), t.elapsed());

    // 4) 非 2xx 是否返回 Err（用于错误分类）
    let e2 = agent.post("http://localhost:11434/api/chat").send_json(serde_json::json!({"model":"no-such-model:1b","stream":false,"messages":[]}));
    println!("missing model -> status/err = {:?}", e2.err().map(|e| e.to_string()).or_else(|| Some("ok".into())));
    Ok(())
}
