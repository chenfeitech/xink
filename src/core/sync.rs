//! 编辑区 ↔ 预览区滚动同步的公共坐标（纯数据层）
//!
//! 两端内容结构不同（一边是 Markdown 源码行，一边是渲染块），直接拷贝
//! "滚动条百分比"必然错位。这里定义两端共用的中间坐标：
//!
//! - **Markdown**：源码**逻辑行号**（小数，行内用小数细分）；
//! - **数据文件等无法按行映射的内容**：退回滚动**比例**（0~1）。
//!
//! 设计见 `docs/preview/scroll-sync-design.md`。
//!
//! 本模块不碰 gpui：坐标换算需要的像素由 UI 层用 `frac × 块高` 自己算。

use std::time::{Duration, Instant};

/// 预览里一个**顶层块**对应的源行区间（0-based，含首尾）
///
/// 预览的列表项 = Markdown 顶层块（`gpui-base text/document.rs:207-217`），
/// 所以块的**下标**就是列表的 item 下标，这是两端对齐的桥梁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSpan {
    /// 块的第一行（0-based）
    pub start_line: usize,
    /// 块的最后一行（0-based，含）
    pub end_line: usize,
}

impl BlockSpan {
    /// 块占多少源行（至少 1 行）
    fn height(&self) -> f32 {
        (self.end_line.saturating_sub(self.start_line) + 1) as f32
    }
}

/// 顶层块 → 源行的映射表
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnchorTable {
    blocks: Vec<BlockSpan>,
    /// 文档总行数（用于把越界锚点夹回文档内）
    total_lines: usize,
}

impl AnchorTable {
    /// 解析一份**预览源文本**（必须是送进 `TextViewState` 的同一份文本）
    pub fn from_source(source: &str, total_lines: usize) -> Self {
        Self {
            blocks: block_spans(source),
            total_lines,
        }
    }

    /// 顶层块数量。运行时应与 `ListState::item_count()` 相等，不等说明
    /// 解析器行为有出入，调用方应降级（见设计文档 §4.2）。
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// 文档总行数
    pub fn total_lines(&self) -> usize {
        self.total_lines.max(1)
    }

    /// 源行（小数）→ (块下标, 块内比例)
    ///
    /// 块内按比例插值：块内行高并不均匀（代码块的一行可能对应渲染出的多行），
    /// 但用于"对齐到同一个块"已经足够，误差不跨块累积。
    pub fn locate(&self, line: f32) -> (usize, f32) {
        if self.blocks.is_empty() {
            return (0, 0.0);
        }
        let target = line.max(0.0) as usize;
        let ix = self
            .blocks
            .iter()
            .position(|block| target <= block.end_line)
            .unwrap_or(self.blocks.len() - 1);
        let block = self.blocks[ix];
        let frac = ((line - block.start_line as f32) / block.height()).clamp(0.0, 1.0);
        (ix, frac)
    }

    /// (块下标, 块内比例) → 源行（小数）
    pub fn line_of(&self, ix: usize, frac: f32) -> f32 {
        match self.blocks.get(ix) {
            Some(block) => block.start_line as f32 + frac.clamp(0.0, 1.0) * block.height(),
            // 块下标越界：退回最后一块的末尾，绝不返回垃圾值
            None => self.total_lines() as f32,
        }
    }
}

/// 解析顶层块的源行区间
///
/// 解析选项与预览保持一致（GFM + 行内/块级公式，未启用 frontmatter 与 MDX），
/// 见 `gpui-base-0.6.4/src/text/markdown_ext.rs:392-408`（两者默认均未开启）。
pub fn block_spans(source: &str) -> Vec<BlockSpan> {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;

    let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(source, &options) else {
        return Vec::new();
    };

    root.children
        .iter()
        .filter_map(|node| {
            let pos = node.position()?;
            // mdast 的行号是 1-based
            let start_line = pos.start.line.saturating_sub(1);
            let end_line = pos.end.line.saturating_sub(1).max(start_line);
            Some(BlockSpan {
                start_line,
                end_line,
            })
        })
        .collect()
}

/// 两端传递的滚动锚点
///
/// Markdown 用**源行**；无法按行映射的内容（数据文件预览是格式化后的文本）
/// 用**比例**。两端种类不一致时忽略这次同步，避免把行号当比例用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollAnchor {
    /// 源码逻辑行号（小数）
    Line(f32),
    /// 滚动比例 0~1
    Ratio(f32),
}

impl ScrollAnchor {
    /// 数值（行号或比例），仅供差值比较
    pub fn value(self) -> f32 {
        match self {
            ScrollAnchor::Line(v) => v,
            ScrollAnchor::Ratio(v) => v,
        }
    }

    /// 按种类夹到合法区间；`Line` 的上界由 `max_line` 给出
    pub fn clamped(self, max_line: f32) -> Self {
        match self {
            ScrollAnchor::Line(v) => ScrollAnchor::Line(v.clamp(0.0, max_line.max(0.0))),
            ScrollAnchor::Ratio(v) => ScrollAnchor::Ratio(v.clamp(0.0, 1.0)),
        }
    }

    /// 种类相同才算同类，行号与比例不可混用
    pub fn same_kind(self, other: Self) -> bool {
        matches!(
            (self, other),
            (ScrollAnchor::Line(_), ScrollAnchor::Line(_))
                | (ScrollAnchor::Ratio(_), ScrollAnchor::Ratio(_))
        )
    }
}

