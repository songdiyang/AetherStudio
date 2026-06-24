/// 命令面板条目
#[derive(Clone, Debug)]
pub struct CommandPaletteItem {
    pub label: String,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub command_id: crate::menu_bar::CommandId,
    pub icon: Option<String>,
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
        use crate::menu_bar::CommandId;

        self.items = vec![
            CommandPaletteItem {
                label: "文件: 新建文件".to_string(),
                description: Some("创建一个新的空白文件".to_string()),
                shortcut: Some("Ctrl+N".to_string()),
                command_id: CommandId::FileNew,
                icon: Some("📄".to_string()),
            },
            CommandPaletteItem {
                label: "文件: 打开文件".to_string(),
                description: Some("打开现有文件".to_string()),
                shortcut: Some("Ctrl+O".to_string()),
                command_id: CommandId::FileOpen,
                icon: Some("📂".to_string()),
            },
            CommandPaletteItem {
                label: "文件: 打开文件夹".to_string(),
                description: Some("打开文件夹作为工作区".to_string()),
                shortcut: Some("Ctrl+K".to_string()),
                command_id: CommandId::FileOpenFolder,
                icon: Some("📁".to_string()),
            },
            CommandPaletteItem {
                label: "文件: 保存".to_string(),
                description: Some("保存当前文件".to_string()),
                shortcut: Some("Ctrl+S".to_string()),
                command_id: CommandId::FileSave,
                icon: Some("💾".to_string()),
            },
            CommandPaletteItem {
                label: "文件: 另存为".to_string(),
                description: Some("将文件保存到新位置".to_string()),
                shortcut: Some("Ctrl+Shift+S".to_string()),
                command_id: CommandId::FileSaveAs,
                icon: Some("💾".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 撤销".to_string(),
                description: Some("撤销上一步操作".to_string()),
                shortcut: Some("Ctrl+Z".to_string()),
                command_id: CommandId::EditUndo,
                icon: Some("↩".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 重做".to_string(),
                description: Some("重做已撤销的操作".to_string()),
                shortcut: Some("Ctrl+Y".to_string()),
                command_id: CommandId::EditRedo,
                icon: Some("↪".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 剪切".to_string(),
                description: Some("剪切选中文本".to_string()),
                shortcut: Some("Ctrl+X".to_string()),
                command_id: CommandId::EditCut,
                icon: Some("✂️".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 复制".to_string(),
                description: Some("复制选中文本".to_string()),
                shortcut: Some("Ctrl+C".to_string()),
                command_id: CommandId::EditCopy,
                icon: Some("📋".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 粘贴".to_string(),
                description: Some("粘贴剪贴板内容".to_string()),
                shortcut: Some("Ctrl+V".to_string()),
                command_id: CommandId::EditPaste,
                icon: Some("📋".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 全选".to_string(),
                description: Some("选择全部内容".to_string()),
                shortcut: Some("Ctrl+A".to_string()),
                command_id: CommandId::EditSelectAll,
                icon: Some("☐".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 查找".to_string(),
                description: Some("在文件中查找".to_string()),
                shortcut: Some("Ctrl+F".to_string()),
                command_id: CommandId::EditFind,
                icon: Some("🔍".to_string()),
            },
            CommandPaletteItem {
                label: "编辑: 替换".to_string(),
                description: Some("查找并替换".to_string()),
                shortcut: Some("Ctrl+H".to_string()),
                command_id: CommandId::EditReplace,
                icon: Some("🔄".to_string()),
            },
            CommandPaletteItem {
                label: "视图: 切换侧边栏".to_string(),
                description: Some("显示/隐藏侧边栏".to_string()),
                shortcut: Some("Ctrl+B".to_string()),
                command_id: CommandId::ViewToggleSidebar,
                icon: Some("📑".to_string()),
            },
            CommandPaletteItem {
                label: "视图: 切换活动栏".to_string(),
                description: Some("显示/隐藏活动栏".to_string()),
                shortcut: None,
                command_id: CommandId::ViewToggleActivityBar,
                icon: Some("📊".to_string()),
            },
            CommandPaletteItem {
                label: "视图: 切换状态栏".to_string(),
                description: Some("显示/隐藏状态栏".to_string()),
                shortcut: None,
                command_id: CommandId::ViewToggleStatusBar,
                icon: Some("📊".to_string()),
            },
            CommandPaletteItem {
                label: "转到: 转到文件".to_string(),
                description: Some("快速打开文件".to_string()),
                shortcut: Some("Ctrl+P".to_string()),
                command_id: CommandId::GotoFile,
                icon: Some("🚀".to_string()),
            },
            CommandPaletteItem {
                label: "转到: 转到行".to_string(),
                description: Some("跳转到指定行".to_string()),
                shortcut: Some("Ctrl+G".to_string()),
                command_id: CommandId::GotoLine,
                icon: Some("#".to_string()),
            },
            CommandPaletteItem {
                label: "运行: 启动".to_string(),
                description: Some("运行当前项目".to_string()),
                shortcut: Some("F5".to_string()),
                command_id: CommandId::RunStart,
                icon: Some("▶".to_string()),
            },
            CommandPaletteItem {
                label: "运行: 调试".to_string(),
                description: Some("启动调试器".to_string()),
                shortcut: Some("F9".to_string()),
                command_id: CommandId::RunDebug,
                icon: Some("🐛".to_string()),
            },
            CommandPaletteItem {
                label: "终端: 新建终端".to_string(),
                description: Some("打开集成终端".to_string()),
                shortcut: Some("Ctrl+`".to_string()),
                command_id: CommandId::TerminalNew,
                icon: Some("💻".to_string()),
            },
            CommandPaletteItem {
                label: "帮助: 关于".to_string(),
                description: Some("关于 Aether 编辑器".to_string()),
                shortcut: None,
                command_id: CommandId::HelpAbout,
                icon: Some("ℹ".to_string()),
            },
            CommandPaletteItem {
                label: "文件: 退出".to_string(),
                description: Some("退出编辑器".to_string()),
                shortcut: Some("Alt+F4".to_string()),
                command_id: CommandId::FileExit,
                icon: Some("🚪".to_string()),
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
                        .map_or(false, |d| d.contains(&self.query))
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
