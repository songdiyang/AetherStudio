/// 命令面板条目
#[derive(Clone, Debug)]
pub struct CommandPaletteItem {
    pub label: String,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub command_id: crate::menu_bar::CommandId,
    pub icon: Option<crate::icons::IconKind>,
}

/// 命令面板状态
#[derive(Clone, Debug, Default)]
pub struct CommandPalette {
    pub visible: bool,
    pub query: String,
    pub items: Vec<CommandPaletteItem>,
    pub filtered_items: Vec<usize>, // 索引到 items
    pub selected_index: usize,
    pub max_visible_items: usize,
}

impl CommandPalette {
    pub fn new() -> Self {
        let mut palette = Self {
            visible: false,
            query: String::new(),
            items: Vec::new(),
            filtered_items: Vec::new(),
            selected_index: 0,
            max_visible_items: 15,
        };
        palette.build_command_list();
        palette
    }

    /// 构建所有可用命令列表
    fn build_command_list(&mut self) {
        use crate::icons::IconKind;
        use crate::menu_bar::CommandId;

        self.items = vec![
            CommandPaletteItem {
                label: "文件: 新建项目".to_string(),
                description: Some("在用户文档目录下创建新项目文件夹".to_string()),
                shortcut: Some("Ctrl+N".to_string()),
                command_id: CommandId::FileNew,
                icon: Some(IconKind::NewFile),
            },
            CommandPaletteItem {
                label: "文件: 打开文件".to_string(),
                description: Some("打开现有文件".to_string()),
                shortcut: Some("Ctrl+O".to_string()),
                command_id: CommandId::FileOpen,
                icon: Some(IconKind::File),
            },
            CommandPaletteItem {
                label: "文件: 打开文件夹".to_string(),
                description: Some("打开文件夹作为工作区".to_string()),
                shortcut: Some("Ctrl+K".to_string()),
                command_id: CommandId::FileOpenFolder,
                icon: Some(IconKind::Folder),
            },
            CommandPaletteItem {
                label: "文件: 保存".to_string(),
                description: Some("保存当前文件".to_string()),
                shortcut: Some("Ctrl+S".to_string()),
                command_id: CommandId::FileSave,
                icon: Some(IconKind::Save),
            },
            CommandPaletteItem {
                label: "文件: 另存为".to_string(),
                description: Some("将文件保存到新位置".to_string()),
                shortcut: Some("Ctrl+Shift+S".to_string()),
                command_id: CommandId::FileSaveAs,
                icon: Some(IconKind::Save),
            },
            CommandPaletteItem {
                label: "编辑: 撤销".to_string(),
                description: Some("撤销上一步操作".to_string()),
                shortcut: Some("Ctrl+Z".to_string()),
                command_id: CommandId::EditUndo,
                icon: Some(IconKind::Undo),
            },
            CommandPaletteItem {
                label: "编辑: 重做".to_string(),
                description: Some("重做已撤销的操作".to_string()),
                shortcut: Some("Ctrl+Y".to_string()),
                command_id: CommandId::EditRedo,
                icon: Some(IconKind::Redo),
            },
            CommandPaletteItem {
                label: "编辑: 剪切".to_string(),
                description: Some("剪切选中文本".to_string()),
                shortcut: Some("Ctrl+X".to_string()),
                command_id: CommandId::EditCut,
                icon: Some(IconKind::Cut),
            },
            CommandPaletteItem {
                label: "编辑: 复制".to_string(),
                description: Some("复制选中文本".to_string()),
                shortcut: Some("Ctrl+C".to_string()),
                command_id: CommandId::EditCopy,
                icon: Some(IconKind::Copy),
            },
            CommandPaletteItem {
                label: "编辑: 粘贴".to_string(),
                description: Some("粘贴剪贴板内容".to_string()),
                shortcut: Some("Ctrl+V".to_string()),
                command_id: CommandId::EditPaste,
                icon: Some(IconKind::Paste),
            },
            CommandPaletteItem {
                label: "编辑: 全选".to_string(),
                description: Some("选择全部内容".to_string()),
                shortcut: Some("Ctrl+A".to_string()),
                command_id: CommandId::EditSelectAll,
                icon: Some(IconKind::SelectAll),
            },
            CommandPaletteItem {
                label: "编辑: 查找".to_string(),
                description: Some("在文件中查找".to_string()),
                shortcut: Some("Ctrl+F".to_string()),
                command_id: CommandId::EditFind,
                icon: Some(IconKind::Search),
            },
            CommandPaletteItem {
                label: "编辑: 替换".to_string(),
                description: Some("查找并替换".to_string()),
                shortcut: Some("Ctrl+H".to_string()),
                command_id: CommandId::EditReplace,
                icon: Some(IconKind::Replace),
            },
            CommandPaletteItem {
                label: "视图: 切换侧边栏".to_string(),
                description: Some("显示/隐藏侧边栏".to_string()),
                shortcut: Some("Ctrl+B".to_string()),
                command_id: CommandId::ViewToggleSidebar,
                icon: Some(IconKind::Sidebar),
            },
            CommandPaletteItem {
                label: "视图: 切换活动栏".to_string(),
                description: Some("显示/隐藏活动栏".to_string()),
                shortcut: None,
                command_id: CommandId::ViewToggleActivityBar,
                icon: Some(IconKind::PanelLeft),
            },
            CommandPaletteItem {
                label: "视图: 切换状态栏".to_string(),
                description: Some("显示/隐藏状态栏".to_string()),
                shortcut: None,
                command_id: CommandId::ViewToggleStatusBar,
                icon: Some(IconKind::PanelBottom),
            },
            CommandPaletteItem {
                label: "转到: 转到文件".to_string(),
                description: Some("快速打开文件".to_string()),
                shortcut: Some("Ctrl+P".to_string()),
                command_id: CommandId::GotoFile,
                icon: Some(IconKind::GotoFile),
            },
            CommandPaletteItem {
                label: "转到: 转到行".to_string(),
                description: Some("跳转到指定行".to_string()),
                shortcut: Some("Ctrl+G".to_string()),
                command_id: CommandId::GotoLine,
                icon: Some(IconKind::Hash),
            },
            CommandPaletteItem {
                label: "运行: 启动".to_string(),
                description: Some("运行当前项目".to_string()),
                shortcut: Some("F5".to_string()),
                command_id: CommandId::RunStart,
                icon: Some(IconKind::Play),
            },
            CommandPaletteItem {
                label: "运行: 调试".to_string(),
                description: Some("启动调试器".to_string()),
                shortcut: Some("F9".to_string()),
                command_id: CommandId::RunDebug,
                icon: Some(IconKind::Bug),
            },
            CommandPaletteItem {
                label: "终端: 新建终端".to_string(),
                description: Some("打开集成终端".to_string()),
                shortcut: Some("Ctrl+`".to_string()),
                command_id: CommandId::TerminalNew,
                icon: Some(IconKind::Terminal),
            },
            CommandPaletteItem {
                label: "搜索: 全局搜索".to_string(),
                description: Some("在工作区中搜索文本".to_string()),
                shortcut: Some("Ctrl+Shift+F".to_string()),
                command_id: CommandId::SearchGlobal,
                icon: Some(IconKind::Search),
            },
            CommandPaletteItem {
                label: "AI: 修复当前诊断".to_string(),
                description: Some("把当前文件的 LSP 错误发送给 AI 修复".to_string()),
                shortcut: Some("Ctrl+Shift+D".to_string()),
                command_id: CommandId::AiFixDiagnostics,
                icon: Some(IconKind::Info),
            },
            CommandPaletteItem {
                label: "帮助: 关于".to_string(),
                description: Some("关于 Aether 编辑器".to_string()),
                shortcut: None,
                command_id: CommandId::HelpAbout,
                icon: Some(IconKind::Info),
            },
            CommandPaletteItem {
                label: "文件: 退出".to_string(),
                description: Some("退出编辑器".to_string()),
                shortcut: Some("Alt+F4".to_string()),
                command_id: CommandId::FileExit,
                icon: Some(IconKind::Exit),
            },
        ];
    }

