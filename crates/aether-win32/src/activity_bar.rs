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
    /// 自定义模式（长按进入）
    pub customize_mode: bool,
    /// 正在拖拽的项索引
    pub drag_index: Option<usize>,
    /// 拖拽放置目标索引
    pub drop_index: Option<usize>,
}

impl ActivityBar {
    pub fn new() -> Self {
        let items = vec![
            ActivityItem::new(ActivityBarView::Explorer),
            ActivityItem::new(ActivityBarView::SourceControl),
            ActivityItem::new(ActivityBarView::RemoteManager),
        ];
        Self {
            active_index: 0,
            hover_index: None,
            items,
            customize_mode: false,
            drag_index: None,
            drop_index: None,
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

    /// 进入自定义模式并开始拖拽指定项
    pub fn begin_drag(&mut self, index: usize) {
        self.customize_mode = true;
        self.drag_index = Some(index);
        self.drop_index = Some(index);
    }

    /// 退出自定义模式
    pub fn exit_customize(&mut self) {
        self.customize_mode = false;
        self.drag_index = None;
        self.drop_index = None;
    }

    /// 根据鼠标 y 计算放置目标索引（0..=items.len()）
    pub fn drop_index_at(&self, y: f32, bar_y: f32) -> usize {
        let icon_size = 48.0;
        let rel = (y - bar_y).max(0.0);
        ((rel / icon_size).round() as usize).min(self.items.len())
    }

    /// 执行重排：将 drag_index 移到 drop_index 位置
    pub fn reorder(&mut self) {
        if let (Some(from), Some(to)) = (self.drag_index, self.drop_index) {
            if from < self.items.len() && to <= self.items.len() && from != to {
                let item = self.items.remove(from);
                let insert_at = if to > from { to - 1 } else { to };
                let insert_at = insert_at.min(self.items.len());
                self.items.insert(insert_at, item);
                // 保持活动项高亮跟随
                if self.active_index == from {
                    self.active_index = insert_at;
                } else if from < self.active_index && to >= self.active_index {
                    self.active_index -= 1;
                } else if from > self.active_index && to <= self.active_index {
                    self.active_index += 1;
                }
            }
        }
    }

    /// 当前顺序的键列表（用于持久化）
    pub fn order_keys(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|i| i.view.key().to_string())
            .collect()
    }

    /// 应用持久化的顺序（保留默认项中存在但配置缺失的视图）
    pub fn apply_order(&mut self, keys: &[String]) {
        let mut new_items: Vec<ActivityItem> = Vec::new();
        let mut used: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for k in keys {
            if let Some(view) = ActivityBarView::from_key(k) {
                if used.insert(view.key()) {
                    let active = self
                        .items
                        .iter()
                        .find(|i| i.view == view)
                        .map(|i| i.is_active)
                        .unwrap_or(false);
                    let mut item = ActivityItem::new(view);
                    item.is_active = active;
                    new_items.push(item);
                }
            }
        }
        // 补充默认顺序中未被配置覆盖的项
        for view in ActivityBarView::default_order() {
            if !used.contains(view.key()) {
                let active = self
                    .items
                    .iter()
                    .find(|i| i.view == view)
                    .map(|i| i.is_active)
                    .unwrap_or(false);
                let mut item = ActivityItem::new(view);
                item.is_active = active;
                new_items.push(item);
            }
        }
        if !new_items.is_empty() {
            self.items = new_items;
            // 修正活动索引
            self.active_index = self.items.iter().position(|i| i.is_active).unwrap_or(0);
        }
    }
}
