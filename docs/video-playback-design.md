# 设计：视频文件预览与播放（**已放弃**，本文转为经验存档）

>
>
> **2026-10-12 结论：放弃本方案，代码与依赖已全部清理。**
>
> 放弃原因：Spike（独立程序）里解码/渲染/音频/同步全部验证通过，但接进真实应用后
> **画面始终出不来**，两轮修复未解决，继续投入的边际收益太低，故停止。
>
> 本文保留下来只作经验存档：下面的实测结论与"坑清单"仍然有效，将来若要重做，
> 从这里开始能省掉一遍调研。

## 0. Spike 实测结论（本文档的全部依据）

| # | 验证项 | 结果 | 证据/数据 |
|---|--------|------|-----------|
| 1 | 逐帧绘制是否可行 | ✅ | `RenderImage::new` + `ImageSource::Render` + `img()` 可逐帧更新画面 |
| 2 | **纹理内存** | ⚠️ **必须显式释放** | 不释放：263 帧 RSS 75MB→**2.2GB**（263×7.9MB 吻合）；每帧 `Window::drop_image(上一帧)` 后：785 帧 75MB→102MB |
| 3 | ffmpeg 版本兼容 | ✅ | `ffmpeg-next 9` 在系统 **FFmpeg 8.1** 上编译、链接、解码均正常（此前担心"FFmpeg 4 兼容"不成立） |
| 4 | 真实解码 | ✅ | H.264 1280×720 → RGBA，stride=5120=1280×4 **连续**，可整块拷贝，无需逐行处理 |
| 5 | 音频出声 | ✅ | AAC 44.1kHz → resample 48kHz/f32/stereo → `rodio` 播放成功（1166 段 PCM） |
| 6 | 音画同步 | ✅ | 以音频墙钟为主时钟：**平均偏差 0.0ms，最大 6.7ms**，丢帧 0 |
| 7 | 性能 | ⚠️ 存疑 | 三次测试都是 **26fps，且与负载无关**（1080p 合成帧 / 540p 合成帧 / 真实解码全一样）→ 瓶颈不在纹理上传，疑为窗口非前台被 macOS 降频，**需前台确认** |

## 1. 现状与问题

| # | 现象 | 代码位置 | 影响 |
|---|------|---------|------|
| 1 | 目录树只显示可编辑文本与图片，视频文件不可见、不可打开 | `src/core/vault.rs:135`（`is_editable_text`）、`src/core/media.rs:11`（`is_image_path`） | 视频文件在库里"不存在" |
| 2 | `open_note` 只放行可编辑文本 | `src/ui/tabs.rs:182` | 即便放行进树也打不开 |
| 3 | 预览面板只支持 Markdown 与图片两种模式 | `src/ui/preview.rs:208-252`（`image: Option<PathBuf>` 分支） | 没有视频的落点 |
| 4 | 框架**没有**任何 video/player 元素 | `gpui-base-0.6.4/src`、`gpui-component-0.6.4/src` 全文无 video/player | 只能自研 |
| 5 | `gpui-pre-media` 名不副实 | 其内容是 CoreMedia/CoreVideo 的**采集侧**绑定（`CMSampleBuffer`、`CVMetalTextureCache`、H.264 parameter set） | 不是播放器，用不上 |
| 6 | 无法嵌入原生播放器 | NSView 相关实现全在 `gpui-pre-macos-0.3.5/src/window.rs` 内部，无公开 API 暴露 NSView / `addSubview` | `AVPlayerLayer` 路线走不通 |
| 7 | GPUI 无音频输出 API | 框架源码无 audio 相关 | 声音必须外接 `rodio` |

## 2. 目标 / 非目标

**目标**
1. 目录树能显示常见视频（mp4 / mov / webm / mkv），双击在**预览面板**里播放（复用图片预览的位置，不新开窗口）。
2. 提供播放/暂停、进度拖动（seek）、音量控制。
3. 音画同步偏差 < 50ms（Spike 实测已达 6.7ms）。
4. 播放不泄漏内存（Spike 已验证对策有效）。

**非目标**
- 不做移动端（GPUI 是桌面框架）。
- 不做字幕、倍速、全屏、截图、播放列表。
- 不追求 4K/60fps 软解性能（超出屏幕显示尺寸的帧一律先缩放）。
- 不改 Markdown / 数据文件预览的任何现有行为。

