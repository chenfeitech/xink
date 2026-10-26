use std::io::{BufRead, BufReader};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30))).build().into();
    for (label, url, body) in [
        ("agent /api/chat", "http://localhost:11434/api/chat", serde_json::json!({"model":"qwen3:0.6b","stream":true,"think":false,"options":{"num_predict":16},"messages":[{"role":"user","content":"hi"} ]})),
        ("agent /api/chat trailing", "http://localhost:11434/api/chat/", serde_json::json!({"model":"qwen3:0.6b","stream":true,"messages":[{"role":"user","content":"hi"}]})),
        ("agent /v1/chat/completions", "http://localhost:11434/v1/chat/completions", serde_json::json!({"model":"qwen3:0.6b","stream":true,"messages":[{"role":"user","content":"hi"}]})),
    ] {
        match agent.post(url).send_json(body.clone()) {
            Ok(mut r) => {
                let mut s = String::new();
                let mut rd = BufReader::new(r.body_mut().as_reader());
                rd.read_line(&mut s)?;
                println!("{label}: status={} first={}", r.status(), s.trim().chars().take(80).collect::<String>());
            }
            Err(e) => println!("{label}: ERR {e}"),
        }
    }
    // 顶层 API 对照
    match ureq::post("http://localhost:11434/api/chat").send_json(serde_json::json!({"model":"qwen3:0.6b","stream":true,"messages":[{"role":"user","content":"hi"}]})) {
        Ok(mut r) => { let mut s=String::new(); BufReader::new(r.body_mut().as_reader()).read_line(&mut s)?; println!("toplevel /api/chat: status={} first={}", r.status(), s.trim().chars().take(80).collect::<String>()); }
        Err(e) => println!("toplevel: ERR {e}"),
    }
    Ok(())
}
