use aether_render::theme::Theme;

/// UI 设置
#[derive(Clone, Debug)]
pub struct UiSettings {
    pub glass_enabled: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            glass_enabled: true,
        }
    }
}

/// 获取玻璃主题（默认）
pub fn glass_theme() -> Theme {
    Theme::glass()
}

/// 获取经典深色主题（不透明回退）
pub fn dark_theme() -> Theme {
    Theme::dark()
}
