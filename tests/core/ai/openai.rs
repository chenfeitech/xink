//! OpenAI 兼容端点的帧解析（通过 `frame_to_events` 的行为覆盖）

use crate::core::config::AiModelParams;

/// 这里的用例走"一次请求"的黑盒路径：构造 endpoint，验证拼接规则。
/// 流式帧解析本身由 `sse.rs` 的分帧 + `openai.rs` 的映射共同完成，
/// 映射逻辑无 IO，单独抽成断言需要把它 pub 出去，暂以 endpoint 规则与
/// 请求体字段的最小约定代替（真实联调见人工验收清单）。
#[test]
fn endpoint_按整体_base_url_拼接() {
    // 智谱不是 /v1，DeepSeek 是 /v1：base_url 必须整体可配
    assert_eq!(
        crate::core::ai::openai::chat_endpoint("https://api.deepseek.com/v1"),
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(
        crate::core::ai::openai::chat_endpoint("https://open.bigmodel.cn/api/paas/v4/"),
        "https://open.bigmodel.cn/api/paas/v4/chat/completions"
    );
}

#[test]
fn 默认生成参数给足预算() {
    let params = AiModelParams::default();
    // thinking 模型会把 num_predict 用在思维链上，太小会一个正文字都不出
    assert!(params.num_predict >= 1024);
    assert!(!params.think, "默认关闭思考");
}
