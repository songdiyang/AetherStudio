//! 活动栏右键上下文菜单
//!
//! 当用户在活动栏区域右键点击时弹出。
//! 菜单由 Direct2D 自绘（与 tab_context_menu 保持一致），
//! 命中测试通过 `hit_test` 完成。
//!
//! 菜单项：
//!   隐藏活动栏 / 分隔符 /
//!   资源管理器 / 源代码管理 / 终端 / 远程管理 / AI 助手
//!
//! 当前活动视图的菜单项会显示勾选标记。

use crate::layout::ActivityBarView;

/// 活动栏上下文菜单命令
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityBarContextMenuCommand {
    HideActivityBar,
    CustomizeSort,
    Separator,
    SwitchToExplorer,
    SwitchToSourceControl,
    SwitchToTerminal,
    SwitchToRemoteManager,
    SwitchToAiAssistant,
}

/// 活动栏上下文菜单项
#[derive(Clone, Debug)]
pub struct ActivityBarContextMenuItem {
    pub label: String,
    pub command: ActivityBarContextMenuCommand,
    pub enabled: bool,
    /// 当前活动视图显示勾选
    pub checked: bool,
}

impl ActivityBarContextMenuItem {
    pub fn new(label: &str, command: ActivityBarContextMenuCommand) -> Self {
        Self {
            label: label.to_string(),
            command,
            enabled: true,
            checked: false,
        }
    }

    pub fn separator() -> Self {
        Self {
            label: "-".to_string(),
            command: ActivityBarContextMenuCommand::Separator,
            enabled: false,
            checked: false,
        }
    }

    pub fn checked_if(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// 是否为分隔符
    pub fn is_separator(&self) -> bool {
        self.command == ActivityBarContextMenuCommand::Separator
    }
}

/// 活动栏上下文菜单状态
#[derive(Clone, Debug)]
pub struct ActivityBarContextMenuState {
    /// 是否可见
    pub visible: bool,
    /// 菜单项列表
    pub items: Vec<ActivityBarContextMenuItem>,
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
}

impl Default for ActivityBarContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: Self::MENU_WIDTH,
            item_height: Self::ITEM_HEIGHT,
            separator_height: Self::SEPARATOR_HEIGHT,
            top_padding: Self::TOP_PADDING,
            bottom_padding: Self::BOTTOM_PADDING,
            hover_index: None,
        }
    }
}

impl ActivityBarContextMenuState {
    /// 单项高度
    pub const ITEM_HEIGHT: f32 = 28.0;
    /// 分隔符高度
    pub const SEPARATOR_HEIGHT: f32 = 8.0;
    /// 菜单宽度
    pub const MENU_WIDTH: f32 = 200.0;
    /// 顶部 padding
    pub const TOP_PADDING: f32 = 4.0;
    /// 底部 padding
    pub const BOTTOM_PADDING: f32 = 6.0;

