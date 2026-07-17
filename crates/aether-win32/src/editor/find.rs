use super::*;

impl EditorState {
    /// 查找所有匹配位置
    /// 优化：缓存查询结果，避免查询未变且文本未变时重复全量扫描
    pub fn find_all(&mut self) {
        self.find_active_index = 0;
        if self.find_query.is_empty() {
            self.find_results.clear();
            self.last_find_query.clear();
            return;
        }
        // 缓存命中：查询和文本版本都未变，跳过搜索
        if self.find_query == self.last_find_query
            && self.find_result_version == self.content.buffer_version
            && !self.find_results.is_empty()
        {
            // 结果已有效，无需重新搜索
            return;
        }
        // 缓存未命中：清空并重新搜索
        self.find_results.clear();
        let query = self.find_query.clone();
        let total_lines = self.content.buffer.len_lines();
        for line_idx in 0..total_lines {
            if let Some(line) = self.content.buffer.get_line(line_idx) {
                let mut start = 0;
                while let Some(pos) = line[start..].find(&query) {
                    let abs_pos = start + pos;
                    self.find_results.push((line_idx, abs_pos));
                    start = abs_pos + query.len();
                    if start >= line.len() {
                        break;
                    }
                }
            }
        }
        // 更新缓存状态
        self.last_find_query = query;
        self.find_result_version = self.content.buffer_version;
    }
    /// 跳转到下一个匹配
    pub fn find_next(&mut self) {
        if self.find_results.is_empty() {
            self.find_all();
        }
        if !self.find_results.is_empty() {
            self.find_active_index = (self.find_active_index + 1) % self.find_results.len();
            let (line, col) = self.find_results[self.find_active_index];
            // P2-6: 选区末尾对齐到字符边界；cursor_col 置于匹配末尾以符合编辑器约定
            let end_col = self.clamp_to_char_boundary(line, col + self.find_query.len());
            self.content.cursor_line = line;
            self.content.cursor_col = end_col;
            // 选中匹配文本
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, end_col));
        }
    }
    /// 跳转到上一个匹配
    pub fn find_prev(&mut self) {
        if self.find_results.is_empty() {
            self.find_all();
        }
        if !self.find_results.is_empty() {
            if self.find_active_index == 0 {
                self.find_active_index = self.find_results.len() - 1;
            } else {
                self.find_active_index -= 1;
            }
            let (line, col) = self.find_results[self.find_active_index];
            // P2-6: 选区末尾对齐到字符边界
            let end_col = self.clamp_to_char_boundary(line, col + self.find_query.len());
            self.content.cursor_line = line;
            self.content.cursor_col = end_col;
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, end_col));
        }
    }
    /// P2-6: 把字节偏移对齐到字符边界（向下取到下一个字符起点）。
    /// 避免 selection_end 落在多字节字符中间导致渲染/截取异常。
    pub(super) fn clamp_to_char_boundary(&self, line_idx: usize, byte_pos: usize) -> usize {
        if let Some(line) = self.content.buffer.get_line(line_idx) {
            let max = line.len();
            if byte_pos >= max {
                return max;
            }
            // 向前微调到字符边界（byte_pos 通常已在边界上，此处做防御性对齐）
            let mut p = byte_pos;
            while p > 0 && !line.is_char_boundary(p) {
                p -= 1;
            }
            p
        } else {
            byte_pos
        }
    }
    /// 替换当前匹配
    pub fn replace_current(&mut self) -> bool {
        if self.find_results.is_empty() || self.find_active_index >= self.find_results.len() {
            return false;
        }
        let (line, col) = self.find_results[self.find_active_index];
        let pos = self.line_byte_start(line) + col;
        let end_pos = pos + self.find_query.len();

        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        self.content.buffer.delete(pos, end_pos);
        self.content.buffer.insert(pos, &self.replace_text);
        self.content.is_dirty = true;
        self.content.buffer_version += 1;

        self.content.cursor_line = line;
        self.content.cursor_col = col + self.replace_text.len();
        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            self.replace_text.len(),
        );

        // 重新查找
        self.find_all();
        true
    }
    /// 替换所有匹配
    /// REQ-P0-02: 使用 begin_group/end_group 包裹，记录撤销历史
    pub fn replace_all(&mut self) -> usize {
        if self.find_query.is_empty() || self.find_query == self.replace_text {
            return 0;
        }
        self.find_all();
        let count = self.find_results.len();
        if count == 0 {
            return 0;
        }

        // REQ-P1-04: 转换为全局字节偏移，避免替换文本含换行符时行号偏移
        let query_len = self.find_query.len();
        let replace_text = self.replace_text.clone();
        let mut global_offsets: Vec<usize> = self
            .find_results
            .iter()
            .map(|(line, col)| self.line_byte_start(*line) + *col)
            .collect();
        // 降序排序：从文件末尾向前替换，前面的位置不受影响
        global_offsets.sort_by(|a, b| b.cmp(a));

        // REQ-P0-02: 记录替换前的光标位置，用于撤销后恢复
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-02: 开始撤销组，所有替换作为一个原子撤销单元
        self.content.history.begin_group();

        for pos in global_offsets {
            let end_pos = pos + query_len;

            // REQ-P0-02: 每次替换前记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.delete(pos, end_pos);
            self.content.buffer.insert(pos, &replace_text);

            // REQ-P0-02: 记录每次替换的编辑历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Replace,
                pos,
                replace_text.len(),
            );
        }

        // REQ-P0-02: 结束撤销组
        self.content.history.end_group();

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.find_results.clear();
        self.find_active_index = 0;
        self.status_message = format!("已替换 {} 处", count);
        self.emit_edit_events();
        count
    }
    /// 切换查找面板
    pub fn toggle_find(&mut self) {
        self.find_visible = !self.find_visible;
        if !self.find_visible {
            self.replace_visible = false;
            self.find_focus = FindReplaceFocus::None;
        } else {
            self.find_focus = FindReplaceFocus::FindQuery;
        }
        if self.find_visible && !self.find_query.is_empty() {
            self.find_all();
        }
    }
    /// 切换替换面板
    pub fn toggle_replace(&mut self) {
        self.replace_visible = !self.replace_visible;
        self.find_visible = self.replace_visible || self.find_visible;
        if !self.find_visible {
            self.find_focus = FindReplaceFocus::None;
        } else {
            self.find_focus = if self.replace_visible {
                FindReplaceFocus::FindQuery
            } else {
                FindReplaceFocus::None
            };
        }
        if self.find_visible && !self.find_query.is_empty() {
            self.find_all();
        }
    }
    /// 关闭查找替换面板
    pub fn close_find_replace(&mut self) {
        self.find_visible = false;
        self.replace_visible = false;
        self.find_focus = FindReplaceFocus::None;
    }
}
