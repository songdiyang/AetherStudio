/// 状态栏区域
#[derive(Clone, Debug)]
pub struct StatusBarSection {
    pub label: String,
    pub width: f32,
    pub clickable: bool,
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
                },
                StatusBarSection {
                    label: "0 错误 0 警告".to_string(),
                    width: 100.0,
                    clickable: true,
                },
                StatusBarSection {
                    label: "Ln 1, Col 1".to_string(),
                    width: 80.0,
                    clickable: false,
                },
                StatusBarSection {
                    label: "UTF-8".to_string(),
                    width: 60.0,
                    clickable: true,
                },
                StatusBarSection {
                    label: "Plain Text".to_string(),
                    width: 80.0,
                    clickable: true,
                },
                StatusBarSection {
                    label: "".to_string(),
                    width: 100.0,
                    clickable: true,
                },
            ],
            hover_index: None,
        }
    }

    /// 更新 Git 分支显示
    pub fn update_git_branch(&mut self, branch: Option<&str>) {
        if let Some(section) = self.sections.get_mut(5) {
            section.label = branch
                .map(|b| format!("{} {}", "🌿", b))
                .unwrap_or_else(|| "".to_string());
        }
    }

    /// 更新行号列号显示
    pub fn update_cursor_position(&mut self, line: usize, col: usize) {
        if let Some(section) = self.sections.get_mut(2) {
            section.label = format!("Ln {}, Col {}", line + 1, col + 1);
        }
    }

    /// 更新语言模式
    pub fn update_language(&mut self, lang: &str) {
        if let Some(section) = self.sections.get_mut(4) {
            section.label = lang.to_string();
        }
    }

    /// 更新状态消息
    pub fn update_status(&mut self, message: &str) {
        if let Some(section) = self.sections.get_mut(0) {
            section.label = message.to_string();
        }
    }

    /// 计算各区域的 x 坐标
    pub fn section_regions(&self, total_width: f32) -> Vec<(f32, f32, f32, f32)> {
        let mut regions = Vec::new();
        let mut x = 10.0f32;

        // 左侧区域（从左边开始）
        for (_i, section) in self.sections.iter().enumerate().take(3) {
            let width = section.width;
            regions.push((x, 0.0, width, 22.0));
            x += width + 10.0;
        }

        // 右侧区域（从右边开始）
        let mut right_x = total_width - 10.0;
        for (_i, section) in self.sections.iter().enumerate().skip(3).rev() {
            let width = section.width;
            right_x -= width;
            regions.push((right_x, 0.0, width, 22.0));
            right_x -= 10.0;
        }

        regions
    }

    /// 点击检测
    pub fn hit_test(&self, x: f32, _y: f32, total_width: f32) -> Option<usize> {
        let regions = self.section_regions(total_width);
        for (i, (rx, _ry, rw, rh)) in regions.iter().enumerate() {
            if x >= *rx && x < *rx + *rw && _y >= 0.0 && _y < *rh {
                return Some(i);
            }
        }
        None
    }
}
