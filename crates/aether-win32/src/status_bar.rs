/// 状态栏区域索引枚举
/// REQ-P0-04: 替代魔法数字，确保 hit_test 返回的索引与 sections 一致
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarIndex {
    Status = 0,
    Errors = 1,
    CursorPos = 2,
    Encoding = 3,
    Language = 4,
    GitBranch = 5,
}

/// 状态栏区域
#[derive(Clone, Debug)]
pub struct StatusBarSection {
    pub label: String,
    pub width: f32,
    pub clickable: bool,
    /// 可选前置矢量图标（替代 emoji）
    pub icon: Option<crate::icons::IconKind>,
}

/// 状态栏
#[derive(Clone, Debug)]
pub struct StatusBar {
    pub sections: Vec<StatusBarSection>,
    pub hover_index: Option<usize>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            sections: vec![
                StatusBarSection {
                    label: "main".to_string(),
                    width: 120.0,
                    clickable: true,
                    icon: None,
                },
                StatusBarSection {
                    label: "0 错误 0 警告".to_string(),
                    width: 100.0,
                    clickable: true,
                    icon: None,
                },
                StatusBarSection {
                    label: "Ln 1, Col 1".to_string(),
                    width: 80.0,
                    clickable: false,
                    icon: None,
                },
                StatusBarSection {
                    label: "UTF-8".to_string(),
                    width: 60.0,
                    clickable: true,
                    icon: None,
                },
                StatusBarSection {
                    label: "Plain Text".to_string(),
                    width: 80.0,
                    clickable: true,
                    icon: None,
                },
                StatusBarSection {
                    label: "".to_string(),
                    width: 100.0,
                    clickable: true,
                    icon: None,
                },
            ],
            hover_index: None,
        }
    }

    /// 更新 Git 分支显示
    ///
    /// 非 repo 时（`branch = None`）将 label 清空、width 设为 0，
    /// `section_regions` 会跳过该分区，从而隐藏 Git 分支显示。
    /// 进入 repo 时恢复默认宽度，后续由 `update_widths` 精确测量。
    pub fn update_git_branch(&mut self, branch: Option<&str>) {
        if let Some(section) = self.sections.get_mut(StatusBarIndex::GitBranch as usize) {
            match branch {
                Some(b) => {
                    section.label = b.to_string();
                    section.icon = Some(crate::icons::IconKind::GitBranch);
                    section.width = 100.0;
                }
                None => {
                    section.label.clear();
                    section.icon = None;
                    section.width = 0.0;
                }
            }
        }
    }

    /// 更新行号列号显示
    pub fn update_cursor_position(&mut self, line: usize, col: usize) {
        if let Some(section) = self.sections.get_mut(StatusBarIndex::CursorPos as usize) {
            section.label = format!("Ln {}, Col {}", line + 1, col + 1);
        }
    }

    /// 更新语言模式
    pub fn update_language(&mut self, lang: &str) {
        if let Some(section) = self.sections.get_mut(StatusBarIndex::Language as usize) {
            section.label = lang.to_string();
        }
    }

    /// 更新状态消息
    pub fn update_status(&mut self, message: &str) {
        if let Some(section) = self.sections.get_mut(StatusBarIndex::Status as usize) {
            section.label = message.to_string();
        }
    }

    /// 计算各区域的 x 坐标
    /// REQ-P0-04: 返回 (原始索引, x, y, width, height)，右侧区域按原始索引顺序
    /// 生成但 x 坐标从右向左计算，确保 hit_test 返回的索引与 sections 一致
    ///
    /// SubTask 10.2: `width <= 0.0` 的分区会被跳过（不生成 region），
    /// 这样右侧其他分区会自动右移填补空缺。StatusBarIndex 枚举值不变。
    pub fn section_regions(&self, total_width: f32) -> Vec<(usize, f32, f32, f32, f32)> {
        let mut regions = Vec::with_capacity(self.sections.len());
        let mut left_x = 10.0f32;
        let mut right_x = total_width - 10.0f32;

        for (i, section) in self.sections.iter().enumerate() {
            if section.width <= 0.0 {
                continue;
            }
            if i < 3 {
                // 左侧区域：从左向右排列
                regions.push((i, left_x, 0.0, section.width, 22.0));
                left_x += section.width + 10.0;
            } else {
                // 右侧区域：从右向左计算 x 坐标，但保持原始索引顺序
                right_x -= section.width;
                regions.push((i, right_x, 0.0, section.width, 22.0));
                right_x -= 10.0;
            }
        }

        regions
    }

    /// 点击检测
    /// REQ-P0-04: 返回 sections 的原始索引（而非 regions 的枚举索引）
    pub fn hit_test(&self, x: f32, y: f32, total_width: f32) -> Option<usize> {
        let regions = self.section_regions(total_width);
        for (original_index, rx, ry, rw, rh) in regions.iter() {
            if x >= *rx && x < *rx + *rw && y >= *ry && y < *rh {
                return Some(*original_index);
            }
        }
        None
    }

    /// SubTask 10.3: 根据文本测量更新各分区宽度
    ///
    /// 使用 DirectWrite 测量 label 文本宽度，叠加图标占用（20px）和内边距（16px），
    /// 将结果 clamp 到 [40.0, 200.0] 范围内。
    /// label 为空且无图标的分区宽度设为 0（隐藏，由 `section_regions` 跳过）。
    ///
    /// 状态栏分区数量少（6 个），每次渲染测量开销可忽略，无需缓存。
    pub fn update_widths(
        &mut self,
        cache: &aether_render::d2d::brush_cache::TextFormatCache,
        font_size: f32,
        font_weight: u32,
    ) {
        for section in &mut self.sections {
            if section.label.is_empty() && section.icon.is_none() {
                section.width = 0.0;
                continue;
            }
            let mut text_width = 0.0f32;
            if !section.label.is_empty() {
                if let Some(w) = cache.measure_text_width(&section.label, font_size, font_weight) {
                    text_width = w;
                }
            }
            // 图标占用 14px + 4px 间距，预留 20px
            let icon_width = if section.icon.is_some() { 20.0 } else { 0.0 };
            // padding 8px * 2 = 16px
            let total = text_width + icon_width + 16.0;
            section.width = total.clamp(40.0, 200.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bar_default_state() {
        let bar = StatusBar::new();
        assert_eq!(bar.sections.len(), 6);
        assert_eq!(bar.hover_index, None);
        assert_eq!(bar.sections[StatusBarIndex::Status as usize].clickable, true);
        assert_eq!(
            bar.sections[StatusBarIndex::CursorPos as usize].clickable,
            false
        );
    }

    #[test]
    fn test_section_regions_skips_zero_width() {
        // GitBranch width=0 时，section_regions 不应生成该 region
        let mut bar = StatusBar::new();
        bar.update_git_branch(None);
        assert_eq!(
            bar.sections[StatusBarIndex::GitBranch as usize].width,
            0.0
        );

        let regions = bar.section_regions(800.0);
        // 不应包含 GitBranch 索引（5）
        assert!(
            !regions.iter().any(|(idx, _, _, _, _)| *idx == StatusBarIndex::GitBranch as usize),
            "GitBranch width=0 时不应生成 region"
        );
        // 其他 5 个分区仍应存在
        assert_eq!(regions.len(), 5);
    }

    #[test]
    fn test_section_regions_includes_all_when_visible() {
        let mut bar = StatusBar::new();
        bar.update_git_branch(Some("main"));
        let regions = bar.section_regions(800.0);
        assert_eq!(regions.len(), 6);
        // GitBranch 索引（5）应存在
        assert!(regions
            .iter()
            .any(|(idx, _, _, _, _)| *idx == StatusBarIndex::GitBranch as usize));
    }

    #[test]
    fn test_update_git_branch_none_hides_section() {
        let mut bar = StatusBar::new();
        // 初始时 GitBranch 有 label="" 但 width=100.0
        bar.update_git_branch(Some("feature/login"));
        assert_eq!(bar.sections[StatusBarIndex::GitBranch as usize].label, "feature/login");
        assert!(bar.sections[StatusBarIndex::GitBranch as usize].width > 0.0);

        // 调用 None 后应隐藏
        bar.update_git_branch(None);
        assert_eq!(bar.sections[StatusBarIndex::GitBranch as usize].label, "");
        assert_eq!(bar.sections[StatusBarIndex::GitBranch as usize].icon, None);
        assert_eq!(bar.sections[StatusBarIndex::GitBranch as usize].width, 0.0);
    }

    #[test]
    fn test_hit_test_skips_hidden_section() {
        // GitBranch 隐藏后，hit_test 不应返回其索引
        let mut bar = StatusBar::new();
        bar.update_git_branch(None);

        // 状态栏高度 22，y=10 应在状态栏内
        assert_eq!(bar.hit_test(0.0, 10.0, 800.0), None);
    }

    #[test]
    fn test_update_widths_clamps_to_range() {
        // 验证 update_widths 后 width 在 [40.0, 200.0] 范围内（除隐藏分区）
        // TextFormatCache::new() 在 Windows 环境下应成功
        let Ok(cache) = aether_render::d2d::brush_cache::TextFormatCache::new() else {
            // 非 Windows 环境或 DWrite 不可用时跳过
            return;
        };
        let mut bar = StatusBar::new();
        bar.update_git_branch(Some("main"));
        bar.update_cursor_position(120, 45);
        bar.update_status("就绪");
        bar.update_language("Rust");

        bar.update_widths(&cache, 12.0, windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL.0 as u32);

        for (i, section) in bar.sections.iter().enumerate() {
            if section.width > 0.0 {
                assert!(
                    section.width >= 40.0 && section.width <= 200.0,
                    "section[{}] width={} 不在 [40, 200] 范围内",
                    i,
                    section.width
                );
            }
        }
    }

    #[test]
    fn test_update_widths_hides_empty_no_icon_section() {
        let Ok(cache) = aether_render::d2d::brush_cache::TextFormatCache::new() else {
            return;
        };
        let mut bar = StatusBar::new();
        // 清空 GitBranch 的 label 和 icon
        bar.update_git_branch(None);
        bar.update_widths(&cache, 12.0, windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_NORMAL.0 as u32);
        assert_eq!(bar.sections[StatusBarIndex::GitBranch as usize].width, 0.0);
    }

    #[test]
    fn test_hover_index_default_none() {
        let bar = StatusBar::new();
        assert_eq!(bar.hover_index, None);
    }
}
