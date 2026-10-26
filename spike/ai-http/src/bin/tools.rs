//! Spike：验证 Ollama native /api/chat 的 tool calling（agent loop 的前提）
use std::time::Instant;

const BASE: &str = "http://localhost:11434";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let model = args.get(1).map(|s| s.as_str()).unwrap_or("qwen3.5:0.8b");
    let tools = serde_json::json!([{
        "type": "function",
        "function": {
            "name": "list_tags",
            "description": "列出当前笔记库里已存在的全部标签",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        }
    }]);
    for round in 0..=1 {
        let mut messages = vec![serde_json::json!({"role":"user","content":"库里现在有哪些标签？请用工具查，不要猜。"})];
        if round == 1 {
            messages.push(serde_json::json!({
                "role":"assistant",
                "tool_calls":[{"function":{"name":"list_tags","arguments":{}}}]
            }));
            messages.push(serde_json::json!({"role":"tool","content":"[\"人工智能\", \"读书\", \"GTD\"]"}));
        }
        let body = serde_json::json!({
            "model": model, "stream": false, "think": false,
            "options": {"num_ctx": 8192, "num_predict": 200},
            "tools": tools, "messages": messages
        });
        let t = Instant::now();
        let mut resp = ureq::post(format!("{BASE}/api/chat")).send_json(body)?;
        let v: serde_json::Value = if resp.status().is_success() { resp.body_mut().read_json()? } else {
            let e = resp.body_mut().read_to_string().unwrap_or_default();
            println!("  HTTP {} body={}", resp.status(), e.chars().take(300).collect::<String>());
            continue;
        };
        println!("--- round {round} ({:?}) ---", t.elapsed());
        let msg = &v["message"];
        let tc = msg["tool_calls"].as_array().map(|a| a.len()).unwrap_or(0);
        println!("  tool_calls={tc} content={:?}", msg["content"].as_str().unwrap_or("").chars().take(120).collect::<String>());
        if tc > 0 { println!("  call: {}", serde_json::to_string(&msg["tool_calls"][0])?); }
        println!("  eval_count={:?} prompt_eval_count={:?}", v["eval_count"], v["prompt_eval_count"]);
    }
    Ok(())
}
