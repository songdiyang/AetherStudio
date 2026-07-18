use super::*;

impl EditorState {
    /// SubTask 9.4: 复制文本到剪贴板（公开接口，供标签右键菜单调用）。
    pub fn copy_text_to_clipboard(&mut self, text: &str) -> bool {
        let ok = Self::set_clipboard_text(text);
        if ok {
            self.status_message = "已复制".to_string();
        }
        ok
    }
    /// 复制选中文本到剪贴板
    pub fn copy(&mut self) {
        if let Some(text) = self.get_selected_text() {
            Self::set_clipboard_text(&text);
            self.status_message = "已复制".to_string();
        }
    }
    /// 剪切选中文本到剪贴板
    pub fn cut(&mut self) {
        if let Some(text) = self.get_selected_text() {
            Self::set_clipboard_text(&text);
            self.delete_selection();
            self.status_message = "已剪切".to_string();
        }
    }
    /// 从剪贴板粘贴文本
    pub fn paste(&mut self) {
        if let Some(text) = Self::get_clipboard_text() {
            // 如果有选区，先删除选中内容
            if self.content.selection_start.is_some() && self.content.selection_end.is_some() {
                self.delete_selection();
            }
            let pos = self.cursor_byte_pos();
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.insert(pos, &text);
            self.content.is_dirty = true;
            self.content.buffer_version += 1;

            // 更新光标位置
            let line_breaks = text.matches('\n').count();
            if line_breaks == 0 {
                self.content.cursor_col += text.len();
            } else {
                self.content.cursor_line += line_breaks;
                self.content.cursor_col = text
                    .rsplit_once('\n')
                    .map(|(_, last)| last.len())
                    .unwrap_or(0);
            }

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
                text.len(),
            );
            self.clear_selection();
            self.status_message = "已粘贴".to_string();
        }
    }
    /// 删除选中文本
    pub fn delete_selection(&mut self) {
        let (start_line, start_col) = match self.content.selection_start {
            Some(s) => s,
            None => return,
        };
        let (end_line, end_col) = match self.content.selection_end {
            Some(e) => e,
            None => return,
        };

        let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        let start_byte = self.line_byte_start(first_line) + first_col;
        let end_byte = self.line_byte_start(last_line) + last_col;

        if start_byte < end_byte {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(start_byte, end_byte);
            self.content.is_dirty = true;
            self.content.buffer_version += 1;

            self.content.cursor_line = first_line;
            self.content.cursor_col = first_col;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                start_byte,
                0,
            );
        }
        self.clear_selection();
    }
    /// 全选
    pub fn select_all(&mut self) {
        let last_line = self.content.buffer.len_lines().saturating_sub(1);
        let last_col = self
            .content
            .buffer
            .get_line(last_line)
            .map(|t| t.len())
            .unwrap_or(0);
        self.content.selection_start = Some((0, 0));
        self.content.selection_end = Some((last_line, last_col));
        self.content.cursor_line = last_line;
        self.content.cursor_col = last_col;
        self.is_selecting = false;
    }
    /// 设置剪贴板文本
    pub(super) fn set_clipboard_text(text: &str) -> bool {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
        };
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(None).is_err() {
                return false;
            }
            let _ = EmptyClipboard();

            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let byte_size = wide.len() * 2;

            let hglobal = match GlobalAlloc(GMEM_MOVEABLE, byte_size) {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return false;
                }
            };
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                // H-19: GlobalLock 失败时释放 HGLOBAL
                let _ = GlobalUnlock(hglobal);
                let _ = CloseClipboard();
                return false;
            }
            let dst = ptr as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(hglobal);
            // H-19: SetClipboardData 失败时释放 HGLOBAL，防止内存泄漏
            if SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0)).is_err() {
                // UI-M07: SetClipboardData 失败后 HGLOBAL 所有权未转移，必须手动释放
                extern "system" {
                    fn GlobalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
                }
                GlobalFree(hglobal.0);
                let _ = CloseClipboard();
                return false;
            }
            let _ = CloseClipboard();
            true
        }
    }
    /// 获取剪贴板文本
    pub(crate) fn get_clipboard_text() -> Option<String> {
        use windows::Win32::Foundation::{HANDLE, HGLOBAL};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let result = GetClipboardData(CF_UNICODETEXT)
                .ok()
                .and_then(|handle: HANDLE| {
                    let hglobal = HGLOBAL(handle.0);
                    let ptr = GlobalLock(hglobal);
                    if ptr.is_null() {
                        return None;
                    }
                    let wide_ptr = ptr as *const u16;
                    // UI-C03: 使用 GlobalSize 限制扫描范围，防止越界读
                    let total_bytes = GlobalSize(hglobal) as usize;
                    let max_chars = total_bytes / std::mem::size_of::<u16>();
                    let mut len = 0;
                    while len < max_chars && *wide_ptr.add(len) != 0 {
                        len += 1;
                    }
                    // 如果没有找到 null 终止符，使用全部数据
                    if len >= max_chars {
                        len = max_chars;
                    }
                    let slice = std::slice::from_raw_parts(wide_ptr, len);
                    let _ = GlobalUnlock(hglobal);
                    String::from_utf16(slice).ok()
                });
            let _ = CloseClipboard();
            result
        }
    }
    pub fn insert_char(&mut self, ch: char) {
        // P1-4: 自动配对括号
        if self.try_auto_pair(ch) {
            return;
        }

        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        let text = ch.to_string();
        self.content.buffer.insert(pos, &text);
        self.content.cursor_col += ch.len_utf8();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            ch.len_utf8(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        self.lsp_notify_change();
    }
    /// P1-4: 尝试自动配对括号。
    /// 返回 true 表示已处理（调用方不应再执行默认插入）。
    /// 规则：
    /// 1. 输入开括号 `( [ { ' "`，且无选区：插入配对，光标居中
    /// 2. 输入开括号且有选区：包裹选区（开括号在选区前，闭括号在选区后）
    /// 3. 输入闭括号 `) ] }`，且光标后已是相同闭括号：跳过插入，光标右移
    pub(super) fn try_auto_pair(&mut self, ch: char) -> bool {
        // 开括号 → 闭括号映射
        let pair_close = match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '\'' => Some('\''),
            '"' => Some('"'),
            _ => None,
        };

        // 闭括号跳过逻辑：光标后已是相同闭括号，直接右移光标
        let is_skip_close = matches!(ch, ')' | ']' | '}');
        if is_skip_close {
            if let Some(text) = self.content.buffer.get_line(self.content.cursor_line) {
                if self.content.cursor_col < text.len() {
                    if let Some(next_ch) = text[self.content.cursor_col..].chars().next() {
                        if next_ch == ch {
                            // 跳过插入，光标右移一个字符
                            self.content.cursor_col += ch.len_utf8();
                            return true;
                        }
                    }
                }
            }
            return false;
        }

        let close_ch = match pair_close {
            Some(c) => c,
            None => return false,
        };

        // 检查是否有选区
        let selection = self
            .content
            .selection_start
            .zip(self.content.selection_end)
            .filter(|(s, e)| s != e);

        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // C-05: 使用模式匹配代替 unwrap，避免选择状态不一致时 panic
        if let Some(((sel_start_line, sel_start_col), (sel_end_line, sel_end_col))) = selection {
            // 包裹选区：在选区开始处插入开括号，在选区结束处插入闭括号
            // 确保 start < end
            let (start_line, start_col, end_line, end_col) =
                if (sel_start_line, sel_start_col) <= (sel_end_line, sel_end_col) {
                    (sel_start_line, sel_start_col, sel_end_line, sel_end_col)
                } else {
                    (sel_end_line, sel_end_col, sel_start_line, sel_start_col)
                };

            let start_byte = self.line_col_to_byte(start_line, start_col);
            let end_byte = self.line_col_to_byte(end_line, end_col);

            // 插入闭括号在前（避免位置偏移），开括号在后
            let close_str = close_ch.to_string();
            let open_str = ch.to_string();
            self.content.buffer.insert(end_byte, &close_str);
            self.content.buffer.insert(start_byte, &open_str);

            // REQ-P1-05: 更新光标到选区末尾（闭括号之后）
            // 开括号在 start_byte 插入，若与 end 同行则 end_col 需要加上开括号长度
            let open_shift = if start_line == end_line {
                ch.len_utf8()
            } else {
                0
            };
            self.content.cursor_line = end_line;
            self.content.cursor_col = end_col + open_shift + close_ch.len_utf8();

            // 更新选区：保持选中文本不变，扩展到包含括号
            self.content.selection_start = Some((start_line, start_col));
            self.content.selection_end =
                Some((end_line, end_col + open_shift + close_ch.len_utf8()));

            self.content.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.mark_dirty();
            }
            self.content.buffer_version += 1;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Insert,
                pos,
                close_str.len() + open_str.len(),
            );
            self.status_message = "已修改".to_string();
            self.emit_edit_events();
            return true;
        }

        // 无选区：插入开括号 + 闭括号，光标置于中间
        let pair_text = format!("{}{}", ch, close_ch);
        self.content.buffer.insert(pos, &pair_text);
        // 光标移动到开括号之后（不前进到闭括号）
        self.content.cursor_col += ch.len_utf8();

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            pair_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        true
    }
    pub fn insert_tab(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        let tab_text = "    ";
        self.content.buffer.insert(pos, tab_text);
        self.content.cursor_col += tab_text.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            tab_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
    }
    pub fn insert_newline(&mut self) {
        let pos = self.cursor_byte_pos();
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // 获取当前行的前导空白（用于自动缩进）
        let indent = if let Some(line_text) = self.content.buffer.get_line(self.content.cursor_line)
        {
            let leading_ws: String = line_text
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            leading_ws
        } else {
            String::new()
        };

        // 检测是否需要额外缩进（行尾有 { 或 :）
        let extra_indent =
            if let Some(line_text) = self.content.buffer.get_line(self.content.cursor_line) {
                let trimmed = line_text.trim_end();
                if trimmed.ends_with('{') || trimmed.ends_with(':') {
                    "    "
                } else {
                    ""
                }
            } else {
                ""
            };

        let full_indent = format!("{}{}", indent, extra_indent);
        let insert_text = if full_indent.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", full_indent)
        };

        self.content.buffer.insert(pos, &insert_text);
        self.content.cursor_line += 1;
        self.content.cursor_col = full_indent.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            insert_text.len(),
        );
        self.status_message = "已修改".to_string();
        self.emit_edit_events();
        self.lsp_notify_change();
    }
    pub fn delete_char(&mut self) {
        if self.content.cursor_col > 0 {
            let pos = self.cursor_byte_pos();
            let prev_pos = self.find_prev_char_boundary(pos);
            if prev_pos < pos {
                let before_pieces = self.content.buffer.get_pieces();
                let before_add_len = self.content.buffer.add_buffer_len();
                let cursor_before =
                    CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

                self.content.buffer.delete(prev_pos, pos);
                self.content.cursor_col -= pos - prev_pos;
                self.content.is_dirty = true;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.mark_dirty();
                }
                self.content.buffer_version += 1;

                let cursor_after =
                    CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
                self.content.history.record(
                    before_pieces,
                    before_add_len,
                    cursor_before,
                    cursor_after,
                    OpType::Delete,
                    prev_pos,
                    0,
                );
                self.status_message = "已修改".to_string();
                // REQ-P1-02: 行内退格也需要触发编辑事件，确保脏矩形标记和即时刷新
                self.emit_edit_events();
            }
        } else if self.content.cursor_line > 0 {
            let prev_line = self.content.cursor_line - 1;
            if let Some(prev_text) = self.content.buffer.get_line(prev_line) {
                let prev_len = prev_text.len();
                if let Some(curr_text) = self.content.buffer.get_line(self.content.cursor_line) {
                    let curr_len = curr_text.len();
                    let start = self.line_byte_start(prev_line) + prev_len;
                    let end = start + curr_len + 1;

                    let before_pieces = self.content.buffer.get_pieces();
                    let before_add_len = self.content.buffer.add_buffer_len();
                    let cursor_before =
                        CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

                    self.content.buffer.delete(start, end);
                    self.content.cursor_line = prev_line;
                    self.content.cursor_col = prev_len;
                    self.content.is_dirty = true;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.mark_dirty();
                    }
                    self.content.buffer_version += 1;

                    let cursor_after =
                        CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
                    self.content.history.record(
                        before_pieces,
                        before_add_len,
                        cursor_before,
                        cursor_after,
                        OpType::Delete,
                        start,
                        0,
                    );
                    self.status_message = "已修改".to_string();
                    self.emit_edit_events();
                }
            }
        }
        self.lsp_notify_change();
    }
    pub fn delete_forward(&mut self) {
        let pos = self.cursor_byte_pos();
        let next_pos = self.find_next_char_boundary(pos);
        if next_pos > pos {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();
            let cursor_before =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

            self.content.buffer.delete(pos, next_pos);
            self.content.is_dirty = true;
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.mark_dirty();
            }
            self.content.buffer_version += 1;

            let cursor_after =
                CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_after,
                OpType::Delete,
                pos,
                0,
            );
            self.status_message = "已修改".to_string();
            self.emit_edit_events();
        }
        self.lsp_notify_change();
    }
    /// 多光标编辑操作广播
    /// 将插入、删除等操作应用到所有光标位置
    /// 从后往前执行，避免位置偏移问题
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    pub fn broadcast_insert_char(&mut self, ch: char) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.insert_char(ch);
            return;
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        // 多光标模式：从后往前插入
        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);

            // REQ-P0-03: 记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.insert(pos, &ch.to_string());

            // REQ-P0-03: 记录撤销历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Insert,
                pos,
                ch.len_utf8(),
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.col += ch.len_utf8();
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.status_message = format!("已在 {} 个位置插入", self.multi_cursor.cursor_count());
        self.emit_edit_events();
    }
    /// 多光标删除（退格）广播
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    /// REQ-P2-06: 修正同行多光标位置 — 使用删除偏移量调整而非 find_prev_char_boundary
    pub fn broadcast_delete_char(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.delete_char();
            return;
        }

        // 先计算所有需要删除的位置，同时记录每个删除操作的光标索引和原始列
        // REQ-P2-06: 记录 (cursor_index, line, col) 用于后续位置调整
        // 使用克隆的 (idx, line, col) 避免对 cursors 的长期借用，便于后续可变修改
        let mut delete_info: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
        let mut indexed_cursors: Vec<(usize, usize, usize)> = self
            .multi_cursor
            .cursors
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.line, c.col))
            .collect();
        indexed_cursors.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));

        for (idx, line, col) in &indexed_cursors {
            if *col > 0 {
                let pos = self.line_col_to_byte(*line, *col);
                let prev_pos = self.find_prev_char_boundary(pos);
                if prev_pos < pos {
                    delete_info.push((*idx, *line, *col, prev_pos, pos));
                }
            }
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        // 执行删除（delete_info 已按 line/col 降序排列，从后往前删除）
        for (_, _, _, start, end) in &delete_info {
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.delete(*start, *end);

            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Delete,
                *start,
                0,
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // REQ-P2-06: 正确调整光标位置
        // 对于每个光标，新 col = 原 col - (同行中位于该光标之前或同位置的删除次数)
        // 因为每次删除都会让该位置之后的光标左移 1
        for (idx, line, col) in indexed_cursors.iter() {
            if *col == 0 {
                continue;
            }
            // 统计同行中删除位置 <= 当前光标 col 的次数
            let shifts = delete_info
                .iter()
                .filter(|(_, dline, dcol, _, _)| *dline == *line && *dcol <= *col)
                .count();
            let new_col = col.saturating_sub(shifts);
            self.multi_cursor.cursors[*idx].col = new_col;
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
    }
    /// 多光标插入换行广播
    /// REQ-P0-03: 记录撤销历史，使用 begin_group/end_group 作为原子撤销组
    pub fn broadcast_insert_newline(&mut self) {
        if self.multi_cursor.cursor_count() <= 1 {
            self.insert_newline();
            return;
        }

        // REQ-P0-03: 记录操作前光标位置
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        // REQ-P0-03: 开始撤销组
        self.content.history.begin_group();

        let cursors: Vec<_> = self.multi_cursor.cursors.clone();
        for cursor in cursors.iter().rev() {
            let pos = self.line_col_to_byte(cursor.line, cursor.col);

            // REQ-P0-03: 记录缓冲区状态
            let before_pieces = self.content.buffer.get_pieces();
            let before_add_len = self.content.buffer.add_buffer_len();

            self.content.buffer.insert(pos, "\n");

            // REQ-P0-03: 记录撤销历史
            self.content.history.record(
                before_pieces,
                before_add_len,
                cursor_before,
                cursor_before,
                OpType::Insert,
                pos,
                1,
            );
        }

        // REQ-P0-03: 结束撤销组
        self.content.history.end_group();

        // 更新所有光标位置
        for cursor in &mut self.multi_cursor.cursors {
            cursor.line += 1;
            cursor.col = 0;
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
    }
    /// 撤销
    pub fn undo(&mut self) {
        let current_pieces = self.content.buffer.get_pieces();
        let current_add_len = self.content.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.content
                .history
                .undo(current_pieces, current_add_len, current_cursor)
        {
            self.content.buffer.restore(pieces, add_len);
            self.content.cursor_line = cursor.line;
            self.content.cursor_col = cursor.column;
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已撤销".to_string();
            // REQ-P2-05: 撤销后触发编辑事件，确保脏矩形更新和事件订阅者通知
            self.emit_edit_events();
        }
    }
    /// 重做
    pub fn redo(&mut self) {
        let current_pieces = self.content.buffer.get_pieces();
        let current_add_len = self.content.buffer.add_buffer_len();
        let current_cursor = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some((pieces, add_len, cursor)) =
            self.content
                .history
                .redo(current_pieces, current_add_len, current_cursor)
        {
            self.content.buffer.restore(pieces, add_len);
            self.content.cursor_line = cursor.line;
            self.content.cursor_col = cursor.column;
            self.content.is_dirty = true;
            self.content.buffer_version += 1;
            self.status_message = "已重做".to_string();
            // REQ-P2-05: 重做后触发编辑事件，确保脏矩形更新和事件订阅者通知
            self.emit_edit_events();
        }
    }
    /// P1-6: 切换行注释（按语言决定注释符号）。
    /// 当前行已有注释符号则移除，否则添加。
    pub fn toggle_line_comment(&mut self) {
        let comment_prefix = match self.content.language {
            Language::Rust
            | Language::C
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::Json => "// ",
            Language::Python | Language::Toml => "# ",
            _ => return, // 不支持的语言（如 PlainText/Markdown/Html/Css）直接返回
        };

        let line_idx = self.content.cursor_line;
        let line = match self.content.buffer.get_line(line_idx) {
            Some(s) => s,
            None => return,
        };

        // 检测是否已有注释前缀
        let stripped = line.strip_prefix(comment_prefix);
        let pos = self.line_byte_start(line_idx);
        let before_pieces = self.content.buffer.get_pieces();
        let before_add_len = self.content.buffer.add_buffer_len();
        let cursor_before = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);

        if let Some(_rest) = stripped {
            // 已有注释：移除前缀
            let remove_len = comment_prefix.len();
            self.content.buffer.delete(pos, pos + remove_len);
            // 光标列前移
            self.content.cursor_col = self.content.cursor_col.saturating_sub(remove_len);
        } else {
            // 无注释：在行首添加前缀
            self.content.buffer.insert(pos, comment_prefix);
            // 光标列后移
            self.content.cursor_col += comment_prefix.len();
        }

        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;

        let cursor_after = CursorPosition::new(self.content.cursor_line, self.content.cursor_col);
        self.content.history.record(
            before_pieces,
            before_add_len,
            cursor_before,
            cursor_after,
            OpType::Insert,
            pos,
            comment_prefix.len(),
        );
        self.status_message = "已切换注释".to_string();
    }
    pub fn get_selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.content.selection_start?;
        let (end_line, end_col) = self.content.selection_end?;

        if start_line == end_line {
            let line = self.content.buffer.get_line(start_line)?;
            let start = line.floor_char_boundary(start_col.min(line.len()));
            let end = line.floor_char_boundary(end_col.min(line.len()));
            let (s, e) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            return Some(line[s..e].to_string());
        }

        // Multi-line selection (simplified)
        let mut result = String::new();
        let (first_line, first_col) = if (start_line, start_col) <= (end_line, end_col) {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if (start_line, start_col) <= (end_line, end_col) {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };

        for line_idx in first_line..=last_line {
            if let Some(line) = self.content.buffer.get_line(line_idx) {
                if line_idx == first_line {
                    let start = line.floor_char_boundary(first_col.min(line.len()));
                    result.push_str(&line[start..]);
                } else if line_idx == last_line {
                    let end = line.floor_char_boundary(last_col.min(line.len()));
                    result.push_str(&line[..end]);
                } else {
                    result.push_str(&line);
                }
                if line_idx != last_line {
                    result.push('\n');
                }
            }
        }
        Some(result)
    }
    pub(super) fn selected_text(&self) -> Option<String> {
        let (start_line, start_col) = self.content.selection_start?;
        let (end_line, end_col) = self.content.selection_end?;
        let (first_line, first_col) = if start_line <= end_line {
            (start_line, start_col)
        } else {
            (end_line, end_col)
        };
        let (last_line, last_col) = if start_line <= end_line {
            (end_line, end_col)
        } else {
            (start_line, start_col)
        };
        let start_byte = self.line_byte_start(first_line) + first_col;
        let end_byte = self.line_byte_start(last_line) + last_col;
        if start_byte >= end_byte {
            return None;
        }
        Some(self.content.buffer.get_text(start_byte, end_byte))
    }
}
