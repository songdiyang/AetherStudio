//! 资源管理器（Explorer）空白区域上下文菜单与文件节点上下文菜单。
//!
//! 当用户在侧边栏文件树的空白区域右键点击时显示空白区域菜单。
//! 当用户在文件/文件夹节点上右键点击时显示文件节点菜单。
//! 菜单由 Direct2D 自绘（与 user_menu 保持一致），
//! 命中测试通过 `hit_test_menu` 完成。
//!
//! 空白区域菜单项（VS Code 风格的空白区域标准操作）：
//!   新建文件 / 新建文件夹 / 分隔符 / 刷新 / 分隔符 /
//!   在文件资源管理器中打开 / 分隔符 / 复制路径
//!
//! 文件节点菜单项：
//!   重命名 / 删除 / 分隔符 /
//!   在文件资源管理器中打开 / 复制路径

/// 资源管理器空白区域上下文菜单项
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerContextMenuItem {
    NewFile,
    NewFolder,
    Separator1,
    Refresh,
    Separator2,
    RevealInExplorer,
    Separator3,
    CopyPath,
}

impl ExplorerContextMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            ExplorerContextMenuItem::NewFile => "新建文件",
            ExplorerContextMenuItem::NewFolder => "新建文件夹",
            ExplorerContextMenuItem::Separator1 => "",
            ExplorerContextMenuItem::Refresh => "刷新",
            ExplorerContextMenuItem::Separator2 => "",
            ExplorerContextMenuItem::RevealInExplorer => "在文件资源管理器中打开",
            ExplorerContextMenuItem::Separator3 => "",
            ExplorerContextMenuItem::CopyPath => "复制路径",
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(
            self,
            ExplorerContextMenuItem::Separator1
                | ExplorerContextMenuItem::Separator2
                | ExplorerContextMenuItem::Separator3
        )
    }
}

/// 文件节点上下文菜单项（文件/文件夹右键）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileNodeContextMenuItem {
    Rename,
    Delete,
    Separator1,
    RevealInExplorer,
    CopyPath,
}

impl FileNodeContextMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            FileNodeContextMenuItem::Rename => "重命名",
            FileNodeContextMenuItem::Delete => "删除",
            FileNodeContextMenuItem::Separator1 => "",
            FileNodeContextMenuItem::RevealInExplorer => "在文件资源管理器中打开",
            FileNodeContextMenuItem::CopyPath => "复制路径",
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(self, FileNodeContextMenuItem::Separator1)
    }
}

/// 资源管理器上下文菜单状态（空白区域）
#[derive(Clone, Debug)]
pub struct ExplorerContextMenu {
    /// 菜单是否展开
    pub is_open: bool,
    /// 鼠标悬停的菜单项索引（指向 `items` 中的位置；分隔符不可悬停）
    pub hover_index: Option<usize>,
    /// 菜单项列表
    pub items: Vec<ExplorerContextMenuItem>,
    /// 菜单区域（用于点击检测和渲染，逻辑像素）
    pub menu_rect: Option<crate::layout::Region>,
    /// 菜单弹出位置 x（逻辑像素，已做边界校正前的原始请求位置，留作调试）
    pub origin_x: f32,
    /// 菜单弹出位置 y（逻辑像素）
    pub origin_y: f32,
}

impl ExplorerContextMenu {
    /// 单项高度（与 user_menu 保持一致）
    pub const ITEM_HEIGHT: f32 = 32.0;
    /// 分隔符高度
    pub const SEPARATOR_HEIGHT: f32 = 9.0;
    /// 菜单宽度
    pub const MENU_WIDTH: f32 = 220.0;
    /// 顶部 padding（无 user_menu 的用户名头部，留少量上边距）
    pub const TOP_PADDING: f32 = 4.0;
    /// 底部 padding
    pub const BOTTOM_PADDING: f32 = 6.0;

