//! 标签右键上下文菜单
//!
//! 当用户在标签栏的某个标签上右键点击时弹出。
//! 菜单由 Direct2D 自绘（与 explorer_context_menu 保持一致），
//! 命中测试通过 `hit_test` 完成。
//!
//! 菜单项：
//!   关闭 / 关闭其他 / 关闭右侧 / 关闭所有 / 分隔符 /
//!   复制路径 / 在文件资源管理器中打开

/// 标签上下文菜单命令
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabContextMenuCommand {
    Close,
    CloseOthers,
    CloseToTheRight,
    CloseAll,
    Separator,
    CopyPath,
    RevealInExplorer,
}

/// 标签上下文菜单项
#[derive(Clone, Debug)]
pub struct TabContextMenuItem {
    pub label: String,
    pub command: TabContextMenuCommand,
    pub enabled: bool,
}

impl TabContextMenuItem {
    pub fn new(label: &str, command: TabContextMenuCommand) -> Self {
        Self {
            label: label.to_string(),
            command,
            enabled: true,
        }
    }

    pub fn separator() -> Self {
        Self {
            label: "-".to_string(),
            command: TabContextMenuCommand::Separator,
            enabled: false,
        }
    }

    pub fn enabled_if(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 是否为分隔符
    pub fn is_separator(&self) -> bool {
        self.command == TabContextMenuCommand::Separator
    }
}

/// 标签上下文菜单状态
#[derive(Clone, Debug)]
pub struct TabContextMenuState {
    /// 是否可见
    pub visible: bool,
    /// 菜单项列表
    pub items: Vec<TabContextMenuItem>,
    /// 菜单显示位置（左上角）
    pub x: f32,
    pub y: f32,
    /// 菜单宽度
    pub width: f32,
    /// 每项高度
    pub item_height: f32,
    /// 分隔符高度
    pub separator_height: f32,
    /// 顶部 padding
    pub top_padding: f32,
    /// 底部 padding
    pub bottom_padding: f32,
    /// 当前 hover 的项索引
    pub hover_index: Option<usize>,
    /// 触发菜单的标签索引
    pub tab_index: Option<usize>,
}

impl Default for TabContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 220.0,
            item_height: 28.0,
            separator_height: 8.0,
            top_padding: 4.0,
            bottom_padding: 6.0,
            hover_index: None,
            tab_index: None,
        }
    }
}

impl TabContextMenuState {
    /// 单项高度
    pub const ITEM_HEIGHT: f32 = 28.0;
    /// 分隔符高度
    pub const SEPARATOR_HEIGHT: f32 = 8.0;
    /// 菜单宽度
    pub const MENU_WIDTH: f32 = 220.0;
    /// 顶部 padding
    pub const TOP_PADDING: f32 = 4.0;
    /// 底部 padding
    pub const BOTTOM_PADDING: f32 = 6.0;

    /// 为指定标签构建菜单。
    ///
    /// `has_path` 控制复制路径/在资源管理器中打开是否可用。
    pub fn build_for_tab(tab_index: usize, has_path: bool) -> Self {
        Self {
            visible: true,
            items: vec![
                TabContextMenuItem::new("关闭", TabContextMenuCommand::Close),
                TabContextMenuItem::new("关闭其他", TabContextMenuCommand::CloseOthers),
                TabContextMenuItem::new("关闭右侧", TabContextMenuCommand::CloseToTheRight),
                TabContextMenuItem::new("关闭所有", TabContextMenuCommand::CloseAll),
                TabContextMenuItem::separator(),
                TabContextMenuItem::new("复制路径", TabContextMenuCommand::CopyPath)
                    .enabled_if(has_path),
                TabContextMenuItem::new(
                    "在文件资源管理器中打开",
                    TabContextMenuCommand::RevealInExplorer,
                )
                .enabled_if(has_path),
            ],
            x: 0.0,
            y: 0.0,
            width: Self::MENU_WIDTH,
            item_height: Self::ITEM_HEIGHT,
            separator_height: Self::SEPARATOR_HEIGHT,
            top_padding: Self::TOP_PADDING,
            bottom_padding: Self::BOTTOM_PADDING,
            hover_index: None,
            tab_index: Some(tab_index),
        }
    }

    /// 隐藏菜单
    pub fn hide(&mut self) {
        self.visible = false;
        self.hover_index = None;
    }

