/// 菜单项
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub shortcut: Option<String>,
    pub command_id: CommandId,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: &str, command_id: CommandId) -> Self {
        Self {
            label: label.to_string(),
            shortcut: None,
            command_id,
            enabled: true,
        }
    }

    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_string());
        self
    }

    pub fn separator() -> Self {
        Self {
            label: "-".to_string(),
            shortcut: None,
            command_id: CommandId::None,
            enabled: false,
        }
    }
}

/// 命令ID枚举
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    None,
    // 文件
    FileNew,
    FileNewWindow,
    FileOpen,
    FileOpenFolder,
    FileCloseWorkspace,
    FileSave,
    FileSaveAs,
    FileExit,
    // 编辑
    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditFind,
    EditReplace,
    EditSelectAll,
    // 选择
    SelectAll,
    // 查看
    ViewToggleSidebar,
    ViewToggleActivityBar,
    ViewToggleStatusBar,
    ViewZoomIn,
    ViewZoomOut,
    // 转到
    GotoFile,
    GotoLine,
    // 运行
    RunStart,
    RunDebug,
    // 终端
    TerminalNew,
    // 帮助
    HelpAbout,
}

impl CommandId {
    pub fn label(&self) -> &'static str {
        match self {
            CommandId::None => "",
            CommandId::FileNew => "新建文件",
            CommandId::FileNewWindow => "新建窗口",
            CommandId::FileOpen => "打开文件",
            CommandId::FileOpenFolder => "打开文件夹",
            CommandId::FileCloseWorkspace => "关闭工作区",
            CommandId::FileSave => "保存",
            CommandId::FileSaveAs => "另存为",
            CommandId::FileExit => "退出",
            CommandId::EditUndo => "撤销",
            CommandId::EditRedo => "重做",
            CommandId::EditCut => "剪切",
            CommandId::EditCopy => "复制",
            CommandId::EditPaste => "粘贴",
            CommandId::EditFind => "查找",
            CommandId::EditReplace => "替换",
            CommandId::EditSelectAll => "全选",
            CommandId::SelectAll => "全选",
            CommandId::ViewToggleSidebar => "切换侧边栏",
            CommandId::ViewToggleActivityBar => "切换活动栏",
            CommandId::ViewToggleStatusBar => "切换状态栏",
            CommandId::ViewZoomIn => "放大",
            CommandId::ViewZoomOut => "缩小",
            CommandId::GotoFile => "转到文件",
            CommandId::GotoLine => "转到行",
            CommandId::RunStart => "启动",
            CommandId::RunDebug => "调试",
            CommandId::TerminalNew => "新建终端",
            CommandId::HelpAbout => "关于",
        }
    }
}

/// 菜单栏项
#[derive(Clone, Debug)]
pub struct MenuBarItem {
    pub label: String,
    pub items: Vec<MenuItem>,
    pub expanded: bool,
}

impl MenuBarItem {
    pub fn new(label: &str, items: Vec<MenuItem>) -> Self {
        Self {
            label: label.to_string(),
            items,
            expanded: false,
        }
    }

    /// 持久化键：去除 "(F)" 等助记符后的纯文本（如 "文件(F)" → "文件"）
    pub fn key(&self) -> String {
        self.label
            .split('(')
            .next()
            .unwrap_or(&self.label)
            .trim()
            .to_string()
    }
}

