//! 连接测试探针的判定
//!
//! `probe_model` 本身要发真实 HTTP，测不了；但它的**结论**完全由
//! `probe_outcome` 决定，这里是"能力声明不可信"的唯一决策点：
//! 判错了的表现是"设置里显示支持工具，跑起来却永远不调"。

use std::time::Duration;

use crate::core::ai::AiError;

use super::{noop_tool, probe_outcome, probe_timeout};

fn err() -> AiError {
    AiError::Protocol("boom".into())
}

#[test]
fn a_failed_probe_is_never_ok_and_never_supports_tools() {
    let result = probe_outcome(Some(err()), true, true, 12);
    assert!(!result.ok, "出错了就不能算通");
    assert!(!result.supports_tools, "出错时不能声明支持工具");
    assert!(result.error.is_some());
    assert_eq!(result.elapsed_ms, 12);
}

#[test]
fn an_empty_reply_counts_as_not_ok() {
    // 端点回了空 choices：既没正文也没工具调用，算不通
    let result = probe_outcome(None, false, false, 0);
    assert!(!result.ok);
    assert!(!result.supports_tools);
    assert!(result.error.is_none(), "没出错就不该带 error");
}

#[test]
fn a_text_only_reply_is_ok_without_tools() {
    let result = probe_outcome(None, true, false, 30);
    assert!(result.ok);
    assert!(!result.supports_tools, "没调工具就不能打开工具开关");
}

#[test]
fn a_tool_call_alone_is_ok_and_enables_tools() {
    // 只回了工具调用、没有正文，也算通——模型把"ping"理解成了调工具
    let result = probe_outcome(None, false, true, 30);
    assert!(result.ok);
    assert!(result.supports_tools);
}

#[test]
fn the_probe_timeout_has_a_five_second_floor() {
    // 冷启动要几秒：下限写成 min 的话，设 0 会必然超时（假阴性）
    assert_eq!(probe_timeout(0), Duration::from_secs(5));
    assert_eq!(probe_timeout(3), Duration::from_secs(5));
    assert_eq!(probe_timeout(5), Duration::from_secs(5));
    assert_eq!(probe_timeout(60), Duration::from_secs(60));
}

#[test]
fn the_noop_tool_has_the_same_shape_as_real_tools() {
    let tool = noop_tool();
    // 探针测的是"模型会不会按我们的 schema 调用"：形状不一致等于白测
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["function"]["name"], "noop");
    assert_eq!(tool["function"]["parameters"]["type"], "object");
}