    /// 根据当前活动视图构建菜单，活动视图对应的项显示勾选标记。
    pub fn build(active_view: ActivityBarView) -> Self {
        use ActivityBarView::*;
        Self {
            visible: true,
            items: vec![
                ActivityBarContextMenuItem::new(
                    "隐藏活动栏",
                    ActivityBarContextMenuCommand::HideActivityBar,
                ),
                ActivityBarContextMenuItem::new(
                    "自定义排序",
                    ActivityBarContextMenuCommand::CustomizeSort,
                ),
                ActivityBarContextMenuItem::separator(),
                ActivityBarContextMenuItem::new(
                    "资源管理器",
                    ActivityBarContextMenuCommand::SwitchToExplorer,
                )
                .checked_if(active_view == Explorer),
                ActivityBarContextMenuItem::new(
                    "源代码管理",
                    ActivityBarContextMenuCommand::SwitchToSourceControl,
                )
                .checked_if(active_view == SourceControl),
                ActivityBarContextMenuItem::new(
                    "终端",
                    ActivityBarContextMenuCommand::SwitchToTerminal,
                )
                .checked_if(active_view == Terminal),
                ActivityBarContextMenuItem::new(
                    "远程管理",
                    ActivityBarContextMenuCommand::SwitchToRemoteManager,
                )
                .checked_if(active_view == RemoteManager),
                ActivityBarContextMenuItem::new(
                    "AI 助手",
                    ActivityBarContextMenuCommand::SwitchToAiAssistant,
                )
                .checked_if(active_view == AiAssistant),
            ],
            x: 0.0,
            y: 0.0,
            width: Self::MENU_WIDTH,
            item_height: Self::ITEM_HEIGHT,
            separator_height: Self::SEPARATOR_HEIGHT,
            top_padding: Self::TOP_PADDING,
            bottom_padding: Self::BOTTOM_PADDING,
            hover_index: None,
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

    fn build_menu(active: ActivityBarView) -> ActivityBarContextMenuState {
        ActivityBarContextMenuState::build(active)
    }

    #[test]
    fn test_build_items() {
        let m = build_menu(ActivityBarView::Explorer);
        assert!(m.visible);
        // 8 项：1 隐藏 + 1 自定义排序 + 1 分隔符 + 5 视图切换
        assert_eq!(m.items.len(), 8);
        assert_eq!(m.items[0].command, ActivityBarContextMenuCommand::HideActivityBar);
        assert_eq!(m.items[1].command, ActivityBarContextMenuCommand::CustomizeSort);
        assert!(m.items[2].is_separator());
        assert_eq!(m.items[3].command, ActivityBarContextMenuCommand::SwitchToExplorer);
        assert_eq!(m.items[4].command, ActivityBarContextMenuCommand::SwitchToSourceControl);
        assert_eq!(m.items[5].command, ActivityBarContextMenuCommand::SwitchToTerminal);
        assert_eq!(m.items[6].command, ActivityBarContextMenuCommand::SwitchToRemoteManager);
        assert_eq!(m.items[7].command, ActivityBarContextMenuCommand::SwitchToAiAssistant);
    }

    #[test]
    fn test_checked_marks_active_view() {
        let m = build_menu(ActivityBarView::SourceControl);
        assert!(!m.items[3].checked); // Explorer
        assert!(m.items[4].checked); // SourceControl
        assert!(!m.items[5].checked); // Terminal
        assert!(!m.items[6].checked); // RemoteManager
        assert!(!m.items[7].checked); // AiAssistant

        let m2 = build_menu(ActivityBarView::AiAssistant);
        assert!(m2.items[7].checked);
        assert!(!m2.items[3].checked);
    }

    #[test]
    fn test_default_invisible() {
        let m = ActivityBarContextMenuState::default();
        assert!(!m.visible);
        assert_eq!(m.hover_index, None);
        assert!(m.items.is_empty());
    }

    #[test]
    fn test_hide_clears_state() {
        let mut m = build_menu(ActivityBarView::Explorer);
        m.hover_index = Some(0);
        assert!(m.visible);
        m.hide();
        assert!(!m.visible);
        assert_eq!(m.hover_index, None);
    }

    #[test]
    fn test_menu_height_includes_padding_and_separator() {
        let m = build_menu(ActivityBarView::Explorer);
        let h = m.menu_height();
        // 7 普通项 + 1 分隔符 + 上下 padding
        let expected = 7.0 * ActivityBarContextMenuState::ITEM_HEIGHT
            + 1.0 * ActivityBarContextMenuState::SEPARATOR_HEIGHT
            + ActivityBarContextMenuState::TOP_PADDING
            + ActivityBarContextMenuState::BOTTOM_PADDING;
        assert!((h - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn test_open_at_clamps_to_window() {
        let mut m = build_menu(ActivityBarView::Explorer);
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
        let mut m = build_menu(ActivityBarView::Explorer);
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
        let mut m = build_menu(ActivityBarView::Explorer);
        m.x = 100.0;
        m.y = 100.0;
        // 第一项（隐藏活动栏）位于 top_padding 起始处
        let first_y = m.y + m.top_padding + 1.0;
        let idx = m.hit_test(m.x + 20.0, first_y).unwrap();
        assert_eq!(m.items[idx].command, ActivityBarContextMenuCommand::HideActivityBar);
    }

    #[test]
    fn test_hit_test_skips_separator() {
        let mut m = build_menu(ActivityBarView::Explorer);
        m.x = 100.0;
        m.y = 100.0;
        // 分隔符位于 2 个普通项（隐藏 + 自定义排序）之后
        let sep_y = m.y + m.top_padding + 2.0 * m.item_height + m.separator_height / 2.0;
        assert_eq!(m.hit_test(m.x + 20.0, sep_y), None);

        // 分隔符之后的 Explorer
        let explorer_y = m.y + m.top_padding + 2.0 * m.item_height + m.separator_height
            + m.item_height / 2.0;
        let idx = m.hit_test(m.x + 20.0, explorer_y).unwrap();
        assert_eq!(m.items[idx].command, ActivityBarContextMenuCommand::SwitchToExplorer);
    }

    #[test]
    fn test_hit_test_last_item() {
        let mut m = build_menu(ActivityBarView::Explorer);
        m.x = 50.0;
        m.y = 50.0;
        // 最后一项 AiAssistant 位于底部 padding 之前
        let last_y = m.y + m.menu_height() - m.bottom_padding - m.item_height / 2.0;
        let idx = m.hit_test(m.x + 20.0, last_y).unwrap();
        assert_eq!(m.items[idx].command, ActivityBarContextMenuCommand::SwitchToAiAssistant);
    }

    #[test]
    fn test_update_hover_changes_state() {
        let mut m = build_menu(ActivityBarView::Explorer);
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
        let mut m = ActivityBarContextMenuState::default();
        assert!(!m.update_hover(50.0, 50.0));
    }

    #[test]
    fn test_separator_helper() {
        let sep = ActivityBarContextMenuItem::separator();
        assert!(sep.is_separator());
        assert!(!sep.enabled);

        let item = ActivityBarContextMenuItem::new(
            "隐藏活动栏",
            ActivityBarContextMenuCommand::HideActivityBar,
        );
        assert!(!item.is_separator());
        assert!(item.enabled);
    }

    #[test]
    fn test_checked_if_fluent() {
        let item = ActivityBarContextMenuItem::new(
            "终端",
            ActivityBarContextMenuCommand::SwitchToTerminal,
        )
        .checked_if(true);
        assert!(item.checked);

        let item2 = ActivityBarContextMenuItem::new(
            "终端",
            ActivityBarContextMenuCommand::SwitchToTerminal,
        )
        .checked_if(false);
        assert!(!item2.checked);
    }
}
