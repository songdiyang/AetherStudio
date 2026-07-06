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
    pub fn update_git_branch(&mut self, branch: Option<&str>) {
        if let Some(section) = self.sections.get_mut(StatusBarIndex::GitBranch as usize) {
            match branch {
                Some(b) => {
                    section.label = b.to_string();
                    section.icon = Some(crate::icons::IconKind::GitBranch);
                }
                None => {
                    section.label.clear();
                    section.icon = None;
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
    pub fn section_regions(&self, total_width: f32) -> Vec<(usize, f32, f32, f32, f32)> {
        let mut regions = Vec::with_capacity(self.sections.len());
        let mut left_x = 10.0f32;
        let mut right_x = total_width - 10.0f32;

        for (i, section) in self.sections.iter().enumerate() {
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
}
