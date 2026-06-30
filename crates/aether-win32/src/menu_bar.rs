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

    /// 从标签中提取助记符字母，如 "文件(F)" -> Some('F')，"帮助(H)" -> Some('H')
    pub fn mnemonic(&self) -> Option<char> {
        // 查找最后一对括号内的单个字符
        let bytes = self.label.as_bytes();
        if bytes.len() >= 3 {
            let n = bytes.len();
            if bytes[n - 1] == b')' && bytes[n - 3] == b'(' {
                let ch = bytes[n - 2] as char;
                if ch.is_ascii_alphabetic() {
                    return Some(ch.to_ascii_uppercase());
                }
            }
        }
        None
    }
}

/// 菜单栏
#[derive(Clone, Debug)]
pub struct MenuBar {
    pub items: Vec<MenuBarItem>,
    pub active_index: Option<usize>,
    pub hover_index: Option<usize>,
    /// 键盘焦点索引（F10/Alt 激活后用于导航）
    pub focus_index: Option<usize>,
    /// 子菜单内键盘焦点索引
    pub submenu_focus_index: Option<usize>,
    /// 是否处于键盘导航模式（用于显示焦点指示器）
    pub keyboard_active: bool,
    pub item_widths: Vec<f32>,
    /// 每个菜单项的 x 位置（用于子菜单定位）
    pub item_x_positions: Vec<f32>,
    /// 布局是否需要重建（菜单项或 DPI 变化时设为 true）
    pub layout_dirty: bool,
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
            focus_index: None,
            submenu_focus_index: None,
            keyboard_active: false,
            item_widths: Vec::new(),
            item_x_positions: Vec::new(),
            layout_dirty: true,
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
        self.focus_index = None;
        self.submenu_focus_index = None;
        self.keyboard_active = false;
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

    // ===== 键盘导航方法 =====

    /// 获取菜单项下首个可选中（非分隔符、非禁用）子项索引
    fn first_enabled_submenu(items: &[MenuItem]) -> Option<usize> {
        items
            .iter()
            .enumerate()
            .find(|(_, it)| it.enabled && it.label != "-" && it.command_id != CommandId::None)
            .map(|(i, _)| i)
    }

    /// 获取菜单项下最后一个可选中子项索引
    fn last_enabled_submenu(items: &[MenuItem]) -> Option<usize> {
        items
            .iter()
            .enumerate()
            .rfind(|(_, it)| it.enabled && it.label != "-" && it.command_id != CommandId::None)
            .map(|(i, _)| i)
    }

    /// 通过助记符激活菜单，返回是否命中
    /// 若菜单已激活，则匹配子菜单项的助记符（首个非分隔符字符）
    pub fn activate_by_mnemonic(&mut self, key: char) -> bool {
        let key = key.to_ascii_uppercase();
        // 顶层菜单未激活：匹配顶层助记符
        if self.active_index.is_none() {
            // 先查找匹配索引，再执行可变操作，避免借用冲突
            let found = self
                .items
                .iter()
                .enumerate()
                .find(|(_, it)| it.mnemonic() == Some(key))
                .map(|(i, it)| (i, Self::first_enabled_submenu(&it.items)));
            if let Some((i, first_sub)) = found {
                self.expand(i);
                self.focus_index = Some(i);
                self.submenu_focus_index = first_sub;
                self.keyboard_active = true;
                return true;
            }
            return false;
        }
        // 顶层菜单已激活：尝试匹配子菜单项首字符（非助记符，是 label 首字符快速跳转）
        if let Some(ai) = self.active_index {
            let found = self.items.get(ai).and_then(|item| {
                item.items.iter().enumerate().find(|(_, sub)| {
                    sub.enabled
                        && sub.label != "-"
                        && sub
                            .label
                            .chars()
                            .next()
                            .map(|c| c.to_ascii_uppercase() == key)
                            .unwrap_or(false)
                })
            });
            if let Some((i, _)) = found {
                self.submenu_focus_index = Some(i);
                return true;
            }
        }
        false
    }

