use super::*;

impl EditorState {
    /// 滚动
    pub fn scroll(&mut self, delta_y: f32) {
        let line_height = self.text_renderer.line_height();
        let total_height = self.content.buffer.len_lines() as f32 * line_height;
        // UI-M02: 使用实际编辑器区域高度替代硬编码 24.0
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let max_scroll = (total_height - editor_height).max(0.0);
        self.content.scroll_y = (self.content.scroll_y + delta_y).clamp(0.0, max_scroll);
        self.emit_event(crate::events::EditorEvent::Scrolled);
    }

    /// P2.3: 大文件阈值（行数）
    pub(super) const LARGE_FILE_LINE_THRESHOLD: usize = 100_000;
    /// P2.3: 大文件阈值（字节数）
    pub(super) const LARGE_FILE_BYTE_THRESHOLD: usize = 5 * 1024 * 1024;
    /// P2.3: 重建行 Y 偏移前缀和缓存
    pub fn rebuild_line_y_offsets(&mut self) {
        let total_lines = self.content.buffer.len_lines().max(1);
        if self.content.line_y_offsets.len() != total_lines {
            self.content.line_y_offsets.resize(total_lines, 0.0);
        }
        let line_height = self.text_renderer.line_height();
        let mut y = 0.0;
        for (i, offset) in self.content.line_y_offsets.iter_mut().enumerate() {
            *offset = y;
            y += line_height;
            // 大文件时避免浮点误差累积：每 1000 行重新基线
            if i % 1000 == 0 {
                y = (i + 1) as f32 * line_height;
            }
        }
    }
    /// P2.1: 计算当前可见行范围 [start_line, end_line)
    ///
    /// 返回的行号已限制在 [0, total_lines) 内，end_line 为开区间。
    pub fn visible_line_range(&self) -> (usize, usize) {
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_content_region(self.show_tab_bar());
        let height = editor_region.height.max(line_height);
        let total_lines = self.content.cached_lines.len().max(1);
        let start_line = (self.content.scroll_y / line_height) as usize;
        let visible_lines = (height / line_height) as usize + 2;
        let end_line = (start_line + visible_lines).min(total_lines);
        (start_line.min(total_lines), end_line)
    }
    /// P0-3: 水平滚动。
    /// `delta_x` 为正表示向右滚动（查看右侧内容），为负向左。
    /// 最大滚动范围由当前可见行中最长行的像素宽度决定。
    pub fn scroll_horizontal(&mut self, delta_x: f32) {
        let char_width = self.text_renderer.char_width();
        let editor_region = self.layout.editor_region();
        let editor_width = editor_region.width.max(1.0);

        // 计算可见范围内最长行的字符宽度
        let line_height = self.text_renderer.line_height();
        let start_line = (self.content.scroll_y / line_height) as usize;
        let visible_lines = ((editor_region.height / line_height) as usize + 2).max(1);
        let end_line = (start_line + visible_lines).min(self.content.cached_lines.len().max(1));

        let mut max_line_chars: usize = 0;
        for line_idx in start_line..end_line {
            if let Some(text) = self.content.cached_lines.get(line_idx) {
                let chars = text.chars().map(unicode_char_width).sum::<usize>();
                if chars > max_line_chars {
                    max_line_chars = chars;
                }
            }
        }

        // 行号宽度 + 5px 内边距，扣除后为文本可视宽度
        let text_visible_width = (editor_width - 60.0 - 5.0).max(1.0);
        let max_content_width = max_line_chars as f32 * char_width;
        let max_scroll_x = (max_content_width - text_visible_width).max(0.0);

        self.content.scroll_x = (self.content.scroll_x + delta_x).clamp(0.0, max_scroll_x);
        self.emit_event(crate::events::EditorEvent::Scrolled);
    }
    /// P0-3: 重置水平滚动（光标跳转、文件加载时调用）
    pub fn reset_scroll_x(&mut self) {
        self.content.scroll_x = 0.0;
    }
    /// P0-3: 确保光标在水平方向可见，必要时调整 scroll_x。
    /// 在光标移动后调用。
    pub fn ensure_cursor_visible_horizontal(&mut self) {
        let char_width = self.text_renderer.char_width();
        let editor_region = self.layout.editor_region();
        let text_visible_width = (editor_region.width - 60.0 - 5.0).max(1.0);

        // 光标在当前行的字符列
        let cursor_char_col =
            if let Some(text) = self.content.cached_lines.get(self.content.cursor_line) {
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                text[..byte_pos]
                    .chars()
                    .map(unicode_char_width)
                    .sum::<usize>()
            } else {
                0
            };
        let cursor_x = cursor_char_col as f32 * char_width;

        let left = self.content.scroll_x;
        let right = self.content.scroll_x + text_visible_width;

        if cursor_x < left {
            // 光标在可视区左侧，向左滚动
            self.content.scroll_x = cursor_x.max(0.0);
        } else if cursor_x >= right {
            // 光标在可视区右侧，向右滚动（留 1 字符余量）
            self.content.scroll_x = cursor_x - text_visible_width + char_width;
        }
    }
    /// REQ-P1-01: 确保光标在垂直方向可见，必要时调整 scroll_y。
    /// 在光标上下移动后调用。
    pub fn ensure_cursor_visible_vertical(&mut self) {
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let cursor_y = self.content.cursor_line as f32 * line_height;

        if cursor_y < self.content.scroll_y {
            self.content.scroll_y = cursor_y;
        } else if cursor_y + line_height > self.content.scroll_y + editor_height {
            self.content.scroll_y = (cursor_y + line_height - editor_height).max(0.0);
        }
    }
    /// 跳转到指定 1-based 行/列位置，并确保光标可见。
    ///
    /// - line 和 column 均为 1-based（与用户输入一致）。
    /// - 行号/列号越界时会自动钳制到有效范围。
    pub fn goto_position(&mut self, line: usize, column: usize) {
        if self.content.buffer.len_lines() == 0 {
            return;
        }

        let max_line = self.content.buffer.len_lines().saturating_sub(1);
        let target_line = line.saturating_sub(1).min(max_line);

        let line_text = self
            .content
            .buffer
            .get_line(target_line)
            .unwrap_or_default();
        let target_col =
            char_offset_to_byte_offset(&line_text, column.saturating_sub(1)).min(line_text.len());

        self.content.cursor_line = target_line;
        self.content.cursor_col = target_col;
        self.content.selection_start = None;
        self.content.selection_end = None;

        // 同步到当前标签页
        if let Some(crate::tabs::Tab::File(content)) = self.tabs.get_mut(self.active_tab) {
            content.cursor_line = target_line;
            content.cursor_col = target_col;
            content.selection_start = None;
            content.selection_end = None;
        }

        // 垂直滚动：让目标行可见
        let line_height = self.text_renderer.line_height();
        let editor_region = self.layout.editor_region();
        let editor_height = editor_region.height.max(1.0);
        let cursor_y = target_line as f32 * line_height;

        if cursor_y < self.content.scroll_y {
            self.content.scroll_y = cursor_y;
        } else if cursor_y + line_height > self.content.scroll_y + editor_height {
            self.content.scroll_y = (cursor_y + line_height - editor_height).max(0.0);
        }

        // 水平滚动：让目标列可见
        self.ensure_cursor_visible_horizontal();

        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// 侧边栏滚动（文件树虚拟滚动）
    pub fn scroll_sidebar(&mut self, delta_y: f32) {
        match &self.sidebar_content {
            crate::layout::SidebarContent::FileTree => {
                let node_height = 16.0;
                let estimated_nodes = if let Some(tree) = &self.file_tree {
                    tree.len() as f32
                } else {
                    0.0
                };
                let total_height = estimated_nodes * node_height + 20.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.sidebar_scroll_y = (self.sidebar_scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::RemoteFileTree => {
                let node_height = 16.0;
                // P0-1: 按可见节点数（含展开的子节点）估算滚动高度
                let visible_nodes = self
                    .remote_file_tree
                    .as_ref()
                    .map(|t| t.count_visible_nodes())
                    .unwrap_or(0) as f32;
                let total_height = visible_nodes * node_height + 40.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.remote_scroll_y = (self.remote_scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::SourceControlPanel => {
                let item_height = 22.0;
                let staged = self.git.staged_files().len() as f32;
                let unstaged = self.git.unstaged_files().len() as f32;
                let untracked = self.git.untracked_files().len() as f32;
                let total_height = 100.0 + (staged + unstaged + untracked) * item_height + 60.0;
                let sidebar_region = self.layout.sidebar_region();
                let visible_height = sidebar_region.height;
                let max_scroll = (total_height - visible_height).max(0.0);
                self.git.scroll_y = (self.git.scroll_y + delta_y).clamp(0.0, max_scroll);
            }
            crate::layout::SidebarContent::AiAssistantPanel => {
                // 使用渲染实测的最大滚动量（消息换行后高度可变，固定估算会失真）
                let max_scroll = self.ai_panel.content_height;
                let new_scroll = (self.ai_panel.scroll_y + delta_y).clamp(0.0, max_scroll);
                self.ai_panel.scroll_y = new_scroll;
                // 手动滚离底部则取消吸附；回到底部则恢复吸附
                self.ai_panel.stick_to_bottom = new_scroll >= max_scroll - 1.0;
            }
            _ => {}
        }
        self.emit_event(crate::events::EditorEvent::SidebarChanged);
    }
    pub fn move_cursor_left(&mut self) {
        if self.content.cursor_col > 0 {
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                let col = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                if let Some(ch) = text[..col].chars().next_back() {
                    self.content.cursor_col = col - ch.len_utf8();
                } else {
                    self.content.cursor_col = 0;
                }
            }
        } else if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = text.len();
            }
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    pub fn move_cursor_right(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            if self.content.cursor_col < text.len() {
                if let Some(ch) = text[self.content.cursor_col..].chars().next() {
                    self.content.cursor_col += ch.len_utf8();
                }
            } else if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
                self.content.cursor_line += 1;
                self.content.cursor_col = 0;
            }
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    pub fn move_cursor_up(&mut self) {
        if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = self.content.cursor_col.min(text.len());
            }
        }
        // REQ-P1-01: 垂直滚动跟随，确保光标可见
        self.ensure_cursor_visible_vertical();
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    pub fn move_cursor_down(&mut self) {
        if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
            self.content.cursor_line += 1;
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                self.content.cursor_col = self.content.cursor_col.min(text.len());
            }
        }
        // REQ-P1-01: 垂直滚动跟随，确保光标可见
        self.ensure_cursor_visible_vertical();
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    pub fn move_cursor_home(&mut self) {
        self.content.cursor_col = 0;
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    pub fn move_cursor_end(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            self.content.cursor_col = text.len();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: Smart Home - 跳到行首首个非空白字符。
    /// 若光标已在首个非空白位置，再按一次跳到行首（col=0）。
    /// 通过传入 `already_at_smart_home` 判断是否为第二次按 Home。
    pub fn move_cursor_smart_home(&mut self, already_at_smart_home: bool) {
        if already_at_smart_home {
            self.content.cursor_col = 0;
            self.emit_event(crate::events::EditorEvent::CursorMoved);
            return;
        }
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let first_non_ws = text
                .char_indices()
                .skip_while(|(_, c)| c.is_whitespace())
                .map(|(i, _)| i)
                .next()
                .unwrap_or(text.len());
            self.content.cursor_col = first_non_ws;
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: 移动到文件首行
    pub fn move_cursor_file_start(&mut self) {
        self.content.cursor_line = 0;
        self.content.cursor_col = 0;
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: 移动到文件末行末列
    pub fn move_cursor_file_end(&mut self) {
        let last_line = self.content.buffer.len_lines().saturating_sub(1);
        self.content.cursor_line = last_line;
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            self.content.cursor_col = text.len();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: 向左移动一个单词。
    /// 跳过当前空白，再跳到上一个单词边界。
    /// REQ-P0-01: 修复字节/字符索引混淆——cursor_col 是字节偏移，
    /// 必须先转为字符索引再用于 chars Vec 的索引。
    /// REQ-P2-02: 避免每次调用分配 Vec<char>，直接基于字节偏移遍历。
    pub fn move_cursor_word_left(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let text_len = text.len();
            let mut byte_offset = text.floor_char_boundary(self.content.cursor_col.min(text_len));

            // 辅助：取 byte_offset 之前一个字符的字节位置与该字符
            let prev_char = |pos: usize| -> Option<(usize, char)> {
                if pos == 0 {
                    return None;
                }
                let prev_pos = text.floor_char_boundary(pos - 1);
                text[prev_pos..pos].chars().next().map(|c| (prev_pos, c))
            };

            // 向后跳过空白
            while let Some((prev_pos, ch)) = prev_char(byte_offset) {
                if ch.is_whitespace() {
                    byte_offset = prev_pos;
                } else {
                    break;
                }
            }

            // 跳过当前单词（字母数字下划线）或跳过一个符号
            if let Some((prev_pos, ch)) = prev_char(byte_offset) {
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if is_word_char(ch) {
                    while let Some((p, c)) = prev_char(byte_offset) {
                        if is_word_char(c) {
                            byte_offset = p;
                        } else {
                            break;
                        }
                    }
                } else {
                    // 非单词字符：跳过一个符号
                    byte_offset = prev_pos;
                }
            }

            self.content.cursor_col = byte_offset;
        } else if self.content.cursor_line > 0 {
            self.content.cursor_line -= 1;
            self.move_cursor_end();
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: 向右移动一个单词。
    /// REQ-P2-02: 避免每次调用分配 Vec<char>，直接基于字节偏移遍历。
    pub fn move_cursor_word_right(&mut self) {
        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            let text_len = text.len();
            let mut byte_offset = text.floor_char_boundary(self.content.cursor_col.min(text_len));

            // 辅助：取 byte_offset 处字符的字节范围
            let curr_char = |pos: usize| -> Option<(usize, usize, char)> {
                if pos >= text_len {
                    return None;
                }
                let ch = text[pos..].chars().next()?;
                Some((pos, pos + ch.len_utf8(), ch))
            };

            // 向前跳过空白
            while let Some((_, next_pos, ch)) = curr_char(byte_offset) {
                if ch.is_whitespace() {
                    byte_offset = next_pos;
                } else {
                    break;
                }
            }

            // 跳过当前单词或一个符号
            if let Some((_, next_pos, ch)) = curr_char(byte_offset) {
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if is_word_char(ch) {
                    while let Some((_, np, c)) = curr_char(byte_offset) {
                        if is_word_char(c) {
                            byte_offset = np;
                        } else {
                            break;
                        }
                    }
                } else {
                    // 非单词字符：跳过一个符号
                    byte_offset = next_pos;
                }
            }

            self.content.cursor_col = byte_offset;
        } else if self.content.cursor_line + 1 < self.content.buffer.len_lines() {
            self.content.cursor_line += 1;
            self.content.cursor_col = 0;
        }
        self.emit_event(crate::events::EditorEvent::CursorMoved);
    }
    /// P1-6: 在下一行同一列添加光标（Ctrl+Alt+Down）。
    pub fn add_cursor_line_below(&mut self) {
        let line = self.content.cursor_line;
        let col = self.content.cursor_col;
        if line + 1 < self.content.buffer.len_lines() {
            let new_line = line + 1;
            // 钳制 col 到新行长度
            let max_col = self
                .content
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.content.cursor_line = new_line;
            self.content.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }
    /// P1-6: 在上一行同一列添加光标（Ctrl+Alt+Up）。
    pub fn add_cursor_line_above(&mut self) {
        let line = self.content.cursor_line;
        let col = self.content.cursor_col;
        if line > 0 {
            let new_line = line - 1;
            let max_col = self
                .content
                .buffer
                .get_line(new_line)
                .map(|s| s.len())
                .unwrap_or(col);
            self.multi_cursor
                .add_cursor(Cursor::new(new_line, col.min(max_col)));
            self.content.cursor_line = new_line;
            self.content.cursor_col = col.min(max_col);
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }
    /// P1-6: 添加下一个相同单词的光标（Ctrl+D）。
    /// 找到当前选中文本或光标所在单词的下一个出现位置，添加光标。
    pub fn add_cursor_at_next_occurrence(&mut self) {
        // 获取当前要查找的文本（来自选区或光标所在单词）
        let search_text = if let (Some((sline, scol)), Some((eline, ecol))) =
            (self.content.selection_start, self.content.selection_end)
        {
            if sline == eline {
                let s = self.line_col_to_byte(sline, scol);
                let e = self.line_col_to_byte(eline, ecol);
                if s < e {
                    self.content.buffer.get_text(s, e)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            // 取光标所在单词
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                let chars: Vec<char> = text.chars().collect();
                let byte_pos = text.floor_char_boundary(self.content.cursor_col.min(text.len()));
                let char_idx = text[..byte_pos].chars().count();
                let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
                if char_idx < chars.len() && is_word_char(chars[char_idx]) {
                    // 找单词边界
                    let mut start = char_idx;
                    while start > 0 && is_word_char(chars[start - 1]) {
                        start -= 1;
                    }
                    let mut end = char_idx;
                    while end < chars.len() && is_word_char(chars[end]) {
                        end += 1;
                    }
                    let mut byte_start = 0;
                    let mut byte_end = 0;
                    for (i, c) in chars.iter().enumerate() {
                        if i < start {
                            byte_start += c.len_utf8();
                        }
                        if i < end {
                            byte_end += c.len_utf8();
                        }
                    }
                    text[byte_start..byte_end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        };

        if search_text.is_empty() {
            return;
        }

        // 从当前光标位置开始向后查找
        let start_byte = self.cursor_byte_pos() + search_text.len();
        let total_bytes = self.content.buffer.len_bytes();
        let text_after = self.content.buffer.get_text(start_byte, total_bytes);

        if let Some(rel_pos) = text_after.find(&search_text) {
            let abs_byte = start_byte + rel_pos;
            // 转换为 (line, col)
            let (line, col) = self.byte_to_line_col(abs_byte);
            self.multi_cursor.add_cursor(Cursor::new(line, col));
            self.content.cursor_line = line;
            self.content.cursor_col = col;
            self.content.selection_start = Some((line, col));
            self.content.selection_end = Some((line, col + search_text.len()));
            self.status_message =
                format!("已添加光标（共 {} 处）", self.multi_cursor.cursor_count());
        }
    }
    pub fn set_cursor_from_mouse(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        editor_x: f32,
        editor_y: f32,
    ) {
        let line_height = self.text_renderer.line_height();
        let char_width = self.text_renderer.char_width();
        let line_number_width = 40.0;

        // P0-3: 鼠标 x 加上 scroll_x 抵消，确保点击的字符位置正确
        let rel_x = mouse_x - editor_x - line_number_width - 5.0 + self.content.scroll_x;
        let rel_y = mouse_y - editor_y + self.content.scroll_y;

        let line = (rel_y / line_height) as usize;
        let char_col = (rel_x / char_width).max(0.0) as usize;

        let total_lines = self.content.buffer.len_lines();
        self.content.cursor_line = line.min(total_lines.saturating_sub(1));

        if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
            // 将字符列转换为字节偏移，对齐到字符边界
            let mut byte_col = 0usize;
            for (i, ch) in text.chars().enumerate() {
                if i >= char_col {
                    break;
                }
                byte_col += ch.len_utf8();
            }
            self.content.cursor_col = byte_col.min(text.len());
        } else {
            self.content.cursor_col = 0;
        }
    }
    pub fn start_selection(&mut self) {
        self.content.selection_start = Some((self.content.cursor_line, self.content.cursor_col));
        self.content.selection_end = Some((self.content.cursor_line, self.content.cursor_col));
        self.is_selecting = true;
    }
    pub fn update_selection(&mut self) {
        if self.is_selecting {
            self.content.selection_end = Some((self.content.cursor_line, self.content.cursor_col));
        }
    }
    pub fn end_selection(&mut self) {
        self.is_selecting = false;
    }
    pub fn clear_selection(&mut self) {
        self.content.selection_start = None;
        self.content.selection_end = None;
    }
    /// P2-5: 双击选词。基于鼠标位置定位到 (line, byte_col)，然后在当前行
    /// 选择光标下的"词"（连续的字母/数字/下划线为词；否则选单个字符）。
    pub fn select_word_at_mouse(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        editor_x: f32,
        editor_y: f32,
    ) {
        // 先把光标定位到点击位置
        self.set_cursor_from_mouse(mouse_x, mouse_y, editor_x, editor_y);
        let line_idx = self.content.cursor_line;
        let byte_col = self.content.cursor_col;
        let line_text = match self.content.buffer.get_line(line_idx) {
            Some(t) => t,
            None => return,
        };
        // 把字节偏移转换为 char 索引
        let mut byte_to_char: Vec<usize> = Vec::with_capacity(line_text.len() + 1);
        let mut acc = 0usize;
        byte_to_char.push(0);
        for ch in line_text.chars() {
            acc += ch.len_utf8();
            byte_to_char.push(acc);
        }
        let total_chars = line_text.chars().count();
        // byte_col 可能等于 line_text.len()（行末）
        let click_char_idx = byte_to_char
            .iter()
            .position(|&b| b >= byte_col)
            .map(|i| i.min(total_chars))
            .unwrap_or(total_chars);
        let chars: Vec<char> = line_text.chars().collect();
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let (start_char, end_char) = if click_char_idx >= total_chars {
            // 行末：选择最后一字符（若存在）
            let s = total_chars.saturating_sub(1);
            (s, total_chars)
        } else {
            let c = chars[click_char_idx];
            if is_word_char(c) {
                // 向左扩展
                let mut s = click_char_idx;
                while s > 0 && is_word_char(chars[s - 1]) {
                    s -= 1;
                }
                // 向右扩展（end 为排他边界）
                let mut e = click_char_idx + 1;
                while e < total_chars && is_word_char(chars[e]) {
                    e += 1;
                }
                (s, e)
            } else {
                // 分隔符：选这一字符
                (click_char_idx, click_char_idx + 1)
            }
        };
        // 把字符索引转回字节偏移
        let start_byte = byte_to_char.get(start_char).copied().unwrap_or(0);
        let end_byte = byte_to_char
            .get(end_char)
            .copied()
            .unwrap_or(line_text.len());
        self.content.selection_start = Some((line_idx, start_byte));
        self.content.selection_end = Some((line_idx, end_byte));
        self.content.cursor_col = end_byte;
        self.is_selecting = false;
    }
    pub(super) fn cursor_byte_pos(&self) -> usize {
        self.line_byte_start(self.content.cursor_line) + self.content.cursor_col
    }
    pub(super) fn line_byte_start(&self, line_idx: usize) -> usize {
        self.content.buffer.line_start_byte(line_idx)
    }
    /// 将行号+列号转换为字节偏移 - O(1) 行起始 + O(1) 列偏移
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> usize {
        let start = self.content.buffer.line_start_byte(line);
        if let Some(text) = self.content.buffer.get_line(line) {
            start + col.min(text.len())
        } else {
            start
        }
    }
    /// P1-6: 将字节偏移转换为 (line, col) - O(log n) 二分查找行号
    pub(super) fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        let total_lines = self.content.buffer.len_lines();
        if total_lines == 0 {
            return (0, 0);
        }
        // 二分查找：找到第一个 line_start_byte > byte 的行，则该行前一行为目标行
        let mut lo: usize = 0;
        let mut hi: usize = total_lines;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.content.buffer.line_start_byte(mid) <= byte {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let line = lo.saturating_sub(1).min(total_lines.saturating_sub(1));
        let line_start = self.content.buffer.line_start_byte(line);
        let col = byte.saturating_sub(line_start);
        (line, col)
    }
    pub(super) fn find_prev_char_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut p = pos - 1;
        // P4-1: 使用 byte_at 替代 get_text(p, p+1).as_bytes()[0]，避免 String 堆分配
        while p > 0
            && self
                .content
                .buffer
                .byte_at(p)
                .is_some_and(|b| (b & 0xC0) == 0x80)
        {
            p -= 1;
        }
        p
    }
    pub(super) fn find_next_char_boundary(&self, pos: usize) -> usize {
        let total = self.content.buffer.len_bytes();
        if pos >= total {
            return total;
        }
        let mut p = pos + 1;
        // P4-1: 使用 byte_at 避免逐字节 String 分配
        while p < total
            && self
                .content
                .buffer
                .byte_at(p)
                .is_some_and(|b| (b & 0xC0) == 0x80)
        {
            p += 1;
        }
        p
    }
}