    pub fn new() -> Self {
        Self {
            is_open: false,
            hover_index: None,
            items: vec![
                ExplorerContextMenuItem::NewFile,
                ExplorerContextMenuItem::NewFolder,
                ExplorerContextMenuItem::Separator1,
                ExplorerContextMenuItem::Refresh,
                ExplorerContextMenuItem::Separator2,
                ExplorerContextMenuItem::RevealInExplorer,
                ExplorerContextMenuItem::Separator3,
                ExplorerContextMenuItem::CopyPath,
            ],
            menu_rect: None,
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }

    /// 打开菜单到指定位置（逻辑像素，客户区坐标）
    pub fn open(&mut self, x: f32, y: f32, window_width: f32, window_height: f32) {
        self.is_open = true;
        self.hover_index = None;
        // 边界校正：保证菜单不超出窗口右下边界
        let max_x = (window_width - Self::MENU_WIDTH).max(4.0);
        let menu_h = self.menu_height();
        let max_y = (window_height - menu_h).max(4.0);
        self.origin_x = x.min(max_x).max(4.0);
        self.origin_y = y.min(max_y).max(4.0);
    }

    /// 关闭菜单
    pub fn close(&mut self) {
        self.is_open = false;
        self.hover_index = None;
        self.menu_rect = None;
    }

    /// 计算菜单总高度
    pub fn menu_height(&self) -> f32 {
        let content: f32 = self
            .items
            .iter()
            .map(|item| {
                if item.is_separator() {
                    Self::SEPARATOR_HEIGHT
                } else {
                    Self::ITEM_HEIGHT
                }
            })
            .sum();
        Self::TOP_PADDING + content + Self::BOTTOM_PADDING
    }

    /// 菜单宽度
    pub fn menu_width(&self) -> f32 {
        Self::MENU_WIDTH
    }

    /// 命中测试：返回点击的菜单项在 `items` 中的索引（分隔符不可命中）。
    /// 必须在 `menu_rect` 已由渲染阶段设置后调用。
    pub fn hit_test_menu(&self, mouse_x: f32, mouse_y: f32) -> Option<usize> {
        let rect = self.menu_rect.clone()?;
        if !rect.contains(mouse_x, mouse_y) {
            return None;
        }
        let mut current_y = rect.y + Self::TOP_PADDING;
        for (i, item) in self.items.iter().enumerate() {
            let h = if item.is_separator() {
                Self::SEPARATOR_HEIGHT
            } else {
                Self::ITEM_HEIGHT
            };
            let in_row = mouse_y >= current_y && mouse_y < current_y + h;
            if in_row && !item.is_separator() {
                return Some(i);
            }
            current_y += h;
        }
        None
    }

    /// 根据鼠标位置更新 hover_index，返回是否有变化
    pub fn update_hover(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        if !self.is_open {
            return false;
        }
        let new = self.hit_test_menu(mouse_x, mouse_y);
        if new != self.hover_index {
            self.hover_index = new;
            return true;
        }
        false
    }
}

impl Default for ExplorerContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// 文件节点上下文菜单状态（文件/文件夹右键）
#[derive(Clone, Debug)]
pub struct FileNodeContextMenu {
    /// 菜单是否展开
    pub is_open: bool,
    /// 鼠标悬停的菜单项索引
    pub hover_index: Option<usize>,
    /// 菜单项列表
    pub items: Vec<FileNodeContextMenuItem>,
    /// 菜单区域（用于点击检测和渲染，逻辑像素）
    pub menu_rect: Option<crate::layout::Region>,
    /// 菜单弹出位置 x
    pub origin_x: f32,
    /// 菜单弹出位置 y
    pub origin_y: f32,
    /// 右键点击时命中的节点索引
    pub target_node: Option<u32>,
}

impl FileNodeContextMenu {
    pub const ITEM_HEIGHT: f32 = 32.0;
    pub const SEPARATOR_HEIGHT: f32 = 9.0;
    pub const MENU_WIDTH: f32 = 220.0;
    pub const TOP_PADDING: f32 = 4.0;
    pub const BOTTOM_PADDING: f32 = 6.0;

    pub fn new() -> Self {
        Self {
            is_open: false,
            hover_index: None,
            items: vec![
                FileNodeContextMenuItem::Rename,
                FileNodeContextMenuItem::Delete,
                FileNodeContextMenuItem::Separator1,
                FileNodeContextMenuItem::RevealInExplorer,
                FileNodeContextMenuItem::CopyPath,
            ],
            menu_rect: None,
            origin_x: 0.0,
            origin_y: 0.0,
            target_node: None,
        }
    }

    pub fn open(&mut self, x: f32, y: f32, window_width: f32, window_height: f32, node_idx: u32) {
        self.is_open = true;
        self.hover_index = None;
        self.target_node = Some(node_idx);
        let max_x = (window_width - Self::MENU_WIDTH).max(4.0);
        let menu_h = self.menu_height();
        let max_y = (window_height - menu_h).max(4.0);
        self.origin_x = x.min(max_x).max(4.0);
        self.origin_y = y.min(max_y).max(4.0);
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.hover_index = None;
        self.menu_rect = None;
        self.target_node = None;
    }

