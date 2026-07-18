use super::*;

impl EditorState {
    pub(super) fn handle_git_panel_click(&mut self, mouse_x: f32, mouse_y: f32) -> bool {
        if !self.git.is_repo() {
            return false;
        }
        // Git 面板布局：分支(30px) + commit输入(30px) + 按钮(30px) + 分隔(5px) + staged + unstaged + untracked
        // 简化实现：根据鼠标位置检测点击的文件或按钮
        let mut current_y = 10.0f32;
        let sidebar_width = self.layout.sidebar_width;
        let item_height = 22.0f32;
        let section_gap = 8.0f32;

        // 跳过标题和分支区域 (约 70px)
        current_y += 70.0;

        // 检测按钮点击 (Commit, Refresh)
        let button_y = current_y;
        if mouse_y >= button_y && mouse_y < button_y + 26.0 {
            if (10.0..70.0).contains(&mouse_x) {
                // Commit 按钮
                if !self.git.commit_message.is_empty() {
                    let msg = self.git.commit_message.clone();
                    let _ = self.git.commit(&msg);
                    self.git.commit_message.clear();
                }
                return true;
            } else if (80.0..140.0).contains(&mouse_x) {
                // Refresh 按钮
                self.git.refresh();
                return true;
            }
        }
        current_y += 36.0;

        // 检测文件列表点击
        let staged = self.git.staged_files();
        let unstaged = self.git.unstaged_files();
        let untracked = self.git.untracked_files();

        // Staged Changes
        if !staged.is_empty() {
            current_y += section_gap + 20.0; // 标题
            for (file, _status) in &staged {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        // 点击取消暂存
                        let _ = self.git.unstage_file(file);
                    } else {
                        // 点击选择文件，显示 diff
                        self.git.selected_file = Some(file.clone());
                        self.show_git_diff(file, true);
                    }
                    return true;
                }
                current_y += item_height;
            }
            current_y += section_gap;
        }

        // Changes (unstaged)
        if !unstaged.is_empty() {
            current_y += section_gap + 20.0;
            for (file, _status) in &unstaged {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        // 点击暂存
                        let _ = self.git.stage_file(file);
                    } else {
                        self.git.selected_file = Some(file.clone());
                        self.show_git_diff(file, false);
                    }
                    return true;
                }
                current_y += item_height;
            }
            current_y += section_gap;
        }

        // Untracked
        if !untracked.is_empty() {
            current_y += section_gap + 20.0;
            for file in &untracked {
                if mouse_y >= current_y && mouse_y < current_y + item_height {
                    if mouse_x >= sidebar_width - 30.0 && mouse_x < sidebar_width - 10.0 {
                        let _ = self.git.stage_file(file);
                    } else {
                        self.git.selected_file = Some(file.clone());
                    }
                    return true;
                }
                current_y += item_height;
            }
        }

        false
    }
    pub(crate) fn update_git_panel_hover(&mut self, mouse_x: f32, mouse_y: f32) {
        if !self.git.is_repo() {
            self.git.hover_button = None;
            return;
        }
        // 与 handle_git_panel_click 使用一致的布局
        let mut current_y = 10.0f32;
        current_y += 70.0; // 跳过标题和分支区域
        let button_y = current_y;
        if mouse_y >= button_y && mouse_y < button_y + 26.0 {
            if (10.0..70.0).contains(&mouse_x) {
                self.git.hover_button = Some("commit".to_string());
                return;
            } else if (80.0..140.0).contains(&mouse_x) {
                self.git.hover_button = Some("refresh".to_string());
                return;
            }
        }
        self.git.hover_button = None;
    }
    /// 显示 Git diff 视图
    pub fn show_git_diff(&mut self, file: &str, staged: bool) {
        if let Some(path) = &self.current_folder {
            let args = if staged {
                vec!["diff", "--cached", "--", file]
            } else {
                vec!["diff", "--", file]
            };
            let (stdout, stderr, success) = crate::git::GitCommand::exec(path, &args);
            if success {
                let diff_text = if stdout.is_empty() {
                    format!("// 无差异: {}\n", file)
                } else {
                    stdout
                };
                let tab = crate::tabs::Tab::File(crate::tabs::TabContent::with_loaded_buffer(
                    Some(PathBuf::from(format!("diff: {}", file))),
                    PieceTable::from_string(diff_text),
                    Language::PlainText,
                    false,
                ));
                self.open_in_new_tab(tab);
                self.status_message = format!("显示 {} 的差异", file);
            } else {
                self.status_message = format!("获取差异失败: {}", stderr);
            }
        }
    }
}