/// 菜单栏
#[derive(Clone, Debug)]
pub struct MenuBar {
    pub items: Vec<MenuBarItem>,
    pub active_index: Option<usize>,
    pub hover_index: Option<usize>,
    pub item_widths: Vec<f32>,
    /// 每个菜单项的 x 位置（用于子菜单定位）
    pub item_x_positions: Vec<f32>,
    /// 布局是否需要重建（菜单项或 DPI 变化时设为 true）
    pub layout_dirty: bool,
    /// 自定义模式（长按进入）
    pub customize_mode: bool,
    /// 正在拖拽的项索引
    pub drag_index: Option<usize>,
    /// 拖拽放置目标索引
    pub drop_index: Option<usize>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            items: vec![
                MenuBarItem::new(
                    "文件(F)",
                    vec![
                        MenuItem::new("新建文件", CommandId::FileNew).with_shortcut("Ctrl+N"),
                        MenuItem::new("新建窗口", CommandId::FileNewWindow)
                            .with_shortcut("Ctrl+Shift+N"),
                        MenuItem::new("打开文件...", CommandId::FileOpen).with_shortcut("Ctrl+O"),
                        MenuItem::new("打开文件夹...", CommandId::FileOpenFolder)
                            .with_shortcut("Ctrl+K"),
                        MenuItem::new("关闭工作区", CommandId::FileCloseWorkspace),
                        MenuItem::separator(),
                        MenuItem::new("保存", CommandId::FileSave).with_shortcut("Ctrl+S"),
                        MenuItem::new("另存为...", CommandId::FileSaveAs)
                            .with_shortcut("Ctrl+Shift+S"),
                        MenuItem::separator(),
                        MenuItem::new("退出", CommandId::FileExit),
                    ],
                ),
                MenuBarItem::new(
                    "编辑(E)",
                    vec![
                        MenuItem::new("撤销", CommandId::EditUndo).with_shortcut("Ctrl+Z"),
                        MenuItem::new("重做", CommandId::EditRedo).with_shortcut("Ctrl+Y"),
                        MenuItem::separator(),
                        MenuItem::new("剪切", CommandId::EditCut).with_shortcut("Ctrl+X"),
                        MenuItem::new("复制", CommandId::EditCopy).with_shortcut("Ctrl+C"),
                        MenuItem::new("粘贴", CommandId::EditPaste).with_shortcut("Ctrl+V"),
                        MenuItem::separator(),
                        MenuItem::new("查找", CommandId::EditFind).with_shortcut("Ctrl+F"),
                        MenuItem::new("替换", CommandId::EditReplace).with_shortcut("Ctrl+H"),
                        MenuItem::separator(),
                        MenuItem::new("全选", CommandId::EditSelectAll).with_shortcut("Ctrl+A"),
                    ],
                ),
                MenuBarItem::new(
                    "选择(S)",
                    vec![MenuItem::new("全选", CommandId::SelectAll).with_shortcut("Ctrl+A")],
                ),
                MenuBarItem::new(
                    "查看(V)",
                    vec![
                        MenuItem::new("切换侧边栏", CommandId::ViewToggleSidebar)
                            .with_shortcut("Ctrl+B"),
                        MenuItem::new("切换活动栏", CommandId::ViewToggleActivityBar),
                        MenuItem::new("切换状态栏", CommandId::ViewToggleStatusBar),
                        MenuItem::separator(),
                        MenuItem::new("放大", CommandId::ViewZoomIn).with_shortcut("Ctrl+="),
                        MenuItem::new("缩小", CommandId::ViewZoomOut).with_shortcut("Ctrl+-"),
                    ],
                ),
                MenuBarItem::new(
                    "转到(G)",
                    vec![
                        MenuItem::new("转到文件...", CommandId::GotoFile).with_shortcut("Ctrl+P"),
                        MenuItem::new("转到行...", CommandId::GotoLine).with_shortcut("Ctrl+G"),
                    ],
                ),
                MenuBarItem::new(
                    "运行(R)",
                    vec![
                        MenuItem::new("启动调试", CommandId::RunDebug).with_shortcut("F5"),
                        MenuItem::new("运行", CommandId::RunStart).with_shortcut("Ctrl+F5"),
                    ],
                ),
                MenuBarItem::new(
                    "终端(T)",
                    vec![MenuItem::new("新建终端", CommandId::TerminalNew)
                        .with_shortcut("Ctrl+Shift+`")],
                ),
                MenuBarItem::new("帮助(H)", vec![MenuItem::new("关于", CommandId::HelpAbout)]),
            ],
            active_index: None,
            hover_index: None,
            item_widths: Vec::new(),
            item_x_positions: Vec::new(),
            layout_dirty: true,
            customize_mode: false,
            drag_index: None,
            drop_index: None,
        }
    }

    /// 获取菜单项的 x 坐标
    pub fn item_x(&self, index: usize) -> f32 {
        let mut x = 0.0;
        for i in 0..index.min(self.item_widths.len()) {
            x += self.item_widths[i];
        }
        x
    }

    /// 查找点击的菜单项索引
    pub fn hit_test(&self, x: f32, y: f32, menu_height: f32) -> Option<usize> {
        if x < 0.0 || y < 0.0 || y > menu_height {
            return None;
        }
        let mut current_x = 0.0;
        for (i, width) in self.item_widths.iter().enumerate() {
            if x >= current_x && x < current_x + width {
                return Some(i);
            }
            current_x += width;
        }
        None
    }

    /// 关闭所有展开的菜单
    pub fn close_all(&mut self) {
        self.active_index = None;
        for item in &mut self.items {
            item.expanded = false;
        }
    }

    /// 展开指定菜单
    pub fn expand(&mut self, index: usize) {
        self.active_index = Some(index);
        for (i, item) in &mut self.items.iter_mut().enumerate() {
            item.expanded = i == index;
        }
    }

    /// 获取展开菜单的子项区域
    pub fn submenu_region(
        &self,
        menu_index: usize,
        x: f32,
        y: f32,
    ) -> Vec<(f32, f32, f32, f32, usize)> {
        let mut regions = Vec::new();
        if let Some(item) = self.items.get(menu_index) {
            if !item.expanded {
                return regions;
            }
            let mut item_y = y + 8.0;
            for (i, menu_item) in item.items.iter().enumerate() {
                let height = if menu_item.label == "-" { 8.0 } else { 26.0 };
                regions.push((x, item_y, 200.0, height, i));
                item_y += height;
            }
        }
        regions
    }

    /// 查找子菜单项点击
    pub fn hit_test_submenu(
        &self,
        menu_index: usize,
        x: f32,
        y: f32,
        menu_x: f32,
        menu_y: f32,
    ) -> Option<usize> {
        let regions = self.submenu_region(menu_index, menu_x, menu_y);
        for (rx, ry, rw, rh, idx) in regions {
            if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
                return Some(idx);
            }
        }
        None
    }

    /// 进入自定义模式并开始拖拽指定项
    pub fn begin_drag(&mut self, index: usize) {
        self.customize_mode = true;
        // 进入自定义模式时关闭所有展开的子菜单
        self.close_all();
        self.drag_index = Some(index);
        self.drop_index = Some(index);
    }

    /// 退出自定义模式
    pub fn exit_customize(&mut self) {
        self.customize_mode = false;
        self.drag_index = None;
        self.drop_index = None;
    }

    /// 根据鼠标 x 计算放置目标索引（0..=items.len()）
    pub fn drop_index_at(&self, x: f32) -> usize {
        let mut current_x = 0.0;
        for (i, width) in self.item_widths.iter().enumerate() {
            let mid = current_x + width / 2.0;
            if x < mid {
                return i;
            }
            current_x += width;
        }
        self.items.len()
    }

    /// 执行重排：将 drag_index 移到 drop_index 位置
    pub fn reorder(&mut self) {
        if let (Some(from), Some(to)) = (self.drag_index, self.drop_index) {
            if from < self.items.len() && to <= self.items.len() && from != to {
                let item = self.items.remove(from);
                let insert_at = if to > from { to - 1 } else { to };
                let insert_at = insert_at.min(self.items.len());
                self.items.insert(insert_at, item);
                // active_index 跟随移动
                if self.active_index == Some(from) {
                    self.active_index = Some(insert_at);
                } else if let Some(ai) = self.active_index {
                    if from < ai && to >= ai {
                        self.active_index = Some(ai - 1);
                    } else if from > ai && to <= ai {
                        self.active_index = Some(ai + 1);
                    }
                }
                self.layout_dirty = true;
            }
        }
    }

    /// 当前顺序的键列表（用于持久化）
    pub fn order_keys(&self) -> Vec<String> {
        self.items.iter().map(|i| i.key()).collect()
    }

    /// 应用持久化的顺序（保留默认项中存在但配置缺失的项）
    pub fn apply_order(&mut self, keys: &[String]) {
        let mut new_items: Vec<MenuBarItem> = Vec::new();
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for k in keys {
            if let Some(item) = self.items.iter().find(|i| i.key() == *k).cloned() {
                if used.insert(k.clone()) {
                    new_items.push(item);
                }
            }
        }
        // 补充默认顺序中未被配置覆盖的项
        for item in &self.items {
            let k = item.key();
            if !used.contains(&k) {
                new_items.push(item.clone());
            }
        }
        if !new_items.is_empty() {
            // 重排后强制关闭所有展开状态
            for item in &mut new_items {
                item.expanded = false;
            }
            self.items = new_items;
            self.active_index = None;
            self.layout_dirty = true;
        }
    }
}
