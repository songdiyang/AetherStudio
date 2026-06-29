/// 用户菜单项
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserMenuItem {
    AetherSettings,
    EditorSettings,
    Separator,
    HelpDocs,
    FeatureRequest,
    BugReport,
    Separator2,
    Logout,
}

impl UserMenuItem {
    pub fn label(&self) -> &'static str {
        match self {
            UserMenuItem::AetherSettings => "Aether 设置",
            UserMenuItem::EditorSettings => "编辑器设置",
            UserMenuItem::Separator => "",
            UserMenuItem::HelpDocs => "帮助文档",
            UserMenuItem::FeatureRequest => "提交功能建议",
            UserMenuItem::BugReport => "问题反馈",
            UserMenuItem::Separator2 => "",
            UserMenuItem::Logout => "退出登录",
        }
    }

    pub fn shortcut(&self) -> Option<&'static str> {
        match self {
            UserMenuItem::AetherSettings => Some("Ctrl+Shift+,"),
            UserMenuItem::EditorSettings => Some("Ctrl+,"),
            _ => None,
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(self, UserMenuItem::Separator | UserMenuItem::Separator2)
    }
}

/// 用户菜单状态
#[derive(Clone, Debug)]
pub struct UserMenu {
    /// 菜单是否展开
    pub is_open: bool,
    /// 鼠标悬停的菜单项索引
    pub hover_index: Option<usize>,
    /// 菜单项列表
    pub items: Vec<UserMenuItem>,
    /// 用户名（显示在菜单顶部）
    pub username: String,
    /// 菜单区域（用于点击检测和渲染）
    pub menu_rect: Option<crate::layout::Region>,
}

impl UserMenu {
    pub fn new() -> Self {
        Self {
            is_open: false,
            hover_index: None,
            items: vec![
                UserMenuItem::AetherSettings,
                UserMenuItem::EditorSettings,
                UserMenuItem::Separator,
                UserMenuItem::HelpDocs,
                UserMenuItem::FeatureRequest,
                UserMenuItem::BugReport,
                UserMenuItem::Separator2,
                UserMenuItem::Logout,
            ],
            username: "diyang song".to_string(),
            menu_rect: None,
        }
    }

    /// 切换菜单展开/关闭
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if !self.is_open {
            self.hover_index = None;
        }
    }

    /// 关闭菜单
    pub fn close(&mut self) {
        self.is_open = false;
        self.hover_index = None;
    }

    /// 检测点击是否在用户头像按钮区域
    pub fn hit_test_button(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        titlebar_height: f32,
        window_width: f32,
    ) -> bool {
        // 用户按钮位于标题栏右侧，最小化按钮左侧
        // 按钮区域：32x32，圆形头像
        let btn_size = 28.0;
        let btn_x = window_width - 46.0 * 3.0 - 32.0 * 2.0 - btn_size - 8.0;
        let btn_y = (titlebar_height - btn_size) / 2.0;
        mouse_x >= btn_x
            && mouse_x < btn_x + btn_size
            && mouse_y >= btn_y
            && mouse_y < btn_y + btn_size
    }

    /// 检测点击是否在菜单区域内
    pub fn hit_test_menu(&self, mouse_x: f32, mouse_y: f32) -> Option<usize> {
        let rect = self.menu_rect.clone()?;
        if !rect.contains(mouse_x, mouse_y) {
            return None;
        }

        // 菜单顶部是用户名区域（约40px）
        let header_height = 40.0;
        if mouse_y < rect.y + header_height {
            return None;
        }

        // 计算点击的是哪个菜单项
        let item_y = mouse_y - rect.y - header_height;
        let item_height = 32.0;
        let index = (item_y / item_height) as usize;

        // 跳过分隔符的索引计算
        let mut visible_index = 0;
        let mut real_index = 0;
        for (i, item) in self.items.iter().enumerate() {
            if item.is_separator() {
                continue;
            }
            if visible_index == index {
                real_index = i;
                break;
            }
            visible_index += 1;
        }

        if real_index < self.items.len() {
            Some(real_index)
        } else {
            None
        }
    }

    /// 获取菜单项的渲染区域
    pub fn item_rect(
        &self,
        index: usize,
        menu_x: f32,
        menu_y: f32,
        menu_width: f32,
    ) -> Option<crate::layout::Region> {
        let header_height = 40.0;
        let item_height = 32.0;
        let separator_height = 9.0;

        let mut current_y = menu_y + header_height;
        for (i, item) in self.items.iter().enumerate() {
            if i == index {
                let height = if item.is_separator() {
                    separator_height
                } else {
                    item_height
                };
                return Some(crate::layout::Region::new(
                    menu_x, current_y, menu_width, height,
                ));
            }
            current_y += if item.is_separator() {
                separator_height
            } else {
                item_height
            };
        }
        None
    }

    /// 计算菜单总高度
    pub fn menu_height(&self) -> f32 {
        let header_height = 40.0;
        let item_height = 32.0;
        let separator_height = 9.0;

        let content_height: f32 = self
            .items
            .iter()
            .map(|item| {
                if item.is_separator() {
                    separator_height
                } else {
                    item_height
                }
            })
            .sum();

        header_height + content_height + 8.0 // 底部padding
    }

    /// 计算菜单宽度
    pub fn menu_width(&self) -> f32 {
        220.0
    }
}