## 3. 方案概述

### 3.1 整体形态

```
目录树（新增 FileKind::Video）
   └─ 双击 → 在预览面板里播放（复用图片预览的同一块区域）
        ├─ 后台线程：解封装 + 视频解码 + swscale → RGBA      （CPU 密集，绝不能在主线程）
        ├─ 后台线程：音频解码 + swresample → f32 → rodio      （音频墙钟 = 主时钟）
        └─ UI 线程：每帧取帧 → RenderImage → paint → drop 上一帧
```

### 3.2 被否决的方案

| 方案 | 否决理由 |
|------|---------|
| 嵌入原生 `AVPlayerLayer` | 无公开 API 拿到窗口 NSView（§1-6），且锁死 macOS |
| 用 `gpui-pre-media` | 是采集侧绑定，不是播放器（§1-5） |
| 纯 Rust 组合（`mp4` + `openh264` + `symphonia`） | 无系统依赖看似好，但只覆盖 H.264/AAC 的 mp4，软解性能差，且要自己处理 AVCC→AnnexB；ffmpeg 已实测可用，没必要退而求其次 |
| 调系统播放器 `open` | 不满足"应用内播放 + 播放控制" |

### 3.3 三条不可违反的实现约束（Spike 换来）

1. **每帧必须 `Window::drop_image(上一帧)`**（`gpui-pre/src/window.rs:4894`）。
   不这么做纹理缓存只增不减，实测 263 帧吃掉 2.2GB。
2. **`image` 依赖必须与 gpui-pre 内部同系列**（锁 `0.25`），否则 `image::Frame` 是另一个
   crate 版本的类型，喂不进 `RenderImage::new`（与 `ropey` 同理）。
3. **`rodio 0.23` 的 API 与老版本完全不同**：没有 `OutputStream`/`Sink`，
   要用 `DeviceSinkBuilder::open_default_sink()` + `Player::connect_new(sink.mixer())`，
   `SamplesBuffer::new` 的声道数/采样率参数是 `NonZero<u16>` / `NonZero<u32>`。
   另外 `Player::get_pos()` 只返回"当前单个 source 内"的位置，**不能当全局时钟**。

## 4. 数据模型与持久化

新增 `src/core/player.rs`（领域层，纯逻辑，无 UI 依赖，可单测）：

```rust
pub enum PlaybackState { Playing, Paused, Ended }

pub struct VideoFrame {
    pub pts: f64,          // 呈现时间戳（秒）
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,    // RGBA
}

pub struct MediaInfo {
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

/// 音画同步：以音频墙钟为主时钟决定"该显示哪一帧"
pub struct SyncPolicy {
    pub drop_threshold: f64,  // 落后超过它就丢帧追赶（Spike 用 0.25s）
    pub ahead_tolerance: f64, // 超前容忍（Spike 用 0.005s）
}
```

- 解码/时钟/同步策略全部放 `core/`，不碰 GPUI 类型，可用普通 `#[test]` 覆盖。
- **不新增持久化**：视频不需要写盘，会话恢复时也不自动恢复播放（只恢复打开的标签）。
- 配置新增 `PreviewConfig.video`：默认音量、是否自动播放（默认不开）。

## 5. 交互流程（正常路径）

1. 目录树双击 `demo.mp4` → `FileKind::Video` 分支 → 通知预览面板进入**视频模式**
   （与图片模式并列：`PreviewMode::{Markdown, Image, Video}`）。
2. 预览面板加载：后台起解复用线程 + 音频线程，读到 `MediaInfo` 后渲染控制条。
3. 播放：视频帧按 pts 对齐音频墙钟后送 UI 绘制；音频 PCM 排入 `rodio::Player`。
4. 用户操作：播放/暂停（暂停视频取帧 + `player.pause()`）、拖进度（seek：
   `ictx.seek()` + 清空两侧缓冲）、拖音量（`player.set_volume()`）。
5. 关闭标签 / 切到别的笔记 → 停止线程、`drop` 掉最后一帧纹理、退出视频模式。

## 6. 异常与边界