/// 方向锁的有效期：这段时间内只有"发起端"能推动同步
///
/// 太短挡不住回显（会 ping-pong），太长会吃掉用户紧接着的反向滚动；
/// 200ms 覆盖"写滚动位置 → 下一帧回读"的完整往返，又不至于粘手。
const OWNER_TIMEOUT: Duration = Duration::from_millis(200);

/// 回显判定的有效期
///
/// 写入滚动位置是 deferred 的：落点会被 clamp、被软换行重排、被虚拟列表
/// 重新测量修正，所以"写入目标"与"下一帧读到的位置"经常差几像素甚至几行。
/// 这段时间内读到的、与写入目标接近的位置都算自己的回显。
/// 超过这个时长还没被消费就作废——否则用户之后自己滚到附近也会被误吸收。
const ECHO_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Editor,
    Preview,
}

/// 同步方向锁：取代旧的"单值 EPS 回显吸收"
///
/// 旧做法把"我刚写入的目标"与"当前读到的值"用固定阈值比较，
/// 取整误差超过阈值就被当成用户滚动反向推回（抖动），
/// 用户滚动又可能落在阈值内被误吸收（丢帧）。这里改成：
///
/// 1. 谁先动谁在 `OWNER_TIMEOUT` 内持有方向锁，期间对端一律按回显吸收；
/// 2. 回显判定用调用方给的**像素换算阈值**，不再用固定比例阈值。
#[derive(Debug)]
pub struct SyncGuard {
    owner: Option<Side>,
    since: Instant,
    /// 我刚写入的目标值，用于判定回显
    expected: Option<ScrollAnchor>,
    /// 写入目标的时刻：超过 [`ECHO_TIMEOUT`] 后不再算回显
    expected_at: Option<Instant>,
}

impl Default for SyncGuard {
    fn default() -> Self {
        Self {
            owner: None,
            since: Instant::now(),
            expected: None,
            expected_at: None,
        }
    }
}

impl SyncGuard {
    /// 切换文档 / 关闭面板时调用：不重置会带着上一篇的位置跳一下
    pub fn reset(&mut self) {
        self.owner = None;
        self.since = Instant::now();
        self.expected = None;
        self.expected_at = None;
    }

    /// 本端检测到真实用户滚动，能否发起同步
    pub fn claim(&mut self, side: Side, now: Instant) -> bool {
        let expired = now.duration_since(self.since) >= OWNER_TIMEOUT;
        match self.owner {
            None => self.take(side, now),
            Some(owner) if owner == side => {
                self.since = now;
                true
            }
            Some(_) if expired => self.take(side, now),
            // 对端正在推动同步：本端这次变化视为回显
            Some(_) => false,
        }
    }

    fn take(&mut self, side: Side, now: Instant) -> bool {
        self.owner = Some(side);
        self.since = now;
        true
    }

    /// 记录"我刚写入的目标"，供回显判定
    pub fn expect(&mut self, anchor: ScrollAnchor) {
        self.expected = Some(anchor);
        self.expected_at = Some(Instant::now());
    }

    /// 当前读到的值是不是"我刚写入的回显"
    ///
    /// `tol` 由调用方按像素换算给出（例如 2px ÷ 行高），
    /// 避免在短文档里 1px 的差异就被误判成用户滚动。
    pub fn is_echo(&self, anchor: ScrollAnchor, tol: f32) -> bool {
        let now = Instant::now();
        match (self.expected, self.expected_at) {
            (Some(expected), Some(at)) if now.duration_since(at) <= ECHO_TIMEOUT => {
                expected.same_kind(anchor)
                    && (expected.value() - anchor.value()).abs() <= tol.max(0.0)
            }
            // 过期 / 没写过：一概不算回显，避免把用户之后的滚动误吸收
            _ => false,
        }
    }
}

/// 用户手势静默窗口
///
/// 滚轮 / 触控板惯性期间，任何"同步写入滚动位置"的操作都会盖掉同一帧的手势结果，
/// 表现为"滚不动"或"被拉回"。识别到用户手势后打开一个静默窗口，期间同步一律不写。
///
/// 判定"这一帧的位置变化是不是用户造成的"由调用方负责（比较上一帧位置与自己
/// 写入的目标），这里只管窗口状态机。
#[derive(Debug, Default)]
pub struct GestureGuard {
    quiet_until: Option<Instant>,
}

impl GestureGuard {
    /// 静默窗口时长：覆盖触控板一次甩动的惯性尾巴
    pub const QUIET_PERIOD: Duration = Duration::from_millis(250);

    /// 观察到一次用户滚动 → 打开（并续期）静默窗口
    pub fn note_user_movement(&mut self, now: Instant) {
        self.quiet_until = Some(now + Self::QUIET_PERIOD);
    }

    /// 是否处于静默窗口内（是则不要写滚动位置）
    pub fn is_quiet(&self, now: Instant) -> bool {
        self.quiet_until.is_some_and(|until| now < until)
    }

    /// 切换文档 / 重置同步时清空
    pub fn reset(&mut self) {
        self.quiet_until = None;
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/sync.rs"]
mod tests;
