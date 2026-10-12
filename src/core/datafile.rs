//! 数据文件（JSON / YAML / TOML）解析、格式化与校验
//!
//! 目录树目前只放行 Markdown 与图片，配置文件在应用里既看不到也打不开。
//! 本模块提供两件事：
//!
//! 1. **种类判定**（[`data_kind`]）：扩展名 → [`DataKind`]，是"哪些文件算数据文件"
//!    的唯一判据来源，供目录树、文件监听、打开入口共用；
//! 2. **解析与格式化**（[`parse`]）：给预览提供 pretty-print 后的文本，
//!    并在内容非法时给出**行列 + 原因**。
//!
//! # 重要取舍：格式化只进预览，绝不写回文件
//!
//! YAML 经 `serde` 往返会**丢注释**（`noyalib` 亦如此，见测试
//! `yaml_formatting_drops_comments`），引号风格、anchor/alias 也会被规范化。
//! 因此格式化结果只用于预览展示，保存链路永远写用户原文。

use std::path::Path;

use noyalib::Value as YamlValue;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_json::ser::{PrettyFormatter, Serializer as JsonSerializer};
use toml::Value as TomlValue;

/// 预览格式化缩进默认值（空格数）
pub const DEFAULT_INDENT: usize = 2;

/// 格式化预览的字节上限默认值：超过则跳过格式化，避免大文件卡顿
pub const DEFAULT_MAX_FORMAT_BYTES: usize = 1_000_000;

/// 错误提示的最大长度，超出截断
const MAX_MESSAGE_LEN: usize = 160;

/// 支持的数据文件种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Json,
    Yaml,
    Toml,
}

impl DataKind {
    /// 预览围栏的语言标签，同时用作编辑区 `set_highlighter` 的语言名
    pub fn lang(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
        }
    }

    /// 工具栏徽标显示的大写名
    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
        }
    }

    /// 预览顶部的常驻说明；无说明返回 `None`
    ///
    /// YAML 的格式化是有损的（注释不保留），必须显式告知，
    /// 否则用户会以为注释被应用删了。
    pub fn notice(self) -> Option<&'static str> {
        match self {
            Self::Yaml => Some("预览为规范化结果（注释不保留）"),
            _ => None,
        }
    }
}

/// 校验错误：行列均为 **0-based**，可直接喂给 `lsp_types::Position`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl ValidationError {
    /// 给人看的"第 L 行第 C 列"（1-based）
    pub fn display_position(&self) -> String {
        format!("第 {} 行第 {} 列", self.line + 1, self.column + 1)
    }
}

/// 解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// 合法：`formatted` 是格式化后的文本（只用于预览，不写盘）
    Ok { formatted: String },
    /// 非法：携带错误位置与原因
    Err { error: ValidationError },
}

// 三个取值方法目前只被测试使用（生产路径直接 `match Parsed`），
// 保留它们是为了让断言可读，故显式豁免死代码告警。
#[allow(dead_code)]
impl Parsed {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    /// 格式化后的文本；非法时为 `None`（调用方用原文兜底）
    pub fn formatted(&self) -> Option<&str> {
        match self {
            Self::Ok { formatted } => Some(formatted),
            Self::Err { .. } => None,
        }
    }

    pub fn error(&self) -> Option<&ValidationError> {
        match self {
            Self::Err { error } => Some(error),
            Self::Ok { .. } => None,
        }
    }
}

/// 按扩展名判定数据文件种类。
///
/// 大小写不敏感；`a.json.bak`、无扩展名、`.md` 一律不命中——
/// 判据只对**最后一个扩展名**生效，避免把备份文件当成配置打开。
pub fn data_kind(path: &Path) -> Option<DataKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "json" => Some(DataKind::Json),
        "yaml" | "yml" => Some(DataKind::Yaml),
        "toml" => Some(DataKind::Toml),
        _ => None,
    }
}

/// 是否数据文件（JSON / YAML / TOML）
pub fn is_data_file(path: &Path) -> bool {
    data_kind(path).is_some()
}

/// 解析并格式化一段文本。
///
/// - `indent`：缩进宽度（空格数），**当前仅对 JSON 生效**——
///   `noyalib` 与 `toml` 的序列化器不提供缩进配置；
/// - 空内容（全空白）一律视为合法，返回空格式化结果，避免"新建的空文件一打开就报错"。
pub fn parse(text: &str, kind: DataKind, indent: usize) -> Parsed {
    if text.trim().is_empty() {
        return Parsed::Ok {
            formatted: String::new(),
        };
    }
    match kind {
        DataKind::Json => parse_json(text, indent),
        DataKind::Yaml => parse_yaml(text),
        DataKind::Toml => parse_toml(text),
    }
}