| 场景 | 处理 |
|------|------|
| 文件打不开 / 无视频流 | 预览面板显示"无法播放：原因"，不崩、不占线程 |
| 只有画面没有音轨 | 静音播放（Spike 已考虑：`best(Audio)` 为 None 时跳过音频） |
| 解码失败 / 编码不支持 | 打印原因并退到"无法播放"，**绝不在解码线程 panic**（会让整个进程 abort） |
| 视频尺寸 > 1280 宽 | 解码后按显示尺寸缩放（Spike 已做 `scale = w > 1280 ? 2 : 1`），纹理上传量降为 1/4 |
| seek 到关键点前 | ffmpeg 会从最近关键帧开始解码，需丢弃目标时刻之前的帧 |
| 窗口关闭/切标签 | 停止线程并 `drop_image` 最后一帧，避免纹理悬挂 |
| 系统没装 ffmpeg | **当前方案无法运行**（动态链接），需在设置/错误提示里明确告知 |

## 7. 测试计划

**可自动化（`cargo test`）**

| 位置 | 用例 |
|------|------|
| `tests/core/player.rs` | `SyncPolicy` 判定：超前→等待、落后超阈值→丢帧、正常→显示；边界值 |
| | 时间基换算：`pts × time_base` → 秒（Spike 里手算的公式要固化成函数并测） |
| | `MediaInfo` 解析：无音轨、0 时长等 |
| `tests/core/media.rs` | `FileKind::Video` 扩展名判定（mp4/mov/webm/mki 命中，`a.mp4.bak` 不命中） |

**需人工验收**

- 画面颜色正常（无红蓝颠倒）、画面在动。
- **声音与画面对得上**（Spike 已用偏差数据佐证，但仍需人耳确认）。
- 前台窗口下的真实帧率（Spike 的 26fps 疑为后台降频）。
- seek 后音画是否重新对齐。
- 连续播放 5 分钟 RSS 不涨。

## 8. 人工验收清单

- [ ] 目录树能看见 `.mp4` / `.mov` / `.webm`，图标可辨识
- [ ] 双击在预览面板内播放（不新开窗口）
- [ ] 画面颜色正常、流畅（前台帧率 ≥ 视频帧率）
- [ ] 有声音，且声画同步（无“口型对不上”）
- [ ] 播放/暂停、拖进度、调音量都生效
- [ ] 播完自动停止，不占 CPU
- [ ] 切换标签/关闭后 CPU 归零、内存回落到播放前水平
- [ ] 没有 ffmpeg 的机器上给出明确提示而不是崩溃
- [ ] Markdown 与 JSON/YAML 预览行为无任何回归

## 9. 风险与开放问题（**需拍板**）

| # | 问题 | 说明 |
|---|------|------|
| **Q1** | **是否接受系统 ffmpeg 依赖** | 当前是动态链接 `/opt/homebrew` 的 ffmpeg 8.1，编译时需 `PKG_CONFIG_PATH=/opt/homebrew/lib/pkgconfig`。**分发给用户时对方机器必须装 ffmpeg**，否则打不开视频。替代：静态链接（许可 LGPL/GPL 需注意）或随包带 dylib。这是最大的产品决策 |
| **Q2** | 26fps 上限 | 三次测试与负载无关地卡在 26fps，需在前台窗口确认真实上限；若确为 26fps，则 30fps 视频会有轻微丢帧（可接受），60fps 视频必须降帧 |
| **Q3** | 支持哪些格式 | ffmpeg 能解的基本都行（mp4/mov/webm/mkv/avi…），但 UI 上要不要限制在常见几种 |
| **Q4** | 是否接受 macOS 先行 | Spike 全在 macOS 验证；`rodio` 与 `cpal` 跨平台，但 Windows/Linux 未验证（项目定位"macOS 优先，架构上保持可编译"） |
| **Q5** | 自动播放 | 默认关闭（避免打开目录就出声）还是打开 |
| **R1** | ffmpeg API 漂移 | ffmpeg-next 是社区维护的绑定，FFmpeg 升级可能再次不兼容；需在 CI/文档中固定支持的版本范围 |
| **R2** | Spike 临时依赖 | `ffmpeg-next`、`image` 已写进 `Cargo.toml`；若放弃本方案必须一并删除 |

## 10. 实施步骤（每期可独立验证）

