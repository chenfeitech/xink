//! 能力字典与推断的单测
//!
//! 由 `src/core/ai/capability.rs` 用 `#[path]` 挂载（`Cargo.toml` 里
//! `autotests = false`，tests/ 不是集成测试目录）。

use super::{KNOWN, effective, infer, info};
use crate::core::config::{AiProviderKind, ModelEntry};

// ── 字典 ──────────────────────────────────────────────

#[test]
fn 已知能力都有中文标签和说明() {
    for id in KNOWN {
        let info = info(id);
        assert_eq!(info.label, info.label.trim(), "{id} 标签有空白");
        assert!(!info.label.is_empty(), "{id} 没有中文标签");
        assert!(!info.meaning.is_empty(), "{id} 没有含义说明");
        assert!(!info.usage.is_empty(), "{id} 没有用途说明");
    }
    // 这几项是 Ollama 实测会报回来的原词，标签不能是"未知"
    assert_eq!(info("completion").label, "对话补全");
    assert_eq!(info("tools").label, "工具调用");
    assert_eq!(info("thinking").label, "思考 / 推理");
    assert_eq!(info("vision").label, "视觉理解");
}

/// 厂商造新词不能让界面 panic，也不能把值丢了
#[test]
fn 未知能力回落到未知而不是崩溃() {
    for id in ["embedding", "", "  ", "TOOLS"] {
        let info = info(id);
        assert_eq!(info.label, "未知能力", "{id:?} 应回落未知");
        assert!(!info.usage.is_empty());
    }
}

// ── 推断 ──────────────────────────────────────────────

/// Ollama 有 `/api/tags` 的真实来源，不该被猜测覆盖
#[test]
fn ollama_侧不推断() {
    assert!(
        infer(
            AiProviderKind::Ollama,
            "http://localhost:11434",
            "qwen3.5:0.8b"
        )
        .is_empty(),
        "Ollama 的能力来自 /api/tags，猜了反而会盖掉真实值"
    );
}

#[test]
fn deepseek_推理模型给思考能力() {
    let caps = infer(
        AiProviderKind::OpenAiCompatible,
        "https://api.deepseek.com",
        "deepseek-reasoner",
    );
    assert!(
        caps.contains(&"thinking".into()),
        "reasoner 应给 thinking：{caps:?}"
    );
    assert!(
        caps.contains(&"completion".into()),
        "基础能力不能缺：{caps:?}"
    );
}

#[test]
fn deepseek_对话模型给工具能力() {
    let caps = infer(
        AiProviderKind::OpenAiCompatible,
        "https://api.deepseek.com",
        "deepseek-chat",
    );
    assert!(caps.contains(&"tools".into()), "chat 应给 tools：{caps:?}");
    assert!(
        !caps.contains(&"thinking".into()),
        "chat 不该被当成推理模型：{caps:?}"
    );
}

/// 名字里带特征词的，不必认识域名也能判出来
#[test]
fn 模型名特征词优先于域名() {
    let caps = infer(
        AiProviderKind::OpenAiCompatible,
        "https://my-gateway.example.com/v1",
        "some-reasoner-v2",
    );
    assert!(caps.contains(&"thinking".into()), "{caps:?}");

    let caps = infer(
        AiProviderKind::OpenAiCompatible,
        "https://my-gateway.example.com/v1",
        "my-vl-model",
    );
    assert!(caps.contains(&"vision".into()), "{caps:?}");
}

#[test]
fn 主流厂商域名给默认能力() {
    for base in [
        "https://api.deepseek.com",
        "https://open.bigmodel.cn/api/paas/v4",
        "https://api.moonshot.cn/v1",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "https://api.openai.com/v1",
    ] {
        let caps = infer(AiProviderKind::OpenAiCompatible, base, "whatever-model");
        assert!(
            caps.contains(&"tools".into()),
            "{base} 应给 tools：{caps:?}"
        );
    }
}

/// 认不出来就返回空，让界面显示"未知"，不硬猜
#[test]
fn 认不出来返回空而不是乱猜() {
    assert!(
        infer(
            AiProviderKind::OpenAiCompatible,
            "https://unknown-vendor.example.com/v1",
            "mystery-model"
        )
        .is_empty(),
        "未知端点应返回空（界面显示「未知」）"
    );
    // base_url 为空也不能崩
    assert!(infer(AiProviderKind::OpenAiCompatible, "", "").is_empty());
}

// ── 展示用清单 ────────────────────────────────────────

/// Ollama 拉回来的能力要原样用，不掺推断
#[test]
fn effective_用已存的能力() {
    let model = ModelEntry {
        id: "qwen3.5:0.8b".into(),
        kind: AiProviderKind::Ollama,
        capabilities: vec!["completion".into(), "vision".into()],
        ..Default::default()
    };
    assert_eq!(effective(&model), vec!["completion", "vision"]);
}

/// OpenAI 兼容侧没来源时用推断值，不留白
#[test]
fn effective_为空时回落到推断值() {
    let model = ModelEntry {
        id: "deepseek-chat".into(),
        kind: AiProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com".into(),
        ..Default::default()
    };
    assert_eq!(effective(&model), vec!["completion", "tools"]);
}

/// 用户手动改过就以用户为准——哪怕是空（全删了不该自动填回来）
#[test]
fn effective_手动改过的优先且空也算数() {
    let model = ModelEntry {
        id: "deepseek-chat".into(),
        kind: AiProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com".into(),
        capabilities: vec!["thinking".into()],
        capabilities_pinned: true,
        ..Default::default()
    };
    assert_eq!(effective(&model), vec!["thinking"]);

    let cleared = ModelEntry {
        capabilities: Vec::new(),
        capabilities_pinned: true,
        ..model
    };
    assert!(
        effective(&cleared).is_empty(),
        "用户全删了就不该被推断值填回来"
    );
}

#[test]
fn 域名解析能应付各种写法() {
    // 带端口、带路径、没有 scheme
    for base in [
        "http://localhost:11434/v1",
        "localhost:11434",
        "https://api.deepseek.com/v1/chat",
    ] {
        // 只要不 panic 即可；具体结果由上面各条用例覆盖
        let _ = infer(AiProviderKind::OpenAiCompatible, base, "deepseek-chat");
    }
}