    pub fn menu_height(&self) -> f32 {
        let content: f32 = self
            .items
            .iter()
            .map(|item| {
                if item.is_separator() {
                    Self::SEPARATOR_HEIGHT
                } else {
                    Self::ITEM_HEIGHT
                }
            })
            .sum();
        Self::TOP_PADDING + content + Self::BOTTOM_PADDING
    }

    pub fn menu_width(&self) -> f32 {
        Self::MENU_WIDTH
    }

    pub fn hit_test_menu(&self, mouse_x: f32, mouse_y: f32) -> Option<usize> {
        let rect = self.menu_rect.clone()?;
        if !rect.contains(mouse_x, mouse_y) {
            return None;
        }
        let mut current_y = rect.y + Self::TOP_PADDING;
        for (i, item) in self.items.iter().enumerate() {
            let h = if item.is_separator() {
                Self::SEPARATOR_HEIGHT
            } else {
                Self::ITEM_HEIGHT
            };
            let in_row = mouse_y >= current_y && mouse_y < current_y + h;
            if in_row && !item.is_separator() {
                return Some(i);
            }
            current_y += h;
        }
        None
    }

    pub fn update_hover(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        if !self.is_open {
            return false;
        }
        let new = self.hit_test_menu(mouse_x, mouse_y);
        if new != self.hover_index {
            self.hover_index = new;
            return true;
        }
        false
    }
}

impl Default for FileNodeContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_menu_at(x: f32, y: f32) -> ExplorerContextMenu {
        let mut m = ExplorerContextMenu::new();
        m.open(x, y, 1280.0, 800.0);
        // 模拟渲染阶段写入 menu_rect（与 render_explorer_context_menu 一致）
        let w = m.menu_width();
        let h = m.menu_height();
        m.menu_rect = Some(crate::layout::Region::new(m.origin_x, m.origin_y, w, h));
        m
    }

    #[test]
    fn test_new_menu_closed() {
        let m = ExplorerContextMenu::new();
        assert!(!m.is_open);
        assert_eq!(m.hover_index, None);
        assert!(m.menu_rect.is_none());
        // 标准空白区域菜单项齐全
        assert!(m.items.contains(&ExplorerContextMenuItem::NewFile));
        assert!(m.items.contains(&ExplorerContextMenuItem::NewFolder));
        assert!(m.items.contains(&ExplorerContextMenuItem::Refresh));
        assert!(m.items.contains(&ExplorerContextMenuItem::RevealInExplorer));
        assert!(m.items.contains(&ExplorerContextMenuItem::CopyPath));
    }

    #[test]
    fn test_open_close() {
        let mut m = ExplorerContextMenu::new();
        m.open(100.0, 200.0, 1280.0, 800.0);
        assert!(m.is_open);
        assert_eq!(m.hover_index, None);
        assert!((m.origin_x - 100.0).abs() < f32::EPSILON);
        assert!((m.origin_y - 200.0).abs() < f32::EPSILON);

        m.close();
        assert!(!m.is_open);
        assert_eq!(m.hover_index, None);
        assert!(m.menu_rect.is_none());
    }

    #[test]
    fn test_open_clamps_to_window_bounds() {
        let mut m = ExplorerContextMenu::new();
        // 右下角超出窗口 → 应被钳制到窗口内
        m.open(2000.0, 2000.0, 1280.0, 800.0);
        let max_x = 1280.0 - ExplorerContextMenu::MENU_WIDTH;
        let max_y = 800.0 - m.menu_height();
        assert!(m.origin_x <= max_x);
        assert!(m.origin_y <= max_y);
        assert!(m.origin_x >= 4.0);
        assert!(m.origin_y >= 4.0);
    }