    /// 激活菜单栏（F10 或 Alt 单独按下）：选中第一个菜单项但不展开
    pub fn activate_first(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.keyboard_active = true;
        if self.active_index.is_some() {
            return;
        }
        self.focus_index = Some(0);
    }

    /// 展开当前焦点菜单并定位首个可选中子项（向下键首次展开时调用）
    pub fn expand_focused(&mut self) {
        let fi = match self.focus_index {
            Some(i) => i,
            None => return,
        };
        if self.active_index.is_some() {
            return;
        }
        self.expand(fi);
        if let Some(item) = self.items.get(fi) {
            self.submenu_focus_index = Self::first_enabled_submenu(&item.items);
        }
    }

    /// 关闭菜单并退出键盘模式
    pub fn close_menu(&mut self) {
        self.close_all();
    }

    /// 顶层菜单向左导航
    pub fn navigate_left(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let cur = self.focus_index.unwrap_or(0);
        let new_idx = if cur == 0 {
            self.items.len() - 1
        } else {
            cur - 1
        };
        self.focus_index = Some(new_idx);
        self.expand(new_idx);
        if let Some(item) = self.items.get(new_idx) {
            self.submenu_focus_index = Self::first_enabled_submenu(&item.items);
        }
    }

    /// 顶层菜单向右导航
    pub fn navigate_right(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let cur = self.focus_index.unwrap_or(0);
        let new_idx = if cur + 1 >= self.items.len() {
            0
        } else {
            cur + 1
        };
        self.focus_index = Some(new_idx);
        self.expand(new_idx);
        if let Some(item) = self.items.get(new_idx) {
            self.submenu_focus_index = Self::first_enabled_submenu(&item.items);
        }
    }

    /// 子菜单向上导航
    pub fn navigate_up(&mut self) {
        let ai = match self.active_index {
            Some(i) => i,
            None => return,
        };
        let items = match self.items.get(ai) {
            Some(it) => &it.items,
            None => return,
        };
        let cur = self.submenu_focus_index.unwrap_or(0);
        // 从当前向前找上一个可选项
        let mut idx = if cur == 0 { items.len() } else { cur };
        loop {
            idx = idx.saturating_sub(1);
            if idx >= items.len() {
                break;
            }
            let it = &items[idx];
            if it.enabled && it.label != "-" && it.command_id != CommandId::None {
                self.submenu_focus_index = Some(idx);
                return;
            }
            if idx == 0 {
                break;
            }
        }
        // 未找到上一个，跳到最后一个
        if let Some(last) = Self::last_enabled_submenu(items) {
            self.submenu_focus_index = Some(last);
        }
    }

    /// 子菜单向下导航
    pub fn navigate_down(&mut self) {
        let ai = match self.active_index {
            Some(i) => i,
            None => return,
        };
        let items = match self.items.get(ai) {
            Some(it) => &it.items,
            None => return,
        };
        let cur = self.submenu_focus_index.unwrap_or(0);
        let mut idx = cur;
        loop {
            idx += 1;
            if idx >= items.len() {
                break;
            }
            let it = &items[idx];
            if it.enabled && it.label != "-" && it.command_id != CommandId::None {
                self.submenu_focus_index = Some(idx);
                return;
            }
        }
        // 到底部，跳回第一个
        if let Some(first) = Self::first_enabled_submenu(items) {
            self.submenu_focus_index = Some(first);
        }
    }

    /// 获取当前键盘焦点选中的命令（Enter 触发）
    pub fn focused_command(&self) -> Option<CommandId> {
        let ai = self.active_index?;
        let si = self.submenu_focus_index?;
        let item = self.items.get(ai)?;
        let sub = item.items.get(si)?;
        if sub.enabled && sub.label != "-" && sub.command_id != CommandId::None {
            Some(sub.command_id)
        } else {
            None
        }
    }
}