    /// 显示命令面板
    pub fn show(&mut self) {
        self.visible = true;
        self.query.clear();
        self.selected_index = 0;
        self.filter();
    }

    /// 隐藏命令面板
    pub fn hide(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// 切换显示状态
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// 更新搜索查询
    pub fn update_query(&mut self, query: &str) {
        self.query = query.to_lowercase();
        self.selected_index = 0;
        self.filter();
    }

    /// 追加字符到查询
    pub fn append_query(&mut self, ch: char) {
        self.query.push(ch);
        self.selected_index = 0;
        self.filter();
    }

    /// 删除查询最后一个字符
    pub fn backspace_query(&mut self) {
        self.query.pop();
        self.selected_index = 0;
        self.filter();
    }

    /// 选择上一个
    pub fn select_prev(&mut self) {
        if !self.filtered_items.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    /// 选择下一个
    pub fn select_next(&mut self) {
        if !self.filtered_items.is_empty() && self.selected_index + 1 < self.filtered_items.len() {
            self.selected_index += 1;
        }
    }

    /// 获取当前选中的命令
    pub fn selected_command(&self) -> Option<CommandId> {
        self.filtered_items
            .get(self.selected_index)
            .and_then(|&idx| self.items.get(idx))
            .map(|item| item.command_id)
    }

    /// 模糊过滤
    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered_items = (0..self.items.len()).collect();
            return;
        }

        self.filtered_items = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                let label_lower = item.label.to_lowercase();
                let desc_lower = item.description.as_ref().map(|d| d.to_lowercase());

                // 简单包含匹配（生产环境可用 fuzzy-matcher 库）
                label_lower.contains(&self.query)
                    || desc_lower
                        .as_ref()
                        .is_some_and(|d| d.contains(&self.query))
            })
            .map(|(idx, _)| idx)
            .collect();
    }

    /// 获取可见条目数量
    pub fn visible_count(&self) -> usize {
        self.filtered_items.len().min(self.max_visible_items)
    }

    /// 获取指定索引的条目（相对于过滤后的列表）
    pub fn get_item(&self, index: usize) -> Option<&CommandPaletteItem> {
        self.filtered_items
            .get(index)
            .and_then(|&idx| self.items.get(idx))
    }
}