    #[test]
    fn test_menu_height_includes_all_items_and_padding() {
        let m = ExplorerContextMenu::new();
        let h = m.menu_height();
        // 5 个普通项 + 3 个分隔符 + 上下 padding
        let expected = 5.0 * ExplorerContextMenu::ITEM_HEIGHT
            + 3.0 * ExplorerContextMenu::SEPARATOR_HEIGHT
            + ExplorerContextMenu::TOP_PADDING
            + ExplorerContextMenu::BOTTOM_PADDING;
        assert!((h - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_outside_menu_returns_none() {
        let m = open_menu_at(100.0, 100.0);
        // 完全在菜单外
        assert_eq!(m.hit_test_menu(10.0, 10.0), None);
        // 紧贴菜单右边界外侧
        let rect = m.menu_rect.clone().unwrap();
        assert_eq!(m.hit_test_menu(rect.right() + 1.0, rect.y + 5.0), None);
    }

    #[test]
    fn test_hit_test_first_item_and_skips_separator() {
        let m = open_menu_at(100.0, 100.0);
        let rect = m.menu_rect.clone().unwrap();
        // 第一项（NewFile）位于 top_padding 起始处
        let first_y = rect.y + ExplorerContextMenu::TOP_PADDING + 1.0;
        let idx = m.hit_test_menu(rect.x + 20.0, first_y).unwrap();
        assert_eq!(m.items[idx], ExplorerContextMenuItem::NewFile);

        // 分隔符区域不可命中（Separator1 位于 NewFile + NewFolder 之后）
        let sep_y = rect.y
            + ExplorerContextMenu::TOP_PADDING
            + 2.0 * ExplorerContextMenu::ITEM_HEIGHT
            + ExplorerContextMenu::SEPARATOR_HEIGHT / 2.0;
        assert_eq!(m.hit_test_menu(rect.x + 20.0, sep_y), None);

        // Refresh 位于 Separator1 之后
        let refresh_y = rect.y
            + ExplorerContextMenu::TOP_PADDING
            + 2.0 * ExplorerContextMenu::ITEM_HEIGHT
            + ExplorerContextMenu::SEPARATOR_HEIGHT
            + ExplorerContextMenu::ITEM_HEIGHT / 2.0;
        let idx = m.hit_test_menu(rect.x + 20.0, refresh_y).unwrap();
        assert_eq!(m.items[idx], ExplorerContextMenuItem::Refresh);
    }

    #[test]
    fn test_hit_test_last_item_copy_path() {
        let m = open_menu_at(50.0, 50.0);
        let rect = m.menu_rect.clone().unwrap();
        // 最后一项 CopyPath 位于底部 padding 之前
        let last_y = rect.bottom()
            - ExplorerContextMenu::BOTTOM_PADDING
            - ExplorerContextMenu::ITEM_HEIGHT / 2.0;
        let idx = m.hit_test_menu(rect.x + 20.0, last_y).unwrap();
        assert_eq!(m.items[idx], ExplorerContextMenuItem::CopyPath);
    }

    #[test]
    fn test_update_hover_changes_state() {
        let mut m = open_menu_at(100.0, 100.0);
        assert_eq!(m.hover_index, None);

        let rect = m.menu_rect.clone().unwrap();
        let first_y = rect.y + ExplorerContextMenu::TOP_PADDING + 1.0;
        // 移到第一项 → hover 变化
        assert!(m.update_hover(rect.x + 20.0, first_y));
        assert_eq!(m.hover_index, Some(0));

        // 同位置再次更新 → 无变化
        assert!(!m.update_hover(rect.x + 20.0, first_y));

        // 移到菜单外 → 清除 hover
        assert!(m.update_hover(10.0, 10.0));
        assert_eq!(m.hover_index, None);
    }

    #[test]
    fn test_update_hover_noop_when_closed() {
        let mut m = ExplorerContextMenu::new();
        assert!(!m.update_hover(50.0, 50.0));
    }

    #[test]
    fn test_separator_is_separator_helper() {
        assert!(ExplorerContextMenuItem::Separator1.is_separator());
        assert!(ExplorerContextMenuItem::Separator2.is_separator());
        assert!(ExplorerContextMenuItem::Separator3.is_separator());
        assert!(!ExplorerContextMenuItem::NewFile.is_separator());
        assert!(!ExplorerContextMenuItem::Refresh.is_separator());
    }

    #[test]
    fn test_separator_label_empty() {
        assert_eq!(ExplorerContextMenuItem::Separator1.label(), "");
        assert_eq!(ExplorerContextMenuItem::NewFile.label(), "新建文件");
        assert_eq!(
            ExplorerContextMenuItem::RevealInExplorer.label(),
            "在文件资源管理器中打开"
        );
    }

    // FileNodeContextMenu 测试
    #[test]
    fn test_file_node_menu_new() {
        let m = FileNodeContextMenu::new();
        assert!(!m.is_open);
        assert_eq!(m.hover_index, None);
        assert!(m.menu_rect.is_none());
        assert!(m.items.contains(&FileNodeContextMenuItem::Rename));
        assert!(m.items.contains(&FileNodeContextMenuItem::Delete));
        assert!(m.items.contains(&FileNodeContextMenuItem::RevealInExplorer));
        assert!(m.items.contains(&FileNodeContextMenuItem::CopyPath));
    }

    #[test]
    fn test_file_node_menu_open_close() {
        let mut m = FileNodeContextMenu::new();
        m.open(100.0, 200.0, 1280.0, 800.0, 5);
        assert!(m.is_open);
        assert_eq!(m.target_node, Some(5));
        m.close();
        assert!(!m.is_open);
        assert_eq!(m.target_node, None);
    }
}
