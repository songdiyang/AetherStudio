use crate::layout::ActivityBarView;

/// 活动栏图标项
#[derive(Clone, Debug)]
pub struct ActivityItem {
    pub view: ActivityBarView,
    pub tooltip: String,
    pub is_active: bool,
}

impl ActivityItem {
    pub fn new(view: ActivityBarView) -> Self {
        Self {
            view,
            tooltip: view.label().to_string(),
            is_active: false,
        }
    }
}

/// 活动栏
#[derive(Clone, Debug)]
pub struct ActivityBar {
    pub items: Vec<ActivityItem>,
    pub active_index: usize,
    pub hover_index: Option<usize>,
}

impl ActivityBar {
    pub fn new() -> Self {
        let items = vec![
            ActivityItem::new(ActivityBarView::Explorer),
            ActivityItem::new(ActivityBarView::SourceControl),
            ActivityItem::new(ActivityBarView::Terminal),
            ActivityItem::new(ActivityBarView::Settings),
            ActivityItem::new(ActivityBarView::AiAssistant),
        ];
        Self {
            active_index: 0,
            hover_index: None,
            items,
        }
    }

    /// 获取当前活动视图
    pub fn active_view(&self) -> ActivityBarView {
        self.items[self.active_index].view
    }

    /// 切换到指定视图
    pub fn switch_to(&mut self, index: usize) {
        if index < self.items.len() {
            self.items[self.active_index].is_active = false;
            self.active_index = index;
            self.items[self.active_index].is_active = true;
        }
    }

    /// 根据视图切换
    pub fn switch_to_view(&mut self, view: ActivityBarView) {
        if let Some(index) = self.items.iter().position(|item| item.view == view) {
            self.switch_to(index);
        }
    }

    /// 点击检测（48x48 图标区域）
    pub fn hit_test(&self, x: f32, y: f32, bar_y: f32) -> Option<usize> {
        if x < 0.0 || x > 48.0 {
            return None;
        }
        let icon_size = 48.0;
        let index = ((y - bar_y) / icon_size) as usize;
        if index < self.items.len() {
            Some(index)
        } else {
            None
        }
    }

    /// 获取图标区域
    pub fn icon_region(&self, index: usize, bar_y: f32) -> Option<(f32, f32, f32, f32)> {
        if index >= self.items.len() {
            return None;
        }
        let icon_size = 48.0;
        let y = bar_y + index as f32 * icon_size;
        Some((0.0, y, 48.0, icon_size))
    }
}