use crate::menu_bar::CommandId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_new_has_items() {
        let mut palette = CommandPalette::new();
        assert!(!palette.items.is_empty());
        // new() 不会自动 filter，需 show() 后 filtered_items 才填充
        palette.show();
        assert!(!palette.filtered_items.is_empty());
        assert_eq!(palette.selected_index, 0);
        assert_eq!(palette.query, "");
    }

    #[test]
    fn test_palette_filter_query() {
        let mut palette = CommandPalette::new();
        palette.update_query("保存");
        assert!(!palette.filtered_items.is_empty());
        for &idx in &palette.filtered_items {
            let item = &palette.items[idx];
            assert!(
                item.label.to_lowercase().contains("保存")
                    || item.description.as_ref().unwrap_or(&String::new()).to_lowercase().contains("保存")
            );
        }
    }

    #[test]
    fn test_palette_filter_no_match() {
        let mut palette = CommandPalette::new();
        palette.update_query("xyz_not_exist");
        assert!(palette.filtered_items.is_empty());
        assert_eq!(palette.visible_count(), 0);
    }

    #[test]
    fn test_palette_select_next_prev() {
        let mut palette = CommandPalette::new();
        palette.update_query("文件");
        let count = palette.filtered_items.len();
        assert!(count > 1);
        palette.select_next();
        assert_eq!(palette.selected_index, 1);
        palette.select_prev();
        assert_eq!(palette.selected_index, 0);
        palette.select_prev();
        assert_eq!(palette.selected_index, 0);
    }

    #[test]
    fn test_palette_selected_command() {
        let mut palette = CommandPalette::new();
        palette.update_query("打开文件");
        let command = palette.selected_command();
        assert!(command.is_some());
        assert_eq!(command.unwrap(), CommandId::FileOpen);
    }

    #[test]
    fn test_palette_show_hide_toggle() {
        let mut palette = CommandPalette::new();
        assert!(!palette.visible);
        palette.show();
        assert!(palette.visible);
        palette.hide();
        assert!(!palette.visible);
        palette.toggle();
        assert!(palette.visible);
        palette.toggle();
        assert!(!palette.visible);
    }

    #[test]
    fn test_palette_append_and_backspace() {
        let mut palette = CommandPalette::new();
        palette.append_query('保');
        palette.append_query('存');
        assert_eq!(palette.query, "保存");
        palette.backspace_query();
        assert_eq!(palette.query, "保");
    }

    #[test]
    fn test_palette_get_item() {
        let mut palette = CommandPalette::new();
        palette.show();
        assert!(palette.get_item(0).is_some());
        assert!(palette.get_item(1000).is_none());
    }
}