    /// 计算菜单总高度
    pub fn menu_height(&self) -> f32 {
        let content: f32 = self
            .items
            .iter()
            .map(|item| {
                if item.is_separator() {
                    self.separator_height
                } else {
                    self.item_height
                }
            })
            .sum();
        self.top_padding + content + self.bottom_padding
    }

    /// 打开菜单到指定位置（逻辑像素，客户区坐标），并做窗口边界校正。
    pub fn open_at(&mut self, x: f32, y: f32, window_width: f32, window_height: f32) {
        let max_x = (window_width - self.width).max(4.0);
        let menu_h = self.menu_height();
        let max_y = (window_height - menu_h).max(4.0);
        self.x = x.min(max_x).max(4.0);
        self.y = y.min(max_y).max(4.0);
    }

    /// 点击检测：返回点击的菜单项索引（分隔符不可命中）。
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        if !self.visible {
            return None;
        }
        if x < self.x || x > self.x + self.width {
            return None;
        }
        if y < self.y || y > self.y + self.menu_height() {
            return None;
        }
        let mut item_y = self.y + self.top_padding;
        for (i, item) in self.items.iter().enumerate() {
            let h = if item.is_separator() {
                self.separator_height
            } else {
                self.item_height
            };
            if y >= item_y && y < item_y + h {
                if item.is_separator() {
                    return None;
                }
                return Some(i);
            }
            item_y += h;
        }
        None
    }

    /// 根据鼠标位置更新 hover_index，返回是否有变化
    pub fn update_hover(&mut self, x: f32, y: f32) -> bool {
        if !self.visible {
            return false;
        }
        let new = self.hit_test(x, y);
        if new != self.hover_index {
            self.hover_index = new;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_menu(tab_index: usize, has_path: bool) -> TabContextMenuState {
        TabContextMenuState::build_for_tab(tab_index, has_path)
    }

    #[test]
    fn test_build_for_tab_items() {
        let m = build_menu(0, true);
        assert!(m.visible);
        assert_eq!(m.tab_index, Some(0));
        // 7 项：4 关闭 + 1 分隔符 + 2 路径操作
        assert_eq!(m.items.len(), 7);
        assert_eq!(m.items[0].command, TabContextMenuCommand::Close);
        assert_eq!(m.items[1].command, TabContextMenuCommand::CloseOthers);
        assert_eq!(m.items[2].command, TabContextMenuCommand::CloseToTheRight);
        assert_eq!(m.items[3].command, TabContextMenuCommand::CloseAll);
        assert!(m.items[4].is_separator());
        assert_eq!(m.items[5].command, TabContextMenuCommand::CopyPath);
        assert_eq!(m.items[6].command, TabContextMenuCommand::RevealInExplorer);
    }

    #[test]
    fn test_has_path_controls_enabled() {
        let m = build_menu(0, false);
        // 无路径时复制路径/在资源管理器中打开应禁用
        assert!(!m.items[5].enabled);
        assert!(!m.items[6].enabled);
        // 关闭类操作始终可用
        assert!(m.items[0].enabled);
        assert!(m.items[1].enabled);
        assert!(m.items[2].enabled);
        assert!(m.items[3].enabled);

        let m2 = build_menu(1, true);
        assert!(m2.items[5].enabled);
        assert!(m2.items[6].enabled);
        assert_eq!(m2.tab_index, Some(1));
    }

    #[test]
    fn test_default_invisible() {
        let m = TabContextMenuState::default();
        assert!(!m.visible);
        assert_eq!(m.hover_index, None);
        assert_eq!(m.tab_index, None);
        assert!(m.items.is_empty());
    }

    #[test]
    fn test_hide_clears_state() {
        let mut m = build_menu(2, true);
        m.hover_index = Some(0);
        assert!(m.visible);
        m.hide();
        assert!(!m.visible);
        assert_eq!(m.hover_index, None);
        // tab_index 保留（hide 不清除触发源，便于后续判断）
        assert_eq!(m.tab_index, Some(2));
    }

    #[test]
    fn test_menu_height_includes_padding_and_separator() {
        let m = build_menu(0, true);
        let h = m.menu_height();
        // 6 普通项 + 1 分隔符 + 上下 padding
        let expected = 6.0 * TabContextMenuState::ITEM_HEIGHT
            + 1.0 * TabContextMenuState::SEPARATOR_HEIGHT
            + TabContextMenuState::TOP_PADDING
            + TabContextMenuState::BOTTOM_PADDING;
        assert!((h - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_open_at_clamps_to_window() {
        let mut m = build_menu(0, true);
        // 右下角超出窗口 → 钳制到窗口内
        m.open_at(2000.0, 2000.0, 1280.0, 800.0);
        let max_x = 1280.0 - m.width;
        let max_y = 800.0 - m.menu_height();
        assert!(m.x <= max_x);
        assert!(m.y <= max_y);
        assert!(m.x >= 4.0);
        assert!(m.y >= 4.0);

        // 正常位置不调整
        m.open_at(100.0, 100.0, 1280.0, 800.0);
        assert!((m.x - 100.0).abs() < f32::EPSILON);
        assert!((m.y - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_outside_returns_none() {
        let mut m = build_menu(0, true);
        m.x = 100.0;
        m.y = 100.0;
        // 完全在菜单外
        assert_eq!(m.hit_test(10.0, 10.0), None);
        // 紧贴菜单右边界外侧
        assert_eq!(m.hit_test(m.x + m.width + 1.0, m.y + 5.0), None);
        // 菜单未可见
        m.visible = false;
        assert_eq!(m.hit_test(m.x + 20.0, m.y + 10.0), None);
    }

    #[test]
    fn test_hit_test_first_item() {
        let mut m = build_menu(0, true);
        m.x = 100.0;
        m.y = 100.0;
        // 第一项（关闭）位于 top_padding 起始处
        let first_y = m.y + m.top_padding + 1.0;
        let idx = m.hit_test(m.x + 20.0, first_y).unwrap();
        assert_eq!(m.items[idx].command, TabContextMenuCommand::Close);
    }

    #[test]
    fn test_hit_test_skips_separator() {
        let mut m = build_menu(0, true);
        m.x = 100.0;
        m.y = 100.0;
        // 分隔符位于 4 个普通项之后
        let sep_y = m.y + m.top_padding + 4.0 * m.item_height + m.separator_height / 2.0;
        assert_eq!(m.hit_test(m.x + 20.0, sep_y), None);

        // 分隔符之后的 CopyPath
        let copy_y = m.y + m.top_padding + 4.0 * m.item_height + m.separator_height
            + m.item_height / 2.0;
        let idx = m.hit_test(m.x + 20.0, copy_y).unwrap();
        assert_eq!(m.items[idx].command, TabContextMenuCommand::CopyPath);
    }

    #[test]
    fn test_hit_test_last_item() {
        let mut m = build_menu(0, true);
        m.x = 50.0;
        m.y = 50.0;
        // 最后一项 RevealInExplorer 位于底部 padding 之前
        let last_y = m.y + m.menu_height() - m.bottom_padding - m.item_height / 2.0;
        let idx = m.hit_test(m.x + 20.0, last_y).unwrap();
        assert_eq!(m.items[idx].command, TabContextMenuCommand::RevealInExplorer);
    }

    #[test]
    fn test_update_hover_changes_state() {
        let mut m = build_menu(0, true);
        m.x = 100.0;
        m.y = 100.0;
        assert_eq!(m.hover_index, None);

        let first_y = m.y + m.top_padding + 1.0;
        // 移到第一项 → hover 变化
        assert!(m.update_hover(m.x + 20.0, first_y));
        assert_eq!(m.hover_index, Some(0));

        // 同位置再次更新 → 无变化
        assert!(!m.update_hover(m.x + 20.0, first_y));

        // 移到菜单外 → 清除 hover
        assert!(m.update_hover(10.0, 10.0));
        assert_eq!(m.hover_index, None);
    }

    #[test]
    fn test_update_hover_noop_when_closed() {
        let mut m = TabContextMenuState::default();
        assert!(!m.update_hover(50.0, 50.0));
    }

    #[test]
    fn test_separator_helper() {
        let sep = TabContextMenuItem::separator();
        assert!(sep.is_separator());
        assert!(!sep.enabled);

        let item = TabContextMenuItem::new("关闭", TabContextMenuCommand::Close);
        assert!(!item.is_separator());
        assert!(item.enabled);
    }

    #[test]
    fn test_enabled_if_fluent() {
        let item = TabContextMenuItem::new("复制路径", TabContextMenuCommand::CopyPath)
            .enabled_if(false);
        assert!(!item.enabled);

        let item2 = TabContextMenuItem::new("复制路径", TabContextMenuCommand::CopyPath)
            .enabled_if(true);
        assert!(item2.enabled);
    }
}
