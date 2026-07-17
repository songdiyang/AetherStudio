use super::*;

impl EditorState {
    /// 切换活动视图到指定视图（非 AI 助手）。
    ///
    /// 更新活动栏高亮、`activity_view`、侧边栏可见性与内容。
    /// 供活动栏左键点击与右键上下文菜单共用。
    pub fn switch_activity_view(&mut self, view: ActivityBarView) {
        self.activity_bar.switch_to_view(view);
        self.activity_view = view;
        // 切换活动栏视图时打开侧边栏：恢复上次保存的宽度
        self.layout.show_sidebar();
        self.sidebar_content = SidebarContent::from_view(view);
    }
    /// P2-3: 调整字体大小（Ctrl+= 放大 / Ctrl+- 缩小 / Ctrl+0 重置）。
    /// delta 为正放大、为负缩小；传 None 则重置为 14.0。
    pub fn zoom_font(&mut self, delta: Option<f32>) {
        let current = self.text_renderer.font_size();
        let new_size = match delta {
            Some(d) => current + d,
            None => 14.0,
        };
        self.text_renderer.set_font_size(new_size);
        // 重建文本格式缓存（与 set_font_size 同步，避免渲染时使用旧格式）
        let fs = self.text_renderer.font_size();
        self.render_ctx.text_format_cache.init_common_formats(fs);
        self.status_message = format!("字体大小: {:.1} px", fs);
    }
    /// 发射一个编辑器事件到事件队列
    pub fn emit_event(&mut self, event: crate::events::EditorEvent) {
        self.event_queue.push(event);
    }
    /// P3.1: 请求内联补全建议（占位实现）
    pub fn request_inline_completion(&mut self) {
        // 收集光标前后文本作为上下文
        let prefix = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|s| {
                let pos = s.floor_char_boundary(self.content.cursor_col.min(s.len()));
                s[..pos].to_string()
            })
            .unwrap_or_default();
        let suffix = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|s| {
                let pos = s.floor_char_boundary(self.content.cursor_col.min(s.len()));
                s[pos..].to_string()
            })
            .unwrap_or_default();

        if let Some(suggestion) = self.inline_completion_service.request(&prefix, &suffix) {
            self.content.inline_completion = Some(crate::inline_completion::InlineCompletion {
                text: suggestion.text,
                trigger_line: self.content.cursor_line,
                trigger_col: self.content.cursor_col,
                version: suggestion.version,
            });
            self.emit_event(crate::events::EditorEvent::CursorMoved);
        }
    }
    /// P3.1: 清除当前内联补全建议
    pub fn clear_inline_completion(&mut self) {
        self.content.inline_completion = None;
    }
    /// P3.3: 接受当前内联补全建议，将建议文本插入到光标处
    pub fn accept_inline_completion(&mut self) -> bool {
        let Some(comp) = self.content.inline_completion.take() else {
            return false;
        };
        if comp.trigger_line != self.content.cursor_line
            || comp.trigger_col != self.content.cursor_col
        {
            return false;
        }
        let pos = self.cursor_byte_pos();
        self.content.buffer.insert(pos, &comp.text);
        self.content.cursor_col += comp.text.len();
        self.content.is_dirty = true;
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.mark_dirty();
        }
        self.content.buffer_version += 1;
        self.emit_edit_events();
        true
    }
    /// 发射文本编辑相关事件（TextChanged + CursorMoved）
    pub(super) fn emit_edit_events(&mut self) {
        self.emit_event(crate::events::EditorEvent::TextChanged {
            start_line: self.content.cursor_line,
            end_line: self.content.cursor_line + 1,
        });
        self.emit_event(crate::events::EditorEvent::CursorMoved);
        // 自动保存：文本变更后按防抖延迟（重）设空闲保存定时器
        self.schedule_autosave_debounce();
    }
    /// 将事件队列中所有事件转换为脏矩形标记
    pub fn flush_events_to_dirty_tracker(&mut self) {
        // 预取布局区域，避免闭包多次借用 self.layout
        let editor_region = self.layout.editor_region();
        let status_region = self.layout.status_bar_region();
        let sidebar_region = self.layout.sidebar_region();
        let right_panel_region = self.layout.right_panel_region();
        let bottom_region = self.layout.bottom_panel_region();
        let line_height = self.text_renderer.line_height();
        // REQ-P1-03: 用字符列（而非字节偏移）计算脏矩形光标 x 坐标，
        // 避免非 ASCII 文本时光标残影/撕裂
        let char_col = self
            .content
            .buffer
            .get_line(self.content.cursor_line)
            .map(|line| {
                let pos = line.floor_char_boundary(self.content.cursor_col.min(line.len()));
                line[..pos].chars().count()
            })
            .unwrap_or(0);
        let cursor_x =
            editor_region.x + 60.0 + 5.0 + char_col as f32 * self.text_renderer.char_width()
                - self.content.scroll_x;
        let cursor_y =
            editor_region.y + self.content.cursor_line as f32 * line_height - self.content.scroll_y;

        self.event_queue
            .drain_to_dirty_tracker(&mut self.dirty_tracker, |event| {
                use crate::events::EditorEvent;
                match event {
                    EditorEvent::TextChanged { .. } => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::CursorMoved => Some((cursor_x, cursor_y, 2.0, line_height)),
                    EditorEvent::SelectionChanged => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::Scrolled => Some((
                        editor_region.x,
                        editor_region.y,
                        editor_region.width,
                        editor_region.height,
                    )),
                    EditorEvent::TabChanged => None, // 由 switch_tab 显式标记局部区域
                    EditorEvent::SidebarChanged => {
                        if sidebar_region.width > 0.0 {
                            Some((
                                sidebar_region.x,
                                sidebar_region.y,
                                sidebar_region.width,
                                sidebar_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::RightPanelChanged => {
                        if right_panel_region.width > 0.0 {
                            Some((
                                right_panel_region.x,
                                right_panel_region.y,
                                right_panel_region.width,
                                right_panel_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::BottomPanelChanged => {
                        if bottom_region.height > 0.0 {
                            Some((
                                bottom_region.x,
                                bottom_region.y,
                                bottom_region.width,
                                bottom_region.height,
                            ))
                        } else {
                            None
                        }
                    }
                    EditorEvent::StatusBarChanged => Some((
                        status_region.x,
                        status_region.y,
                        status_region.width,
                        status_region.height,
                    )),
                    EditorEvent::WindowResized => None, // 全窗口事件在内部处理
                    EditorEvent::FindReplaceChanged => None, // 由调用方显式标记
                    EditorEvent::DialogVisibilityChanged => None, // 全窗口事件在内部处理
                }
            });
    }
    /// P2.3: 根据当前 buffer 大小更新大文件标记
    pub fn update_large_file_flag(&mut self) {
        let line_count = self.content.buffer.len_lines();
        let byte_count = self.content.buffer.len_bytes();
        self.content.is_large_file = line_count > Self::LARGE_FILE_LINE_THRESHOLD
            || byte_count > Self::LARGE_FILE_BYTE_THRESHOLD;
    }
    /// 执行菜单命令
    pub fn execute_command(&mut self, cmd: crate::menu_bar::CommandId, hwnd: HWND) {
        match cmd {
            crate::menu_bar::CommandId::FileNew => {
                self.new_project();
            }
            crate::menu_bar::CommandId::FileNewWindow => {
                // 通过 PostMessage 通知窗口过程创建新窗口
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::WM_APP + 2,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
            }
            crate::menu_bar::CommandId::FileOpen => {
                if let Some(path) = Dialogs::open_file_dialog(hwnd, "打开文件", &[]) {
                    self.load_file(path);
                }
            }
            crate::menu_bar::CommandId::FileOpenFolder => {
                if let Some(path) = Dialogs::open_folder_dialog(hwnd, "打开文件夹") {
                    self.open_folder(path);
                }
            }
            crate::menu_bar::CommandId::FileCloseWorkspace => {
                self.close_workspace();
            }
            crate::menu_bar::CommandId::FileSave => {
                self.save_file();
            }
            crate::menu_bar::CommandId::FileSaveAs => {
                if let Some(path) = Dialogs::save_file_dialog(hwnd, "另存为", "untitled.txt") {
                    self.save_as(path);
                }
            }
            crate::menu_bar::CommandId::FileExit => unsafe {
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            },
            crate::menu_bar::CommandId::EditUndo => {
                self.undo();
            }
            crate::menu_bar::CommandId::EditRedo => {
                self.redo();
            }
            crate::menu_bar::CommandId::EditCut => {
                self.cut();
            }
            crate::menu_bar::CommandId::EditCopy => {
                self.copy();
            }
            crate::menu_bar::CommandId::EditPaste => {
                self.paste();
            }
            crate::menu_bar::CommandId::EditFind => {
                self.toggle_find();
            }
            crate::menu_bar::CommandId::EditReplace => {
                self.toggle_replace();
            }
            crate::menu_bar::CommandId::EditSelectAll => {
                self.select_all();
            }
            crate::menu_bar::CommandId::ViewToggleSidebar => {
                self.layout.sidebar_visible = !self.layout.sidebar_visible;
            }
            crate::menu_bar::CommandId::ViewToggleActivityBar => {
                self.layout.activity_bar_visible = !self.layout.activity_bar_visible;
            }
            crate::menu_bar::CommandId::ViewToggleStatusBar => {
                self.layout.status_bar_visible = !self.layout.status_bar_visible;
            }
            crate::menu_bar::CommandId::ViewZoomIn => {
                self.status_message = "放大功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::ViewZoomOut => {
                self.status_message = "缩小功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::GotoFile => {
                self.status_message = "转到文件功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::GotoLine => {
                self.status_message = "转到行功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::RunStart => {
                self.status_message = "运行功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::RunDebug => {
                self.status_message = "调试功能即将推出".to_string();
            }
            crate::menu_bar::CommandId::SearchGlobal => {
                self.search_panel.toggle();
                if self.search_panel.visible {
                    self.search_panel.search(self.current_folder.as_deref());
                }
            }
            crate::menu_bar::CommandId::AiFixDiagnostics => {
                self.ai_fix_diagnostics();
            }
            crate::menu_bar::CommandId::TerminalNew => {
                self.layout.toggle_terminal_panel();
                if self.layout.bottom_panel_visible {
                    self.terminal_panel.focused = true;
                    self.set_terminal_ime_bypass(true);
                    if !self.terminal_panel.running {
                        let _ = self.terminal_panel.start();
                    }
                    // 启动周期刷新定时器以显示异步 shell 输出
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(
                            self.hwnd, 0xA002, 50, None,
                        );
                    }
                } else {
                    self.terminal_panel.focused = false;
                    self.set_terminal_ime_bypass(false);
                    unsafe {
                        let _ =
                            windows::Win32::UI::WindowsAndMessaging::KillTimer(self.hwnd, 0xA002);
                    }
                }
                self.status_message = if self.layout.bottom_panel_visible {
                    "终端已打开"
                } else {
                    "终端已关闭"
                }
                .to_string();
            }
            crate::menu_bar::CommandId::HelpAbout => {
                self.status_message = "牧羊人编辑器 v0.1.0".to_string();
            }
            crate::menu_bar::CommandId::None => {}
        }
    }
    /// 增量重建缓存：只重建可见行范围内的缓存，大幅减少大文件的词法分析开销
    pub(crate) fn rebuild_cache(&mut self, visible_start: usize, visible_end: usize) {
        let total_lines = self.content.buffer.len_lines().max(1);

        // tree-sitter 优先高亮：返回支持的语言的字符串标识
        // 不支持的语言返回 None，由调用方 fallback 到手写 lexer
        let ts_lang = language_to_ts_str(self.content.language);

        // === P0-3: 后台语法高亮 — 始终 poll，即使在空闲帧 ===
        // 必须在签名检查之前 poll，否则空闲帧（签名匹配）会 early return，
        // 导致后台高亮结果永远无法被消费，tokens 停留在空/旧状态。
        if ts_lang.is_some() && !self.content.is_large_file {
            if let Some(result) = self.bg_highlighter.poll_result() {
                let min_len = result
                    .token_lines
                    .len()
                    .min(self.content.cached_tokens.len());
                for i in 0..min_len {
                    self.content.cached_tokens[i] = result.token_lines[i].clone();
                }
                // 后台高亮结果刚到达：标记编辑器区域脏，使本帧立即以着色重绘，
                // 避免文件打开后停留在无高亮的纯文本状态直到下一次无关重绘。
                let er = self.layout.editor_region();
                self.dirty_tracker.mark_region(
                    er.x,
                    er.y,
                    er.width,
                    er.height,
                    crate::dirty_rect::DirtyRegionType::EditorContent,
                );
            }
        }

        // REQ-P2-01: 变化检测 — 如果 buffer_version、可见范围、总行数均未变化，跳过整个重建
        // 空闲帧（无编辑、无滚动）不会产生任何缓存重建开销
        let signature = (
            self.content.buffer_version,
            visible_start,
            visible_end,
            total_lines,
        );
        if self.content.last_cache_signature == signature
            && self.content.cached_lines.len() == total_lines
        {
            return;
        }
        self.content.last_cache_signature = signature;

        // P2.3: 大文件检测与行偏移缓存
        self.update_large_file_flag();
        self.rebuild_line_y_offsets();

        // 如果行数变化，重新调整缓存向量大小
        if self.content.cached_lines.len() != total_lines {
            self.content
                .cached_lines
                .resize_with(total_lines, String::new);
            self.content
                .cached_tokens
                .resize_with(total_lines, Vec::new);
            self.content.line_cache_versions.resize(total_lines, 0);
        }

        // 调整行号 UTF-16 缓存大小
        if self.cached_line_numbers.len() != total_lines {
            self.cached_line_numbers.resize_with(total_lines, Vec::new);
        }

        // 只重建可见行范围内的缓存（加上前后各2行的缓冲，避免滚动时闪烁）
        let cache_start = visible_start.saturating_sub(2);
        let cache_end = (visible_end + 2).min(total_lines);

        // P2.3: 大文件模式下跳过语法高亮，只缓存行文本
        // 延迟创建 fallback lexer：仅在 tree-sitter 不支持且至少一行需要重建时才创建
        let mut lexer: Option<Box<dyn aether_core::lexer::Lexer>> = None;

        // === P0-3: 后台语法高亮 — 发送请求 ===
        // poll 逻辑已移至签名检查之前，确保空闲帧也能消费后台结果。
        // 此处仅在 buffer_version 变化时发送新请求。
        if let Some(lang) = ts_lang {
            if !self.content.is_large_file
                && self.content.buffer_version != self.hl_request_version
                && !self.bg_highlighter.has_pending()
            {
                let full_text = self.content.buffer.get_all_text();
                let doc_id = self
                    .content
                    .file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                self.bg_highlighter.request(&doc_id, lang, &full_text);
                self.hl_request_version = self.content.buffer_version;
            }
        }

        for i in cache_start..cache_end {
            if self.content.line_cache_versions[i] != self.content.buffer_version {
                let line = self.content.buffer.get_line(i).unwrap_or_default();

                if self.content.is_large_file {
                    // 大文件：跳过语法高亮
                    self.content.cached_lines[i] = line;
                    self.content.cached_tokens[i] = Vec::new();
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                } else if ts_lang.is_some() {
                    // tree-sitter 语言：只更新文本，tokens 由后台线程异步更新
                    // 保留上一版本的 tokens（stale but usable），实现零输入延迟
                    self.content.cached_lines[i] = line;
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                } else {
                    // fallback：手写 lexer（Markdown/Html/Css/PlainText/Image 等）
                    if lexer.is_none() {
                        lexer = Some(self.content.language.create_lexer());
                    }
                    // C-03: lexer 创建可能返回 None（不支持的语言），unwrap 会 panic 并穿越 WndProc
                    let tokens = if let Some(lex) = lexer.as_ref() {
                        lex.lex_full(&line)
                    } else {
                        Vec::new()
                    };
                    self.content.cached_lines[i] = line;
                    self.content.cached_tokens[i] = tokens;
                    self.content.line_cache_versions[i] = self.content.buffer_version;
                }
            }
            // 行号 UTF-16 缓存：如果为空则构建
            if self.cached_line_numbers[i].is_empty() {
                let num_str = format!("{}", i + 1);
                self.cached_line_numbers[i] = num_str.encode_utf16().chain(Some(0)).collect();
            }
        }
    }
}
