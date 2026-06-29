/// 打开标签页面板状态
#[derive(Clone, Debug)]
pub struct OpenTabsPanel {
    /// 鼠标悬停的标签页索引
    pub hover_tab: Option<usize>,
    /// 鼠标悬停的关闭按钮索引
    pub hover_close: Option<usize>,
    /// 标签页项命中区域缓存 (tab_index, x, y, w, h)
    pub tab_regions: Vec<(usize, f32, f32, f32, f32)>,
    /// 关闭按钮命中区域缓存 (tab_index, x, y, w, h)
    pub close_regions: Vec<(usize, f32, f32, f32, f32)>,
}

impl OpenTabsPanel {
    pub fn new() -> Self {
        Self {
            hover_tab: None,
            hover_close: None,
            tab_regions: Vec::new(),
            close_regions: Vec::new(),
        }
    }

    pub fn clear_regions(&mut self) {
        self.tab_regions.clear();
        self.close_regions.clear();
    }

    pub fn add_tab_region(&mut self, tab_index: usize, x: f32, y: f32, w: f32, h: f32) {
        self.tab_regions.push((tab_index, x, y, w, h));
    }

    pub fn add_close_region(&mut self, tab_index: usize, x: f32, y: f32, w: f32, h: f32) {
        self.close_regions.push((tab_index, x, y, w, h));
    }

    /// 命中检测：标签页项
    pub fn hit_test_tab(&self, x: f32, y: f32) -> Option<usize> {
        for (tab_idx, tx, ty, tw, th) in &self.tab_regions {
            if x >= *tx && x < tx + tw && y >= *ty && y < ty + th {
                return Some(*tab_idx);
            }
        }
        None
    }

    /// 命中检测：关闭按钮
    pub fn hit_test_close(&self, x: f32, y: f32) -> Option<usize> {
        for (tab_idx, cx, cy, cw, ch) in &self.close_regions {
            if x >= *cx && x < cx + cw && y >= *cy && y < cy + ch {
                return Some(*tab_idx);
            }
        }
        None
    }
}