| 期 | 内容 | 可验证点 |
|----|------|---------|
| **S1** | `core/media.rs` 加 `FileKind::Video` + 扩展名判据；`vault.rs` / `watcher.rs` 可见性放开；目录树图标 | 树里能看到视频；双击还只是"打不开"（此时不崩） |
| **S2** | `core/player.rs`：解封装/视频解码/时间基换算/`SyncPolicy` + 单测（**不含 UI**） | `cargo test` 通过；命令行可跑通"解码 N 帧并打印 pts" |
| **S3** | 渲染接入：`PreviewMode::Video` + `RenderImage` + **`drop_image`** | 预览面板能出画面（无声） |
| **S4** | 音频接入：`rodio` + 音画同步 | 有声且同步（用 Spike 的偏差统计自检） |
| **S5** | 控制条 UI：播放/暂停、进度（seek）、音量；配置 `preview.video` | 三个控件都生效 |
| **S6** | 清理：删除 Spike bin 与临时依赖、补 `docs/REQUIREMENTS.md` | `cargo tree -d` 干净，无回归 |

> S2 是纯 `core/` 逻辑，可在没有窗口的情况下单测，建议先做——它承载了 pts/同步这些真正有 bug 风险的部分。

## 11. 失败复盘（为什么 Spike 过了、集成却没过）

### 现象

- Spike 的四个独立程序（`spike_video` / `spike_decode` / `spike_play` / `spike_play_av`）
  全部验证通过，其中 `spike_play_av` 实测 15 秒播放 389 帧、音画偏差平均 0.0ms。
- 接进 xink 后：双击视频，**画面始终不出现**。
- 终端日志里 `Dropping DeviceSink` 反复出现（3 次），说明音频输出设备被反复释放。

### 已定位的一个真实缺陷（已修，但没解决全部问题）

主时钟 `AudioClock` 只由**音频线程**在收到第一段 PCM 时启动。音频一旦没起来
（无音轨 / 打不开设备 / 播放器被反复重建导致设备冲突），`now_secs()` 恒为 0，
于是每一帧的 `pts - audio_secs > 0` 都被判成 `Wait` → **画面永远停在首帧之前**。

对策（当时已加）：`AudioClock::is_started()` + 打开超过 800ms 音频仍未启动就用视频墙钟兜底，
保证"没声音也能看画面"。

### 未解决的部分

修完上面这条后画面依旧出不来，没能定位到最终原因。剩下的可疑方向：

1. **播放器实体被反复创建/销毁**：日志显示文件被重复打开 5 次、`DeviceSink` 被释放 3 次，
   说明 `VideoPlayer` 实体在一次点击后经历了多次重建，每次重建都会重启解码与音频线程。
   这与"用户在面板里点了多次"或面板生命周期有关，未查清。
2. **Spike 与真实环境的差异**：Spike 是独立窗口 + 独立 main；xink 里播放器嵌在
   `DockArea` 的预览面板内，受布局恢复、面板重建、`Entity` 生命周期影响，
   帧驱动（`on_next_frame` + `notify`）在嵌套面板里可能没有按预期触发。
3. **音频设备争用**：xink 主进程同时存在别的音频/窗口资源，可能与 rodio 的
   默认输出设备冲突（`open_default_sink` 没有报错但立刻被释放）。

### 教训（下次重做时先看这里）

1. **Spike 通过 ≠ 集成能过**。Spike 是"独立窗口 + 独立 main + 没有 dock/面板生命周期"，
   至少要有一个"嵌进真实面板里的最小版本"才算验证到位。
2. **主时钟不要单挂在音频上**。虽然"音频为主时钟"是业界正确做法，但必须有兜底：
   音频没起来时要用视频墙钟，否则表现是"彻底没画面"，极难联想到根因是音频。
3. **先做无声版**。分两步（先画面后声音）能把"渲染问题"和"音频问题"分开定位，
   这次混在一起做，两轮修复都花在猜上。
4. **给播放器加"实例唯一性"保护**：换视频前必须确保旧播放器及其后台线程真的退出了，
   否则多个解码线程/音频设备互相打架（日志里的重复打开就是征兆）。

### 若将来要重做的低成本替代

不做应用内播放，改为**调用系统播放器打开**：目录树识别视频 → 双击 → `open <file>`。
成本约为本方案的 1/20，且零外部依赖、零分发问题。代价只是不在应用内播放。