fn parse_json(text: &str, indent: usize) -> Parsed {
    match serde_json::from_str::<JsonValue>(text) {
        Ok(value) => match pretty_json(&value, indent) {
            Some(formatted) => Parsed::Ok { formatted },
            None => Parsed::Err {
                error: ValidationError {
                    line: 0,
                    column: 0,
                    message: "序列化失败".to_string(),
                },
            },
        },
        Err(err) => Parsed::Err {
            error: ValidationError {
                // serde_json 的 line / column 是 1-based
                line: err.line().saturating_sub(1),
                column: err.column().saturating_sub(1),
                message: shorten(&err.to_string()),
            },
        },
    }
}

/// 按指定缩进序列化 JSON。缩进限制在 1–8，避免配置填了离谱值。
fn pretty_json(value: &JsonValue, indent: usize) -> Option<String> {
    let pad = vec![b' '; indent.clamp(1, 8)];
    let mut buf = Vec::new();
    let mut ser = JsonSerializer::with_formatter(&mut buf, PrettyFormatter::with_indent(&pad));
    value.serialize(&mut ser).ok()?;
    String::from_utf8(buf).ok()
}

fn parse_yaml(text: &str) -> Parsed {
    // 单文档限制：`from_str::<Value>` 遇到多文档会报难以定位的错误，
    // 这里先自己识别分隔符，给出明确的中文提示
    if let Some(line) = multi_doc_line(text) {
        return Parsed::Err {
            error: ValidationError {
                line,
                column: 0,
                message: "暂不支持多文档 YAML（--- 分隔）".to_string(),
            },
        };
    }

    match noyalib::from_str::<YamlValue>(text) {
        Ok(value) => match noyalib::to_string(&value) {
            Ok(formatted) => Parsed::Ok { formatted },
            Err(err) => Parsed::Err {
                error: yaml_error(&err),
            },
        },
        Err(err) => Parsed::Err {
            error: yaml_error(&err),
        },
    }
}

fn yaml_error(err: &noyalib::Error) -> ValidationError {
    let (line, column) = match err.location() {
        // noyalib 的 line / column 是 1-based
        Some(loc) => (loc.line().saturating_sub(1), loc.column().saturating_sub(1)),
        None => (0, 0),
    };
    ValidationError {
        line,
        column,
        message: shorten(strip_location_prefix(&err.to_string())),
    }
}

/// 去掉 `YAML parse error at line 2, column 4: ` 这类前缀。
///
/// 行列我们自己展示，前缀留在消息里会造成"行号显示两遍"。
fn strip_location_prefix(msg: &str) -> &str {
    const PREFIX: &str = "YAML parse error at line ";
    let Some(rest) = msg.strip_prefix(PREFIX) else {
        return msg;
    };
    // 跳过 "N, column M: "
    match rest.split_once(": ") {
        Some((_, tail)) => tail,
        None => msg,
    }
}

/// 多文档分隔符所在行（0-based）；没有则返回 `None`
fn multi_doc_line(text: &str) -> Option<usize> {
    text.lines()
        .enumerate()
        .find(|(i, line)| *i > 0 && line.trim_end() == "---")
        .map(|(i, _)| i)
}

fn parse_toml(text: &str) -> Parsed {
    match toml::from_str::<TomlValue>(text) {
        Ok(value) => match toml::to_string_pretty(&value) {
            Ok(formatted) => Parsed::Ok { formatted },
            Err(err) => Parsed::Err {
                error: ValidationError {
                    line: 0,
                    column: 0,
                    message: shorten(&err.to_string()),
                },
            },
        },
        Err(err) => {
            // toml 的错误只给字节区间，行列为 0-based，需自己换算
            let (line, column) = match err.span() {
                Some(span) => offset_to_position(text, span.start),
                None => (0, 0),
            };
            Parsed::Err {
                error: ValidationError {
                    line,
                    column,
                    message: shorten(err.message()),
                },
            }
        }
    }
}

/// 字节偏移 → (行, 列)，均 0-based。偏移越界时按末尾处理。
pub fn offset_to_position(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.as_bytes().iter().filter(|b| **b == b'\n').count();
    let column = match before.rfind('\n') {
        Some(idx) => before.len() - idx - 1,
        None => before.len(),
    };
    (line, column)
}

/// 截断过长消息，只保留第一行
fn shorten(msg: &str) -> String {
    let first = msg.lines().next().unwrap_or("");
    if first.chars().count() <= MAX_MESSAGE_LEN {
        return first.to_string();
    }
    let cut: String = first.chars().take(MAX_MESSAGE_LEN).collect();
    format!("{cut}…")
}

// 测试代码统一放在仓库根的 `tests/`，目录结构镜像 `src/`。
// `#[path]` 的基准是本文件所在目录（src/core/），故回退两级到仓库根。
// 模块树位置不变，测试里的 `use super::*` 依旧指向本模块。
#[cfg(test)]
#[path = "../../tests/core/datafile.rs"]
mod tests;
