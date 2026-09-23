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
        OpenSettings
    ]
);
