//! 应用级 Action 定义（快捷键与命令面板的统一入口）

use gpui_kit::*;

actions!(
    ink,
    [
        /// 打开一个文件夹作为笔记库
        OpenFolder,
        /// 新建笔记
        NewNote,
        /// 新建文件夹
        NewFolder,
        /// 删除选中项
        DeleteItem,
        /// 重命名选中项（F2，或右键菜单触发）
        RenameItem,
        /// 保存当前笔记
        Save,
        /// 打开设置
        OpenSettings,
        /// 加粗：包裹选中文本或插入 ** 标记
        FormatBold,
        /// 斜体：包裹选中文本或插入 * 标记
        FormatItalic,
        /// 删除线：包裹选中文本或插入 ~~ 标记
        FormatStrikethrough,
        /// 行内代码：包裹选中文本或插入 ` 标记
        FormatInlineCode,
        /// 链接：包裹选中文本或插入 [](url) 占位
        FormatLink,
        /// 在浅色 / 深色之间切换主题
        ToggleTheme,
        /// 打开命令面板：按文件名搜索并打开笔记
        OpenCommandPalette,
        /// 打开全文搜索：按内容检索
        OpenSearch,
        /// 打开 / 关闭编辑器下方的 AI 对话条
        ToggleAiChat
    ]
);
