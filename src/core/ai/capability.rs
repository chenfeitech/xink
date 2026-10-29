//! 模型能力的字典与推断
//!
//! 需求背景（详见 `docs/ai/ai-capability-list-requirements.md`）：
//! 两端的能力来源完全不对称——
//! - **Ollama**：`/api/tags` 顶层就带 `capabilities` 数组（`models.rs:56-60`）；
//! - **OpenAI 兼容端点**（DeepSeek 等）：**完全没有能力来源**。模型只能手填，
//!   而 OpenAI 的 `/v1/models` 只有 `id/created/owned_by`、各家路径还不一样，
//!   拉了也拿不到能力（`ai-m1-completion-design.md:238` 当初就排除了）。
//!
//! 所以这里的定位是：
//! - [`info()`] 给"某项能力是什么意思"——Ollama 报回来的原始字符串
//!   （`completion` / `tools` / `thinking` / `vision`）连中文标签都没有；
//! - [`infer()`] 给 OpenAI 兼容端点一个**默认猜测**，用户可以手动改。
//!
//! **推断不是承诺**：`probe.rs:1-4` 记着实测——`qwen3.5:0.8b` 在 `/api/tags` 里
//! 声明支持 tools，真发工具过去返回 500。所以运行时权威判据一直是探针实测的
//! `tools_enabled`（`ui/ai.rs:861`），本模块只负责"让用户看见"。

use crate::core::config::AiProviderKind;

// ── 字典 ──────────────────────────────────────────────
//
// 以前这些约定值只活在 `config.rs:632` 的一行注释里，没有枚举、没有说明。
// 这里集中成一处（需求 C6）：加一个能力只改这里，UI 自动跟上。

/// 能力的说明：标签给界面看，含义解释"是什么"，用途解释"开了会怎样"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityInfo {
    /// 中文标签
    pub label: &'static str,
    /// 这项能力是什么
    pub meaning: &'static str,
    /// 用来干什么 / 有什么代价
    pub usage: &'static str,
}

/// 查一项能力的说明
///
/// 未知取值**不 panic、不丢弃**：厂商会造新词（比如 `embedding`），
/// 拿到什么就显示什么，只是标注为"未知能力"（需求 R-5 / 约束 C1）。
pub fn info(id: &str) -> CapabilityInfo {
    match id {
        "completion" => CapabilityInfo {
            label: "对话补全",
            meaning: "模型最基本的能力：给一段文字，续写或回答",
            usage: "所有对话、快捷动作、改写都依赖它",
        },
        "tools" => CapabilityInfo {
            label: "工具调用",
            meaning: "能按约定格式请求调用外部函数（读库、搜索笔记）",
            usage: "开启后 AI 可以先查笔记再回答；以「连接测试」实测结果为准",
        },
        "thinking" => CapabilityInfo {
            label: "思考 / 推理",
            meaning: "先输出一段内部推理，再给最终答案",
            usage: "复杂问题更准，但更慢、且占用生成预算；仅 Ollama 侧可关闭",
        },
        "vision" => CapabilityInfo {
            label: "视觉理解",
            meaning: "能接收图片输入",
            usage: "本项目当前未使用",
        },
        _ => CapabilityInfo {
            label: "未知能力",
            meaning: "这个能力项不在已知列表里（厂商自定义）",
            usage: "可手动删除；保留也不影响运行",
        },
    }
}

/// 字典里已知的能力 id
///
/// 「编辑」菜单按这个顺序列出可选项，用户能勾的也就是这些。
pub const KNOWN: [&str; 4] = ["completion", "tools", "thinking", "vision"];

// ── 推断 ──────────────────────────────────────────────

/// 推断一个模型的能力清单
///
/// **纯函数**：不碰网络、不碰全局状态，方便单测（设计 §3.3）。
///
/// Ollama 侧返回空——它有 `/api/tags` 的真实来源，不该被猜测覆盖；
/// 调用方只在 `OpenAiCompatible` 时用它。
///
/// 匹配顺序是先具体后宽泛：模型名特征词 → 域名 → 兜底（空）。
/// 匹配不上就返回空，由 UI 显示"未知"（需求 R-5），不做强行猜测。
pub fn infer(kind: AiProviderKind, base_url: &str, model_id: &str) -> Vec<String> {
    // Ollama 有自己的能力来源（/api/tags），不猜
    if kind == AiProviderKind::Ollama {
        return Vec::new();
    }
    let model = model_id.to_ascii_lowercase();
    let host = host_of(base_url);
    let mut caps: Vec<String> = Vec::new();

    // 1) 名字特征词最可靠：厂商自己就在名字里标了
    if contains_any(&model, &["reasoner", "thinking", "-r1"]) {
        caps.push("completion".into());
        caps.push("thinking".into());
    }
    if contains_any(&model, &["vision", "-vl"]) && !caps.contains(&"vision".to_string()) {
        caps.push("vision".into());
    }
    if !caps.is_empty() {
        // 名字已经说清楚了，不必再看域名
        return caps;
    }

    // 2) 域名 + 模型名前缀：主流厂商的猜测
    let known_host = host.as_deref().is_some_and(is_known_host);
    let known_name = contains_any(&model, &["deepseek", "glm", "moonshot", "qwen", "gpt"]);
    if known_host || known_name {
        caps.push("completion".into());
        caps.push("tools".into());
    }
    caps
}

/// 从 base_url 取主机名小写（拿不到就是 `None`）
fn host_of(base_url: &str) -> Option<String> {
    let rest = base_url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(base_url);
    let host = rest.split(['/', ':', '?']).next().unwrap_or("").trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// 内置表里认得的域名
///
/// 只放主流厂商：表是要维护的，宁可让冷门端点走"未知 + 手动指定"，
/// 也不要硬猜一个错的（设计 §9-R2）。
fn is_known_host(host: &str) -> bool {
    [
        "api.deepseek.com",
        "open.bigmodel.cn",
        "api.moonshot.cn",
        "dashscope.aliyuncs.com",
        "api.openai.com",
    ]
    .iter()
    .any(|known| host == *known || host.ends_with(known))
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
#[path = "../../../tests/core/ai/capability.rs"]
mod tests;
